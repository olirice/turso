use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value as Json;

const HEAD_TABLES: &[&str] = &[
    "pg_authid",
    "pg_namespace",
    "pg_database",
    "pg_init_privs",
    "pg_description",
    "pg_class",
    "pg_attribute",
    "pg_index",
    "pg_constraint",
    "pg_policy",
    "pg_depend",
    "pg_shdepend",
    "pg_type",
    "pg_am",
    "pg_tablespace",
    "pg_proc",
];

pub(crate) fn generate() -> Result<String, String> {
    let relations_json = read_json("capture/out/relations.json")?;
    let relations = rows(&relations_json)?;
    let columns_json = read_json("capture/out/columns.json")?;
    let columns = rows(&columns_json)?;
    let indexes_json = read_json("capture/out/indexes.json")?;
    let indexes = rows(&indexes_json)?;
    let types_json = read_json("capture/out/types.json")?;
    let types = rows(&types_json)?;

    let pg_authid_json = read_json("capture/out/bootstrap_rows/pg_authid.json")?;
    let pg_authid = rows(&pg_authid_json)?;
    let pg_namespace_json = read_json("capture/out/bootstrap_rows/pg_namespace.json")?;
    let pg_namespace = rows(&pg_namespace_json)?;
    let pg_am_json = read_json("capture/out/bootstrap_rows/pg_am.json")?;
    let pg_am = rows(&pg_am_json)?;
    let pg_database_json = read_json("capture/out/bootstrap_rows/pg_database.json")?;
    let pg_database = rows(&pg_database_json)?;
    let pg_tablespace_json = read_json("capture/out/bootstrap_rows/pg_tablespace.json")?;
    let pg_tablespace = rows(&pg_tablespace_json)?;
    let pg_init_privs_json = read_json("capture/out/bootstrap_rows/pg_init_privs.json")?;
    let pg_init_privs = rows(&pg_init_privs_json)?;
    let pg_description_json = read_json("capture/out/bootstrap_rows/pg_description.json")?;
    let pg_description = rows(&pg_description_json)?;
    let pg_class_json = read_json("capture/out/bootstrap_rows/pg_class.json")?;
    let pg_class = rows(&pg_class_json)?;
    let pg_attribute_json = read_json("capture/out/bootstrap_rows/pg_attribute.json")?;
    let pg_attribute = rows(&pg_attribute_json)?;
    let pg_index_json = read_json("capture/out/bootstrap_rows/pg_index.json")?;
    let pg_index = rows(&pg_index_json)?;
    let pg_proc_json = read_json("capture/out/bootstrap_rows/pg_proc.json")?;
    let pg_proc = rows(&pg_proc_json)?;

    let relation_index = Relations::load(relations);
    let role_oid = role_oid_map(pg_authid)?;

    let type_oid = relation_index.oid("pg_type")?;
    let authid_oid = relation_index.oid("pg_authid")?;
    let namespace_oid = relation_index.oid("pg_namespace")?;
    let am_oid = relation_index.oid("pg_am")?;
    let database_oid = relation_index.oid("pg_database")?;
    let tablespace_oid = relation_index.oid("pg_tablespace")?;
    let init_privs_oid = relation_index.oid("pg_init_privs")?;
    let description_oid = relation_index.oid("pg_description")?;
    let class_oid = relation_index.oid("pg_class")?;
    let attribute_oid = relation_index.oid("pg_attribute")?;
    let index_oid = relation_index.oid("pg_index")?;
    let proc_oid = relation_index.oid("pg_proc")?;

    let type_schema = table_columns(columns, type_oid)?;
    let authid_schema = table_columns(columns, authid_oid)?;
    let namespace_schema = table_columns(columns, namespace_oid)?;
    let am_schema = table_columns(columns, am_oid)?;
    let database_schema = table_columns(columns, database_oid)?;
    let tablespace_schema = table_columns(columns, tablespace_oid)?;
    let init_privs_schema = table_columns(columns, init_privs_oid)?;
    let description_schema = table_columns(columns, description_oid)?;
    let class_schema = table_columns(columns, class_oid)?;
    let attribute_schema = table_columns(columns, attribute_oid)?;
    let index_schema = table_columns(columns, index_oid)?;
    let proc_schema = table_columns(columns, proc_oid)?;

    let mut out = String::new();
    out.push_str(
        "use super::{AclItem, Alignment, Column, ColumnId, Grantee, Index, Row, Storage, Table, Value};\n",
    );
    out.push_str("use crate::security::privileges::Privileges;\n\n");

    out.push_str(&tables_literal(relations, columns)?);
    out.push('\n');
    out.push_str(&head_tables_literal(relations, columns, &relation_index)?);
    out.push('\n');
    out.push_str(&indexes_literal(indexes)?);
    out.push('\n');
    out.push_str(&rows_literal(
        "TYPES",
        types,
        type_oid,
        &type_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&types_module_literal(types)?);
    out.push('\n');
    let casts_json = read_json("capture/out/casts.json")?;
    let casts = rows(&casts_json)?;
    out.push_str(&casts_module_literal(casts)?);
    out.push('\n');
    out.push_str(&views_module_literal(relations)?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_AUTHID",
        pg_authid,
        authid_oid,
        &authid_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_NAMESPACE",
        pg_namespace,
        namespace_oid,
        &namespace_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_AM", pg_am, am_oid, &am_schema, &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_DATABASE",
        pg_database,
        database_oid,
        &database_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_TABLESPACE",
        pg_tablespace,
        tablespace_oid,
        &tablespace_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_INIT_PRIVS",
        pg_init_privs,
        init_privs_oid,
        &init_privs_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_DESCRIPTION",
        pg_description,
        description_oid,
        &description_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_CLASS_ROWS",
        pg_class,
        class_oid,
        &class_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_ATTRIBUTE_ROWS",
        pg_attribute,
        attribute_oid,
        &attribute_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_INDEX_ROWS",
        pg_index,
        index_oid,
        &index_schema,
        &role_oid,
    )?);
    out.push('\n');
    out.push_str(&rows_literal(
        "PG_PROC",
        pg_proc,
        proc_oid,
        &proc_schema,
        &role_oid,
    )?);

    Ok(out)
}

fn columns_for_reloid<'a>(columns: &[&'a Json], reloid: u32) -> Result<Vec<&'a Json>, String> {
    let mut matching = Vec::new();
    for column in columns {
        if oid_field(column, "reloid")? == reloid {
            int_field(column, "attnum")?;
            matching.push(*column);
        }
    }
    matching.sort_by_key(|column| int_field(column, "attnum").unwrap_or(0));
    Ok(matching)
}

fn tables_literal(relations: &[Json], columns: &[Json]) -> Result<String, String> {
    let all_columns: Vec<&Json> = columns.iter().collect();
    let mut entries = Vec::new();
    for relation in relations {
        if text_field(relation, "relkind")? != "r" {
            continue;
        }
        let oid = oid_field(relation, "oid")?;
        let name = text_literal(text_field(relation, "relname")?);
        let table_columns = columns_for_reloid(&all_columns, oid)?;
        let mut column_literals = Vec::with_capacity(table_columns.len());
        for column in &table_columns {
            column_literals.push(column_literal(column)?);
        }
        entries.push(format!(
            "Table {{ oid: {oid}, name: {name}, columns: &[{}] }}",
            column_literals.join(", ")
        ));
    }
    Ok(format!(
        "pub(super) static TABLES: &[Table] = &[\n    {}\n];\n",
        entries.join(",\n    ")
    ))
}

fn head_tables_literal(
    relations: &[Json],
    columns: &[Json],
    relations_index: &Relations,
) -> Result<String, String> {
    let all_columns: Vec<&Json> = columns.iter().collect();
    let mut out = String::new();
    for table_name in HEAD_TABLES {
        let oid = relations_index.oid(table_name)?;
        let mut captured = false;
        for relation in relations {
            if oid_field(relation, "oid")? == oid {
                captured = true;
                break;
            }
        }
        if !captured {
            return Err(format!("head table {table_name:?} is captured"));
        }
        let table_columns = columns_for_reloid(&all_columns, oid)?;
        let mut column_literals = Vec::with_capacity(table_columns.len());
        for column in &table_columns {
            column_literals.push(column_literal(column)?);
        }

        out.push_str(&format!("pub(crate) mod {table_name} {{\n"));
        out.push_str("    use super::{Alignment, Column, ColumnId, Storage, Table};\n\n");
        out.push_str(&format!(
            "    pub(crate) static TABLE: Table = Table {{ oid: {oid}, name: {}, columns: &[{}] }};\n",
            text_literal(table_name),
            column_literals.join(", "),
        ));
        let mut non_system = Vec::new();
        for column in &table_columns {
            if !bool_field(column, "is_system_column")? {
                non_system.push(*column);
            }
        }
        for column in &non_system {
            let attnum = int_field(column, "attnum")?;
            let type_oid = oid_field(column, "atttypid")?;
            let const_name = text_field(column, "attname")?.to_uppercase();
            out.push_str(&format!(
                "    pub(crate) const {const_name}: ColumnId = ColumnId {{ table_oid: {oid}, attnum: {attnum}, type_oid: {type_oid} }};\n"
            ));
        }
        if WRITTEN_TABLES.contains(table_name) {
            out.push_str(&written_row_literal(&non_system)?);
        }
        out.push_str("}\n\n");
    }
    Ok(out)
}

/// Tables the head itself inserts rows into (`engine/store/write/*.rs`), as
/// opposed to `HEAD_TABLES` at large, most of which the head only ever
/// serves from compiled-in constant rows. Each gets a generated `Row`
/// struct below: every column is a required field, so a column a writer
/// forgets fails to compile rather than silently going into the INSERT as
/// NULL by omission (the bug class `class_defaults()`-style merged
/// defaults/overrides lists risked).
const WRITTEN_TABLES: &[&str] = &[
    "pg_class",
    "pg_attribute",
    "pg_index",
    "pg_constraint",
    "pg_policy",
    "pg_depend",
    "pg_shdepend",
    "pg_authid",
];

/// One Rust field shape per `pg_type` oid a `WRITTEN_TABLES` column actually
/// has. Deliberately closed (`written_field_kind` fails the build on any
/// other oid): a written table gaining a column of a type not listed here
/// needs a real decision about how the head represents it, not a silent
/// fallback.
enum WrittenFieldKind {
    Bool,
    Char,
    /// `name`/`text`: a plain string scalar.
    Name,
    /// `int2`/`int4`/`oid`/`xid`: every integer width this crate ever
    /// writes to a catalog column, held as `i64` (the width the engine
    /// itself stores integers at; narrowing back to the catalog's own
    /// declared width is `pg_attribute`'s job at read time, not this
    /// struct's).
    Integer,
    /// `real` (`pg_class.reltuples`).
    Real,
    /// `int2vector`/`oidvector`: always present, never NULL.
    Vector,
    /// `int2[]`/`oid[]`/`anyarray`: nullable in `pg_constraint`/`pg_policy`
    /// (no foreign key, no column default), required in `pg_index`.
    IntArray,
    /// `aclitem[]`: never populated by `CREATE TABLE` itself (only a later
    /// `GRANT`, which writes it with its own `UPDATE`), so always `None`
    /// here, but still a real, named field a writer must set.
    AclArray,
    /// `text[]` (`reloptions` and friends): never set by this head.
    TextArray,
    /// `pg_node_tree`/`timestamptz`: this head never parses either back
    /// into a structured value; a `pg_node_tree` column that does hold a
    /// value stores the exact text `pg_get_expr` would render (matching
    /// `analyze::types::CatalogOnlyType::PgNodeTree`'s own engine mapping).
    OpaqueText,
}

fn written_field_kind(type_oid: u32) -> Result<WrittenFieldKind, String> {
    match type_oid {
        16 => Ok(WrittenFieldKind::Bool),
        18 => Ok(WrittenFieldKind::Char),
        19 | 25 => Ok(WrittenFieldKind::Name),
        21 | 23 | 26 | 28 => Ok(WrittenFieldKind::Integer),
        700 => Ok(WrittenFieldKind::Real),
        22 | 30 => Ok(WrittenFieldKind::Vector),
        1005 | 1028 | 2277 => Ok(WrittenFieldKind::IntArray),
        1034 => Ok(WrittenFieldKind::AclArray),
        1009 => Ok(WrittenFieldKind::TextArray),
        194 | 1184 => Ok(WrittenFieldKind::OpaqueText),
        other => Err(format!(
            "no generated Row field mapping for pg_type oid {other}; a WRITTEN_TABLES column of \
             this type needs one added to written_field_kind/written_field_type/written_field_cell_expr"
        )),
    }
}

fn written_field_type(kind: &WrittenFieldKind, not_null: bool) -> String {
    match kind {
        WrittenFieldKind::Bool => "bool".to_string(),
        WrittenFieldKind::Char => "char".to_string(),
        WrittenFieldKind::Real => "f64".to_string(),
        WrittenFieldKind::Vector => "Vec<i64>".to_string(),
        WrittenFieldKind::Name if not_null => "String".to_string(),
        WrittenFieldKind::Name => "Option<String>".to_string(),
        WrittenFieldKind::Integer if not_null => "i64".to_string(),
        WrittenFieldKind::Integer => "Option<i64>".to_string(),
        WrittenFieldKind::IntArray if not_null => "Vec<i64>".to_string(),
        WrittenFieldKind::IntArray => "Option<Vec<i64>>".to_string(),
        WrittenFieldKind::AclArray => "Option<Vec<crate::catalog::AclEntry>>".to_string(),
        WrittenFieldKind::TextArray => "Option<Vec<String>>".to_string(),
        WrittenFieldKind::OpaqueText => "Option<String>".to_string(),
    }
}

/// The expression `Row::into_cells` uses to turn one field into a
/// `Cell`. Only `AclArray` needs `?` (decoding an `aclitem[]`'s own
/// `AclEntry` encoding can fail); `into_cells`'s return type is `Result`
/// regardless, so nothing here needs to say which arms use it.
fn written_field_cell_expr(kind: &WrittenFieldKind, not_null: bool, field: &str) -> String {
    match kind {
        WrittenFieldKind::Bool => format!(
            "crate::lower::sql::Cell::Scalar(turso_core::Value::from_i64(i64::from(self.{field})))"
        ),
        // PostgreSQL's own `charout`: the NUL byte is its C-string
        // terminator, so a `"char"` value of code zero prints as empty
        // text, never a one-byte string holding `'\0'`
        // (`analyze::types::LiteralFold::Char`, probed against a live
        // PostgreSQL 18.6).
        WrittenFieldKind::Char => format!(
            "crate::lower::sql::Cell::Scalar(turso_core::Value::from_text(if self.{field} == '\\0' {{ String::new() }} else {{ self.{field}.to_string() }}))"
        ),
        WrittenFieldKind::Real => {
            format!("crate::lower::sql::Cell::Scalar(turso_core::Value::from_f64(self.{field}))")
        }
        WrittenFieldKind::Vector => format!(
            "crate::lower::sql::Cell::Array(self.{field}.into_iter().map(turso_core::Value::from_i64).collect())"
        ),
        WrittenFieldKind::Name if not_null => {
            format!("crate::lower::sql::Cell::Scalar(turso_core::Value::from_text(self.{field}))")
        }
        WrittenFieldKind::Name => format!(
            "match self.{field} {{ Some(value) => crate::lower::sql::Cell::Scalar(turso_core::Value::from_text(value)), None => crate::lower::sql::Cell::Scalar(turso_core::Value::Null) }}"
        ),
        WrittenFieldKind::Integer if not_null => {
            format!("crate::lower::sql::Cell::Scalar(turso_core::Value::from_i64(self.{field}))")
        }
        WrittenFieldKind::Integer => format!(
            "match self.{field} {{ Some(value) => crate::lower::sql::Cell::Scalar(turso_core::Value::from_i64(value)), None => crate::lower::sql::Cell::Scalar(turso_core::Value::Null) }}"
        ),
        WrittenFieldKind::IntArray if not_null => format!(
            "crate::lower::sql::Cell::Array(self.{field}.into_iter().map(turso_core::Value::from_i64).collect())"
        ),
        WrittenFieldKind::IntArray => format!(
            "match self.{field} {{ Some(values) => crate::lower::sql::Cell::Array(values.into_iter().map(turso_core::Value::from_i64).collect()), None => crate::lower::sql::Cell::Scalar(turso_core::Value::Null) }}"
        ),
        WrittenFieldKind::AclArray => format!(
            "match self.{field} {{ Some(entries) => crate::lower::sql::Cell::Array(entries.into_iter().map(crate::catalog::AclEntry::to_value).collect::<Result<Vec<_>, _>>()?), None => crate::lower::sql::Cell::Scalar(turso_core::Value::Null) }}"
        ),
        WrittenFieldKind::TextArray => format!(
            "match self.{field} {{ Some(values) => crate::lower::sql::Cell::Array(values.into_iter().map(turso_core::Value::from_text).collect()), None => crate::lower::sql::Cell::Scalar(turso_core::Value::Null) }}"
        ),
        WrittenFieldKind::OpaqueText => format!(
            "match self.{field} {{ Some(value) => crate::lower::sql::Cell::Scalar(turso_core::Value::from_text(value)), None => crate::lower::sql::Cell::Scalar(turso_core::Value::Null) }}"
        ),
    }
}

/// A `Row` struct for a `WRITTEN_TABLES` table: one required field per
/// column, and `into_cells` to turn a fully-built one into the
/// `(ColumnId, Cell)` pairs `rows::insert_row` writes. Guarded by
/// `every_written_table_row_has_every_column` in `rows.rs`'s tests, which
/// checks the generated field count against the table's own column count.
fn written_row_literal(non_system: &[&Json]) -> Result<String, String> {
    let mut fields = Vec::with_capacity(non_system.len());
    let mut exprs = Vec::with_capacity(non_system.len());
    for column in non_system {
        let name = text_field(column, "attname")?.to_string();
        let const_name = name.to_uppercase();
        let not_null = bool_field(column, "attnotnull")?;
        let kind = written_field_kind(oid_field(column, "atttypid")?)?;
        fields.push(format!(
            "        pub(crate) {name}: {},\n",
            written_field_type(&kind, not_null)
        ));
        let expr = written_field_cell_expr(&kind, not_null, &name);
        exprs.push(format!("            ({const_name}, {expr}),\n"));
    }
    let mut out = String::new();
    out.push_str("\n    pub(crate) struct Row {\n");
    for field in fields {
        out.push_str(&field);
    }
    out.push_str("    }\n\n");
    out.push_str("    impl Row {\n");
    out.push_str("        pub(crate) fn into_cells(self) -> Result<Vec<(ColumnId, crate::lower::sql::Cell)>, crate::error::HeadError> {\n");
    out.push_str("            Ok(vec![\n");
    for expr in exprs {
        out.push_str(&expr);
    }
    out.push_str("            ])\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    Ok(out)
}

fn indexes_literal(indexes: &[Json]) -> Result<String, String> {
    let mut entries = Vec::new();
    for index in indexes {
        let oid = oid_field(index, "oid")?;
        let table_oid = oid_field(index, "table_oid")?;
        let unique = bool_field(index, "indisunique")?;
        let mut key_attnums: Vec<i16> = Vec::new();
        for attnum in text_field(index, "indkey")?.split_whitespace() {
            key_attnums.push(
                attnum
                    .parse()
                    .map_err(|error| format!("indkey {attnum:?} is not an attnum: {error}"))?,
            );
        }
        entries.push(format!(
            "Index {{ oid: {oid}, table_oid: {table_oid}, unique: {unique}, key_attnums: {} }}",
            i16_slice_literal(&key_attnums)
        ));
    }
    Ok(format!(
        "pub(super) static INDEXES: &[Index] = &[\n    {}\n];\n",
        entries.join(",\n    ")
    ))
}

fn rows_literal(
    static_name: &str,
    rows_json: &[Json],
    table_oid: u32,
    schema: &[ColumnSchema],
    role_oid: &HashMap<String, u32>,
) -> Result<String, String> {
    let mut entries = Vec::with_capacity(rows_json.len());
    for row in rows_json {
        entries.push(row_literal(row, table_oid, schema, role_oid)?);
    }
    Ok(format!(
        "pub(super) static {static_name}: &[Row] = &[\n    {}\n];\n",
        entries.join(",\n    ")
    ))
}

fn types_module_literal(types_json: &[Json]) -> Result<String, String> {
    let mut entries: Vec<(u32, String)> = Vec::with_capacity(types_json.len());
    for row in types_json {
        entries.push((
            oid_field(row, "oid")?,
            text_field(row, "typname")?.to_string(),
        ));
    }
    entries.sort_by_key(|(oid, _)| *oid);
    let mut out = String::new();
    out.push_str("pub(crate) mod types {\n");
    for (oid, typname) in entries {
        let const_name = type_const_name(&typname);
        out.push_str(&format!(
            "    pub(crate) const {const_name}: i64 = {oid};\n"
        ));
    }
    out.push_str("}\n");
    Ok(out)
}

/// `pg::casts::IMPLICIT`: every `(source oid, target oid)` pair PostgreSQL
/// 18 casts implicitly (`capture/out/casts.json`, `pg_cast.castcontext =
/// 'i'`), the fact `analyze/typing/check.rs`'s common-type merges read
/// instead of a hand-ranked width tier.
fn casts_module_literal(casts: &[Json]) -> Result<String, String> {
    let mut pairs: Vec<(u32, u32)> = Vec::with_capacity(casts.len());
    for row in casts {
        pairs.push((oid_field(row, "source")?, oid_field(row, "target")?));
    }
    pairs.sort_unstable();
    let mut out = String::new();
    out.push_str("pub(crate) mod casts {\n");
    out.push_str("    pub(crate) const IMPLICIT: &[(u32, u32)] = &[\n");
    for (source, target) in pairs {
        out.push_str(&format!("        ({source}, {target}),\n"));
    }
    out.push_str("    ];\n");
    out.push_str("}\n");
    Ok(out)
}

fn views_module_literal(relations: &[Json]) -> Result<String, String> {
    let mut entries: Vec<(u32, String)> = Vec::new();
    for relation in relations {
        if text_field(relation, "relkind")? != "v" {
            continue;
        }
        entries.push((
            oid_field(relation, "oid")?,
            text_field(relation, "relname")?.to_uppercase(),
        ));
    }
    entries.sort_by_key(|(oid, _)| *oid);
    let mut out = String::new();
    out.push_str("pub(crate) mod views {\n");
    for (oid, name) in entries {
        out.push_str(&format!("    pub(crate) const {name}_OID: u32 = {oid};\n"));
    }
    out.push_str("}\n");
    Ok(out)
}

fn type_const_name(typname: &str) -> String {
    match typname.strip_prefix('_') {
        Some(base) => format!("{}_ARRAY_OID", base.to_uppercase()),
        None => format!("{}_OID", typname.to_uppercase()),
    }
}

fn role_oid_map(pg_authid: &[Json]) -> Result<HashMap<String, u32>, String> {
    let mut map = HashMap::with_capacity(pg_authid.len());
    for row in pg_authid {
        map.insert(
            text_field(row, "rolname")?.to_string(),
            oid_field(row, "oid")?,
        );
    }
    Ok(map)
}

fn column_literal(column: &Json) -> Result<String, String> {
    let attnum = int_field(column, "attnum")?;
    let name = text_literal(text_field(column, "attname")?);
    let type_oid = oid_field(column, "atttypid")?;
    let not_null = bool_field(column, "attnotnull")?;
    let len = int_field(column, "attlen")?;
    let align = alignment_variant(text_field(column, "attalign")?)?;
    let storage = storage_variant(text_field(column, "attstorage")?)?;
    let collation = oid_field(column, "attcollation")?;
    let ndims = int_field(column, "attndims")?;
    let is_system = bool_field(column, "is_system_column")?;
    Ok(format!(
        "Column {{ attnum: {attnum}, name: {name}, type_oid: {type_oid}, \
         not_null: {not_null}, len: {len}, align: {align}, storage: {storage}, \
         collation: {collation}, ndims: {ndims}, is_system: {is_system} }}"
    ))
}

fn alignment_variant(letter: &str) -> Result<&'static str, String> {
    match letter {
        "c" => Ok("Alignment::Char"),
        "s" => Ok("Alignment::Short"),
        "i" => Ok("Alignment::Int"),
        "d" => Ok("Alignment::Double"),
        other => Err(format!("unknown pg_attribute.attalign letter: {other:?}")),
    }
}

fn storage_variant(letter: &str) -> Result<&'static str, String> {
    match letter {
        "p" => Ok("Storage::Plain"),
        "x" => Ok("Storage::Extended"),
        other => Err(format!("unknown pg_attribute.attstorage letter: {other:?}")),
    }
}

fn row_literal(
    row: &Json,
    table_oid: u32,
    schema: &[ColumnSchema],
    role_oid: &HashMap<String, u32>,
) -> Result<String, String> {
    let mut entries = Vec::with_capacity(schema.len());
    for column in schema {
        let value = value_literal(column.type_oid, field(row, &column.name)?, role_oid)?;
        let attnum = column.attnum;
        let type_oid = column.type_oid;
        entries.push(format!(
            "(ColumnId {{ table_oid: {table_oid}, attnum: {attnum}, type_oid: {type_oid} }}, {value})"
        ));
    }
    Ok(format!("Row {{ columns: &[{}] }}", entries.join(", ")))
}

fn value_literal(
    type_oid: u32,
    json: &Json,
    role_oid: &HashMap<String, u32>,
) -> Result<String, String> {
    if json.is_null() {
        return Ok("Value::Null".to_string());
    }
    match type_oid {
        16 => Ok(format!(
            "Value::Bool({})",
            json.as_bool()
                .ok_or_else(|| format!("bool field is not a bool: {json}"))?
        )),
        19 => Ok(format!(
            "Value::Name({})",
            text_literal(
                json.as_str()
                    .ok_or_else(|| format!("name field is not a string: {json}"))?
            )
        )),
        21 => Ok(format!(
            "Value::I16({})",
            json.as_i64()
                .ok_or_else(|| format!("int2 field is not a number: {json}"))?
        )),
        23 => Ok(format!(
            "Value::I32({})",
            json.as_i64()
                .ok_or_else(|| format!("int4 field is not a number: {json}"))?
        )),
        // The captured value is a JSON number (serde_json's f64); its digits
        // are embedded with an `f32` suffix so rustc itself parses and
        // rounds the literal to the head's actual `real` (float4) storage
        // type. No numeric cast happens in this generator.
        700 => Ok(format!(
            "Value::F32({}f32)",
            json.as_f64()
                .ok_or_else(|| format!("real field is not a number: {json}"))?
        )),
        18 => {
            let letter = json
                .as_str()
                .ok_or_else(|| format!("\"char\" field is not a string: {json}"))?;
            let byte = match letter.as_bytes() {
                [] => 0u8,
                [only] => *only,
                _ => return Err(format!("\"char\" field {letter:?} is not zero or one byte")),
            };
            Ok(format!("Value::Char({byte})"))
        }
        25 => Ok(format!(
            "Value::Text({})",
            text_literal(
                json.as_str()
                    .ok_or_else(|| format!("text field is not a string: {json}"))?
            )
        )),
        24 | 26 | 28 => {
            let text = json
                .as_str()
                .ok_or_else(|| format!("oid/regproc/xid field is not a string: {json}"))?;
            let oid: u32 = match text {
                "-" => 0,
                text => text.parse().map_err(|_| {
                    format!("oid/regproc/xid field is not a decimal number: {text:?}")
                })?,
            };
            Ok(format!("Value::Oid({oid})"))
        }
        22 => i16_array_literal(json),
        30 => oid_array_literal(json),
        1034 => acl_array_literal(json, role_oid),
        other => Err(format!(
            "no Value mapping for pg_type oid {other} (value {json}); \
             add one only if the capture actually uses it"
        )),
    }
}

fn i16_array_literal(items: &Json) -> Result<String, String> {
    let mut elements = Vec::new();
    for item in rows(items)? {
        let value = item
            .as_i64()
            .ok_or_else(|| format!("int2vector element is not a number: {item}"))?;
        elements.push(value.to_string());
    }
    Ok(format!("Value::I16Array(&[{}])", elements.join(", ")))
}

fn oid_array_literal(items: &Json) -> Result<String, String> {
    let mut elements = Vec::new();
    for item in rows(items)? {
        let text = item
            .as_str()
            .ok_or_else(|| format!("oidvector element is not a string: {item}"))?;
        let oid: u32 = text
            .parse()
            .map_err(|error| format!("oidvector element is not an oid: {error}"))?;
        elements.push(oid.to_string());
    }
    Ok(format!("Value::OidArray(&[{}])", elements.join(", ")))
}

fn acl_array_literal(items: &Json, role_oid: &HashMap<String, u32>) -> Result<String, String> {
    let mut elements = Vec::new();
    for item in rows(items)? {
        let text = item
            .as_str()
            .ok_or_else(|| format!("aclitem array element is not a string: {item}"))?;
        elements.push(acl_item_literal(text, role_oid)?);
    }
    Ok(format!("Value::Acl(&[{}])", elements.join(", ")))
}

fn acl_item_literal(text: &str, role_oid: &HashMap<String, u32>) -> Result<String, String> {
    let (grantee_part, rest) = text
        .split_once('=')
        .ok_or_else(|| format!("aclitem {text:?} has no '='"))?;
    let (privilege_letters, grantor_part) = rest
        .rsplit_once('/')
        .ok_or_else(|| format!("aclitem {text:?} has no '/'"))?;
    let grantee = if grantee_part.is_empty() {
        "Grantee::Public".to_string()
    } else {
        let name = unquote_acl_name(grantee_part);
        let oid = role_oid.get(&name).ok_or_else(|| {
            format!("aclitem grantee {name:?} does not resolve to a captured role")
        })?;
        format!("Grantee::Role({oid})")
    };
    let grantor_name = unquote_acl_name(grantor_part);
    let grantor_oid = role_oid.get(&grantor_name).ok_or_else(|| {
        format!("aclitem grantor {grantor_name:?} does not resolve to a captured role")
    })?;
    let bits = privileges_bits(privilege_letters)?;
    Ok(format!(
        "AclItem {{ grantee: {grantee}, grantor: {grantor_oid}, privileges: Privileges::from_bits({bits}) }}"
    ))
}

const PRIVILEGES_BIT_POSITIONS: &str = "arwdDxtmXUCTc";

fn privileges_bits(letters: &str) -> Result<u16, String> {
    let mut bits: u16 = 0;
    for letter in letters.chars() {
        let index = PRIVILEGES_BIT_POSITIONS
            .chars()
            .position(|candidate| candidate == letter)
            .ok_or_else(|| {
                format!("privilege letter {letter:?} is not one of {PRIVILEGES_BIT_POSITIONS:?}")
            })?;
        bits |= 1 << index;
    }
    Ok(bits)
}

fn unquote_acl_name(raw: &str) -> String {
    match raw
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(inner) => inner.replace("\"\"", "\""),
        None => raw.to_string(),
    }
}

struct ColumnSchema {
    name: String,
    attnum: i64,
    type_oid: u32,
}

fn table_columns(columns: &[Json], reloid: u32) -> Result<Vec<ColumnSchema>, String> {
    let mut matching: Vec<&Json> = Vec::new();
    for column in columns {
        if oid_field(column, "reloid")? == reloid && int_field(column, "attnum")? > 0 {
            matching.push(column);
        }
    }
    matching.sort_by_key(|column| int_field(column, "attnum").unwrap_or(0));
    let mut schema = Vec::with_capacity(matching.len());
    for column in matching {
        schema.push(ColumnSchema {
            name: text_field(column, "attname")?.to_string(),
            attnum: int_field(column, "attnum")?,
            type_oid: oid_field(column, "atttypid")?,
        });
    }
    Ok(schema)
}

struct Relations {
    oid_by_name: HashMap<String, u32>,
}

impl Relations {
    fn load(relations: &[Json]) -> Relations {
        let mut oid_by_name = HashMap::new();
        for relation in relations {
            let Ok(name) = text_field(relation, "relname") else {
                continue;
            };
            let Ok(oid) = oid_field(relation, "oid") else {
                continue;
            };
            oid_by_name.insert(name.to_string(), oid);
        }
        Relations { oid_by_name }
    }

    fn oid(&self, name: &str) -> Result<u32, String> {
        self.oid_by_name
            .get(name)
            .copied()
            .ok_or_else(|| format!("no captured relation named {name:?}"))
    }
}

fn text_literal(text: &str) -> String {
    format!("{text:?}")
}

fn i16_slice_literal(items: &[i16]) -> String {
    let elements: Vec<String> = items.iter().map(|item| item.to_string()).collect();
    format!("&[{}]", elements.join(", "))
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(relative: &str) -> Result<Json, String> {
    let path = manifest_dir().join(relative);
    crate::record_read(&path);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("reading {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("parsing {}: {error}", path.display()))
}

fn rows(json: &Json) -> Result<&Vec<Json>, String> {
    json.as_array()
        .ok_or_else(|| format!("top-level JSON value is not an array: {json}"))
}

fn field<'a>(row: &'a Json, name: &str) -> Result<&'a Json, String> {
    row.get(name)
        .ok_or_else(|| format!("row has no field {name:?}: {row}"))
}

fn text_field<'a>(row: &'a Json, name: &str) -> Result<&'a str, String> {
    field(row, name)?
        .as_str()
        .ok_or_else(|| format!("field {name:?} is not a string: {row}"))
}

fn oid_field(row: &Json, name: &str) -> Result<u32, String> {
    text_field(row, name)?
        .parse()
        .map_err(|error| format!("field {name:?} is not an oid: {error}"))
}

fn bool_field(row: &Json, name: &str) -> Result<bool, String> {
    field(row, name)?
        .as_bool()
        .ok_or_else(|| format!("field {name:?} is not a bool: {row}"))
}

fn int_field(row: &Json, name: &str) -> Result<i64, String> {
    field(row, name)?
        .as_i64()
        .ok_or_else(|| format!("field {name:?} is not a number: {row}"))
}
