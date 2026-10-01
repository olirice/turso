#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;
use turso_core::{MemoryIO, Value};
use turso_pg_head::{Head, Outcome};

const SCHEMA: &str = include_str!("pg_dump/schema.sql");
const STATEMENTS: &str = include_str!("pg_dump/statements.sql");
const CAPTURE: &str = include_str!("pg_dump/capture.json");

const FIRST_USER_OID: i64 = 16384;

#[derive(Deserialize)]
struct Capture {
    columns: Vec<(String, String)>,
    rows: Vec<Vec<Option<String>>>,
}

#[test]
fn every_pg_dump_statement_runs_through_the_pipeline() {
    let statements: Vec<&str> = STATEMENTS.split("\n;;;;\n").collect();
    let captures: BTreeMap<usize, Capture> = parse_indexed(CAPTURE);

    let head =
        Head::open(Arc::new(MemoryIO::new()), "pg-dump-statements.db").expect("the head opens");
    apply_schema(&head, SCHEMA);
    let session = head.connect("postgres").expect("postgres can log in");

    let mut failing = Vec::new();
    let mut wrong_shape = Vec::new();
    let mut wrong_values = Vec::new();
    let mut oid_map = OidMap::default();

    for (index, sql) in statements.iter().enumerate() {
        match session.execute(sql) {
            Ok(outcome) => {
                if let Some(expected) = captures.get(&index) {
                    if let Err(mismatch) = matches_expected_columns(&outcome, expected) {
                        wrong_shape.push(format!("#{index} {sql:?}: {mismatch}"));
                    } else if let Err(mismatch) =
                        matches_expected_values(&outcome, expected, &mut oid_map)
                    {
                        wrong_values.push(format!("#{index} {sql:?}: {mismatch}"));
                    }
                }
            }
            Err(error) => failing.push(format!(
                "#{index} {sql:?}: {} {}",
                error.state.code(),
                error.message
            )),
        }
    }

    assert!(
        wrong_shape.is_empty(),
        "a statement returned the wrong result shape instead of an error:\n{}",
        wrong_shape.join("\n")
    );
    assert!(
        wrong_values.is_empty(),
        "a statement's values disagreed with PostgreSQL 18's own answer over the same schema:\n{}",
        wrong_values.join("\n")
    );
    assert!(
        failing.is_empty(),
        "every pg_dump statement must succeed and match PostgreSQL 18, but:\n{}",
        failing.join("\n")
    );
}

fn apply_schema(head: &Head, schema: &str) {
    let mut role = "postgres".to_string();
    for line in schema.lines() {
        let statement = line.trim();
        if statement.is_empty() {
            continue;
        }
        let statement = statement.strip_suffix(';').unwrap_or(statement);
        if let Some(target) = statement.strip_prefix("set role ") {
            role = target.trim().to_string();
            continue;
        }
        if statement.eq_ignore_ascii_case("reset role") {
            role = "postgres".to_string();
            continue;
        }
        head.connect(&role)
            .unwrap_or_else(|error| panic!("{role} can log in: {error}"))
            .execute(statement)
            .unwrap_or_else(|error| panic!("schema statement {statement:?} failed: {error}"));
    }
}

fn matches_expected_columns(outcome: &Outcome, expected: &Capture) -> Result<(), String> {
    let Outcome::Rows { columns, .. } = outcome else {
        return Err("expected rows back, the head returned a command tag".to_string());
    };
    if columns.len() != expected.columns.len() {
        return Err(format!(
            "expected {} columns, the head returned {}",
            expected.columns.len(),
            columns.len()
        ));
    }
    for (actual, (name, type_name)) in columns.iter().zip(expected.columns.iter()) {
        if &actual.name != name {
            return Err(format!(
                "expected column {name:?}, the head returned {:?}",
                actual.name
            ));
        }
        let Some(expected_oid) = type_oid(type_name) else {
            return Err(format!(
                "no OID mapping in this test for PostgreSQL type {type_name:?}"
            ));
        };
        if actual.type_oid != expected_oid {
            return Err(format!(
                "column {name:?}: expected type OID {expected_oid}, the head returned {}",
                actual.type_oid
            ));
        }
    }
    Ok(())
}

fn matches_expected_values(
    outcome: &Outcome,
    expected: &Capture,
    oid_map: &mut OidMap,
) -> Result<(), String> {
    let Outcome::Rows { values, .. } = outcome else {
        return Err("expected rows back, the head returned a command tag".to_string());
    };
    let expected_rows = &expected.rows;
    let mut claimed = vec![false; expected_rows.len()];
    for (row_index, actual_row) in values.iter().enumerate() {
        if actual_row.len() != expected.columns.len() {
            return Err(format!(
                "row {row_index}: the head returned {} columns, expected {}",
                actual_row.len(),
                expected.columns.len()
            ));
        }
        let rendered: Vec<Option<String>> = actual_row.iter().map(render_value).collect();
        let found = expected_rows
            .iter()
            .enumerate()
            .position(|(candidate, expected_row)| {
                !claimed[candidate] && rows_agree(&expected.columns, &rendered, expected_row)
            });
        match found {
            Some(candidate) => {
                claimed[candidate] = true;
                record_oid_mappings(
                    &expected.columns,
                    &rendered,
                    &expected_rows[candidate],
                    oid_map,
                )
                .map_err(|mismatch| format!("row {row_index} ({rendered:?}): {mismatch}"))?;
            }
            None => {
                return Err(format!(
                    "row {row_index} ({rendered:?}) matches no PostgreSQL row for this statement \
                     (after normalizing project oids and the head's documented deviations)"
                ))
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct OidMap {
    postgres_to_head: BTreeMap<i64, i64>,
    head_to_postgres: BTreeMap<i64, i64>,
}

fn record_oid_mappings(
    columns: &[(String, String)],
    actual_row: &[Option<String>],
    expected_row: &[Option<String>],
    oid_map: &mut OidMap,
) -> Result<(), String> {
    for ((name, type_name), (actual, expected)) in
        columns.iter().zip(actual_row.iter().zip(expected_row))
    {
        if type_name != "oid" {
            continue;
        }
        let (Some(head_oid), Some(postgres_oid)) = (
            actual.as_deref().and_then(|text| text.parse::<i64>().ok()),
            expected
                .as_deref()
                .and_then(|text| text.parse::<i64>().ok()),
        ) else {
            continue;
        };
        if head_oid < FIRST_USER_OID || postgres_oid < FIRST_USER_OID {
            continue;
        }
        match oid_map.postgres_to_head.get(&postgres_oid) {
            Some(&mapped) if mapped != head_oid => {
                return Err(format!(
                    "column {name:?}: PostgreSQL oid {postgres_oid} already maps to head oid \
                     {mapped}, but this row maps it to {head_oid}"
                ))
            }
            _ => {
                oid_map.postgres_to_head.insert(postgres_oid, head_oid);
            }
        }
        match oid_map.head_to_postgres.get(&head_oid) {
            Some(&mapped) if mapped != postgres_oid => {
                return Err(format!(
                    "column {name:?}: head oid {head_oid} already maps to PostgreSQL oid \
                     {mapped}, but this row maps it to {postgres_oid}"
                ))
            }
            _ => {
                oid_map.head_to_postgres.insert(head_oid, postgres_oid);
            }
        }
    }
    Ok(())
}

fn rows_agree(
    columns: &[(String, String)],
    actual_row: &[Option<String>],
    expected_row: &[Option<String>],
) -> bool {
    if actual_row.len() != expected_row.len() {
        return false;
    }
    // Either a `relkind` column says so directly, or the statement's own
    // shape guarantees it: `getIndexes`-style queries describe an index's
    // own `pg_class` row (aliased `t`) but never select `t.relkind`,
    // naming it as `indexdef`'s source (`pg_get_indexdef`) instead.
    let is_index = match columns.iter().position(|(name, _)| name == "relkind") {
        Some(index) => actual_row[index]
            .as_deref()
            .is_some_and(|relkind| relkind == "i"),
        None => columns.iter().any(|(name, _)| name == "indexdef"),
    };
    columns.iter().zip(actual_row.iter().zip(expected_row)).all(
        |((name, type_name), (actual, expected))| {
            values_agree(name, type_name, actual, expected, is_index)
        },
    )
}

fn documented_head_deviation(name: &str, is_index: bool) -> Option<Option<&'static str>> {
    match name {
        "reltype" => Some(Some("0")),
        "relpages" => Some(Some(if is_index { "1" } else { "0" })),
        "reltuples" => Some(Some(if is_index { "0" } else { "-1" })),
        "relallvisible" | "relallfrozen" => Some(Some("0")),
        "toid" | "toastpages" | "toast_reloptions" => Some(None),
        _ => None,
    }
}

fn values_agree(
    name: &str,
    type_name: &str,
    actual: &Option<String>,
    expected: &Option<String>,
    is_index: bool,
) -> bool {
    if let Some(documented) = documented_head_deviation(name, is_index) {
        return actual.as_deref() == documented;
    }
    if type_name == "xid" {
        return true;
    }
    if type_name == "oid" {
        if let (Some(actual_oid), Some(expected_oid)) = (
            actual.as_deref().and_then(|text| text.parse::<i64>().ok()),
            expected
                .as_deref()
                .and_then(|text| text.parse::<i64>().ok()),
        ) {
            if actual_oid >= FIRST_USER_OID && expected_oid >= FIRST_USER_OID {
                return true;
            }
        }
    }
    actual == expected
}

fn render_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        value @ Value::Numeric(_) | value @ Value::Text(_) | value @ Value::Blob(_) => value
            .as_int()
            .map(|integer| integer.to_string())
            .or_else(|| value.to_text().map(str::to_string)),
    }
}

fn type_oid(name: &str) -> Option<i64> {
    Some(match name {
        "\"char\"" => 18,
        "aclitem[]" => 1034,
        "anyarray" => 2277,
        "boolean" => 16,
        "int2vector" => 22,
        "integer" => 23,
        "name" => 19,
        "name[]" => 1003,
        "oid" => 26,
        "oid[]" => 1028,
        "oidvector" => 30,
        "regproc" => 24,
        "smallint" => 21,
        "smallint[]" => 1005,
        "text" => 25,
        "text[]" => 1009,
        "bigint" => 20,
        "real" => 700,
        "xid" => 28,
        _ => return None,
    })
}

fn parse_indexed<T: for<'de> Deserialize<'de>>(json: &str) -> BTreeMap<usize, T> {
    let raw: BTreeMap<String, T> = serde_json::from_str(json).expect("valid json");
    raw.into_iter()
        .map(|(key, value)| {
            let index = key
                .parse()
                .unwrap_or_else(|_| panic!("numeric index, got {key:?}"));
            (index, value)
        })
        .collect()
}
