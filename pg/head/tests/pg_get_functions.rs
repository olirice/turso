#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod common;

use std::sync::Arc;

use common::{run, values};
use turso_core::{MemoryIO, Value};
use turso_pg_head::{Head, Outcome};

fn text(text: &str) -> Value {
    Value::from_text(text.to_string())
}

fn ids(head: &Head, role: &str, sql: &str) -> Vec<i64> {
    match run(head, role, sql) {
        Outcome::Rows { values, .. } => values
            .iter()
            .map(|row| row[0].as_int().expect("the first column is an integer"))
            .collect(),
        Outcome::Command(_) | Outcome::Inserted(_) => panic!("{sql} returned no rows"),
    }
}

// Everything else in this file moved to
// postgres/conformance/head/pg_get_functions.sql (with two refused-by-name
// cases in refusals.sql). What stays here is the one thing psql cannot
// pin: that a policy's stored, re-admitted expression survives a process
// restart (closing and reopening the same on-disk database), which needs
// two separate `Head`s over the same file rather than one live server.
#[test]
fn a_policys_using_expression_persists_across_a_reopen_of_the_same_database() {
    let expected = text(
        "((id > 0) AND (id < 1000) AND (id <= 100) AND (id >= 0) AND (id <> '-5'::integer) AND \
        (id = 7) AND (name IS NOT NULL) AND (tag IS NULL) AND ((id > 0) IS TRUE) AND ((id > 0) \
        IS NOT FALSE) AND ((id < 0) IS FALSE) AND ((id < 0) IS NOT TRUE) AND ((tag = 'q'::text) \
        IS UNKNOWN) AND ((id > 0) IS NOT UNKNOWN) AND (tag IS DISTINCT FROM 'y'::text) AND \
        ((tag || 'z'::text) IS NULL) AND (CURRENT_USER = 'bob'::name) AND ((name IS NULL) OR \
        (NOT (id = 0))) AND ((id)::text = '7'::text) AND (\nCASE id\n    WHEN 7 THEN 1\n    \
        ELSE 0\nEND = 1) AND (\nCASE\n    WHEN (id = 7) THEN 1\n    ELSE 0\nEND = 1) AND (NOT \
        ('a'::text IS DISTINCT FROM 'a'::text)))",
    );
    let using = "(id > 0) AND (id < 1000) AND (id <= 100) AND (id >= 0) AND (id <> -5) \
        AND (id = 7) AND (name IS NOT NULL) AND (tag IS NULL) AND ((id > 0) IS TRUE) \
        AND ((id > 0) IS NOT FALSE) AND ((id < 0) IS FALSE) AND ((id < 0) IS NOT TRUE) \
        AND ((tag = 'q') IS UNKNOWN) AND ((id > 0) IS NOT UNKNOWN) \
        AND (tag IS DISTINCT FROM 'y') AND ((tag || 'z') IS NULL) \
        AND (current_user = 'bob') AND (name IS NULL OR NOT (id = 0)) \
        AND (id::text = '7') \
        AND ((CASE id WHEN 7 THEN 1 ELSE 0 END) = 1) \
        AND ((CASE WHEN id = 7 THEN 1 ELSE 0 END) = 1) \
        AND ('a' IS NOT DISTINCT FROM 'a')";

    let io = Arc::new(MemoryIO::new());
    let path = "policy-round-trip.db";
    let opened = Head::open(io.clone(), path).expect("opening an in-memory head succeeds");
    for sql in [
        "CREATE TABLE t2 (id integer, name text, tag text)",
        "INSERT INTO t2 VALUES (7, 'n1', NULL), (8, 'n2', NULL), (7, NULL, NULL)",
        "CREATE ROLE bob LOGIN",
        "GRANT SELECT ON t2 TO bob",
        &format!("CREATE POLICY q ON t2 FOR SELECT USING ({using})"),
        "ALTER TABLE t2 ENABLE ROW LEVEL SECURITY",
    ] {
        run(&opened, "postgres", sql);
    }
    let pg_get_expr_sql =
        "SELECT pg_get_expr(polqual, polrelid) FROM pg_policy WHERE polrelid = 't2'::regclass";
    assert_eq!(
        values(run(&opened, "postgres", pg_get_expr_sql)),
        vec![vec![expected.clone()]]
    );
    assert_eq!(ids(&opened, "bob", "SELECT id FROM t2"), vec![7]);
    drop(opened);

    let reopened = Head::open(io, path).expect("reopening the same file succeeds");
    assert_eq!(
        values(run(&reopened, "postgres", pg_get_expr_sql)),
        vec![vec![expected]]
    );
    assert_eq!(ids(&reopened, "bob", "SELECT id FROM t2"), vec![7]);
}
