mod bootstrap;
mod cache;
mod load;
pub(in crate::engine) mod rows;
pub(in crate::engine) mod write;

pub(crate) use bootstrap::bootstrap;
pub(crate) use cache::CatalogCache;
pub(crate) use load::load;
pub(crate) use rows::declared_columns;
pub(crate) use write::apply;

use std::sync::Arc;

use turso_core::Value;
use turso_parser::ast;

use crate::catalog::Catalog;
use crate::engine::EngineConnection;
use crate::error::HeadError;
use crate::lower::sql::{self, Relation};

use rows::{integer, malformed, query};

const STATE_TABLE: Relation = Relation::HeadState;

pub(crate) const CATALOG_FORMAT_VERSION: i64 = 2;

pub(crate) struct FromCatalog(());

const PROOF: FromCatalog = FromCatalog(());

pub(crate) fn begin(connection: &EngineConnection) -> Result<(), HeadError> {
    connection.begin_deferred()
}

pub(crate) fn commit(connection: &EngineConnection) -> Result<(), HeadError> {
    connection.commit()
}

pub(crate) fn rollback(connection: &EngineConnection) -> Result<(), HeadError> {
    if connection.is_auto_commit() {
        return Ok(());
    }
    connection.rollback()
}

pub(crate) fn in_unit_of_work<T>(
    connection: &EngineConnection,
    can_write: bool,
    work: impl FnOnce(&EngineConnection) -> Result<T, HeadError>,
) -> Result<T, HeadError> {
    if can_write {
        connection.begin_immediate()?;
    } else {
        connection.begin_deferred()?;
    }
    let outcome = work(connection).and_then(|value| {
        connection.commit()?;
        Ok(value)
    });
    match outcome {
        Err(error) if !connection.is_auto_commit() => match connection.rollback() {
            Ok(()) => Err(error),
            Err(rollback) => Err(HeadError::internal(format!(
                "{error}; the rollback also failed: {rollback}"
            ))),
        },
        outcome => outcome,
    }
}

pub(crate) fn bump_catalog_version(connection: &EngineConnection) -> Result<(), HeadError> {
    let mut params = Vec::new();
    let filter = sql::equals(
        sql::Column::HeadState(sql::HeadStateColumn::Id),
        &mut params,
        Value::from_i64(1),
    )?;
    let expr = Box::new(ast::Expr::Binary(
        sql::column_ref(sql::Column::HeadState(sql::HeadStateColumn::CatalogVersion)),
        ast::Operator::Add,
        Box::new(ast::Expr::Literal(ast::Literal::Numeric("1".to_string()))),
    ));
    connection.write(
        sql::update(
            STATE_TABLE,
            vec![(
                sql::Column::HeadState(sql::HeadStateColumn::CatalogVersion),
                expr,
            )],
            filter,
        ),
        params,
    )
}

pub(crate) fn catalog(
    connection: &EngineConnection,
    cache: &CatalogCache,
    publish: bool,
) -> Result<Arc<Catalog>, HeadError> {
    let version = catalog_version(connection)?;
    if let Some(cached) = cache.lookup(version)? {
        return Ok(cached);
    }
    let loaded = Arc::new(load(connection)?);
    if publish {
        cache.publish(version, Arc::clone(&loaded))?;
    }
    Ok(loaded)
}

fn catalog_version(connection: &EngineConnection) -> Result<i64, HeadError> {
    read_state_scalar(connection, sql::HeadStateColumn::CatalogVersion, integer)
}

fn read_state_scalar<T>(
    connection: &EngineConnection,
    column: sql::HeadStateColumn,
    decode: impl FnOnce(&Value) -> Result<T, HeadError>,
) -> Result<T, HeadError> {
    match query(
        connection,
        sql::select(STATE_TABLE, &[sql::Column::HeadState(column)], None, &[]),
        Vec::new(),
    )?
    .as_slice()
    {
        [row] => match row.as_slice() {
            [value] => decode(value),
            _ => Err(malformed("head_internal_state")),
        },
        _ => Err(malformed("head_internal_state")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use std::sync::Arc;

    use turso_core::{Database, MemoryIO, OpenOptions};

    use super::rows::{integer, query, select_rows, text};
    use super::*;
    use crate::analyze::types::{self, TypeHandle};
    use crate::catalog::pg::{self, ColumnId};
    use crate::catalog::{CatalogWrite, Oid};
    use crate::engine_hooks::EngineHooks;
    use crate::ident::{ColumnName, RoleName, TableName};
    use crate::lower::sql::{self, Relation};

    pub(super) fn test_database() -> Arc<Database> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = format!(
            "store-test-{}.db",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let db = Database::open(
            Arc::new(MemoryIO::new()),
            &path,
            OpenOptions::new(Arc::new(EngineHooks)),
        )
        .expect("opening an in-memory database succeeds");
        let connection = crate::engine::open(db.connect().expect("connecting succeeds"));
        bootstrap(&connection).expect("bootstrap succeeds");
        db
    }

    pub(super) fn test_connection() -> EngineConnection {
        let db = test_database();
        crate::engine::open(db.connect().expect("connecting succeeds"))
    }

    pub(super) fn constant_rows_matching(
        connection: &EngineConnection,
        relation: Oid,
        columns: &[sql::Column],
        filter_column: ColumnId,
        filter_value: Value,
    ) -> Vec<Vec<Value>> {
        let mut params = Vec::new();
        let filter = sql::equals(filter_column, &mut params, filter_value)
            .expect("the query against the constant table succeeds");
        query(
            connection,
            sql::select(Relation::Constant(relation), columns, Some(filter), &[]),
            params,
        )
        .expect("the query against the constant table succeeds")
    }

    pub(super) fn int4_handle() -> TypeHandle {
        TypeHandle::by_oid(types::INT4_OID).expect("int4 is a captured pg_type row")
    }

    pub(super) fn int8_handle() -> TypeHandle {
        TypeHandle::by_oid(types::INT8_OID).expect("int8 is a captured pg_type row")
    }

    pub(super) fn text_handle() -> TypeHandle {
        TypeHandle::by_oid(types::TEXT_OID).expect("text is a captured pg_type row")
    }

    pub(super) fn notes_columns() -> Vec<crate::parse::statement::ColumnDef> {
        vec![
            crate::parse::statement::ColumnDef {
                name: ColumnName::from_catalog(PROOF, "id"),
                ty: int4_handle(),
                primary_key: Some(crate::parse::Location::for_test(0)),
                not_null: true,
            },
            crate::parse::statement::ColumnDef {
                name: ColumnName::from_catalog(PROOF, "body"),
                ty: text_handle(),
                primary_key: None,
                not_null: true,
            },
            crate::parse::statement::ColumnDef {
                name: ColumnName::from_catalog(PROOF, "n"),
                ty: int8_handle(),
                primary_key: None,
                not_null: false,
            },
        ]
    }

    pub(super) fn one_row(
        connection: &EngineConnection,
        table: &pg::Table,
        columns: &[sql::Column],
    ) -> Vec<Value> {
        let rows = select_rows(connection, table, columns, None, Vec::new(), &[])
            .expect("the query against the catalog table succeeds");
        match rows.as_slice() {
            [row] => row.clone(),
            other => panic!("expected exactly one row, found {}", other.len()),
        }
    }

    pub(super) fn rows_matching(
        connection: &EngineConnection,
        table: &pg::Table,
        columns: &[sql::Column],
        filter_column: ColumnId,
        filter_value: Value,
    ) -> Vec<Vec<Value>> {
        let mut params = Vec::new();
        let filter = sql::equals(filter_column, &mut params, filter_value)
            .expect("the query against the catalog table succeeds");
        select_rows(connection, table, columns, Some(filter), params, &[])
            .expect("the query against the catalog table succeeds")
    }

    #[test]
    fn load_reports_pg_catalog_and_public_and_nothing_else() {
        let connection = test_connection();
        let catalog = load(&connection).expect("loading the catalog succeeds");
        let names: Vec<&str> = catalog
            .namespaces_by_oid()
            .into_iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, vec!["pg_catalog", "public"]);
    }

    #[test]
    fn load_reports_no_predefined_role() {
        let connection = test_connection();
        let catalog = load(&connection).expect("loading the catalog succeeds");
        let names: Vec<&str> = catalog
            .roles_by_oid()
            .into_iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, vec!["postgres", "pg_database_owner"]);
        assert!(catalog
            .role(&RoleName::from_catalog(PROOF, "pg_monitor"))
            .is_none());
        assert!(catalog
            .role(&RoleName::from_catalog(PROOF, "pg_read_all_data"))
            .is_none());
    }

    #[test]
    fn constant_pg_class_serves_its_own_row() {
        let connection = test_connection();
        let rows = constant_rows_matching(
            &connection,
            pg::pg_class::TABLE.relation_oid(),
            &[
                pg::pg_class::RELNAME.into(),
                pg::pg_class::RELNATTS.into(),
                pg::pg_class::RELKIND.into(),
            ],
            pg::pg_class::OID,
            Value::from_i64(1259),
        );
        let row = match rows.as_slice() {
            [row] => row.clone(),
            other => panic!("expected exactly one pg_class row, found {}", other.len()),
        };
        assert_eq!(text(&row[0]).unwrap(), "pg_class");
        assert_eq!(integer(&row[1]).unwrap(), 34);
        assert_eq!(text(&row[2]).unwrap(), "r");
    }

    #[test]
    fn constant_pg_type_has_the_int4_row() {
        let connection = test_connection();
        let rows = constant_rows_matching(
            &connection,
            pg::pg_type::TABLE.relation_oid(),
            &[pg::pg_type::TYPNAME.into(), pg::pg_type::TYPLEN.into()],
            pg::pg_type::OID,
            Value::from_i64(23),
        );
        let row = match rows.as_slice() {
            [row] => row.clone(),
            other => panic!("expected exactly one pg_type row, found {}", other.len()),
        };
        assert_eq!(text(&row[0]).unwrap(), "int4");
        assert_eq!(integer(&row[1]).unwrap(), 4);
    }

    #[test]
    fn a_fresh_project_file_has_no_pg_class_row_of_its_own() {
        let connection = test_connection();
        let rows = rows_matching(
            &connection,
            &pg::pg_class::TABLE,
            &[pg::pg_class::RELNAME.into()],
            pg::pg_class::OID,
            Value::from_i64(1259),
        );
        assert!(
            rows.is_empty(),
            "pg_class's own row is served from the binary, not written to the project"
        );
    }

    #[test]
    fn a_schema_reload_still_sees_the_constant_tables() {
        let db = test_database();
        let first = crate::engine::open(db.connect().expect("connecting succeeds"));
        apply(
            &first,
            CatalogWrite::CreateTable {
                oid: Oid::new(16386),
                name: TableName::from_catalog(PROOF, "notes"),
                owner: Oid::new(10),
                columns: notes_columns(),
                primary_key: None,
                not_null_constraints: Vec::new(),
            },
        )
        .expect("creating the table succeeds");

        let second = crate::engine::open(db.connect().expect("connecting succeeds"));
        let rows = constant_rows_matching(
            &second,
            pg::pg_class::TABLE.relation_oid(),
            &[pg::pg_class::RELNAME.into()],
            pg::pg_class::OID,
            Value::from_i64(1259),
        );
        assert_eq!(rows.len(), 1);
    }

    fn create_table(connection: &EngineConnection, oid: u32, name: &str) {
        apply(
            connection,
            CatalogWrite::CreateTable {
                oid: Oid::new(oid),
                name: TableName::from_catalog(PROOF, name),
                owner: Oid::new(10),
                columns: notes_columns(),
                primary_key: None,
                not_null_constraints: Vec::new(),
            },
        )
        .expect("creating the table succeeds");
    }

    #[test]
    fn a_second_read_at_the_same_version_reuses_the_cached_catalog() {
        let connection = test_connection();
        let cache = CatalogCache::new();
        let first = catalog(&connection, &cache, true).expect("the first read loads the catalog");
        let second =
            catalog(&connection, &cache, true).expect("the second read reuses the cached catalog");
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn a_catalog_write_bumps_the_version_and_the_next_read_reloads() {
        let connection = test_connection();
        let cache = CatalogCache::new();
        let first = catalog(&connection, &cache, true).expect("the first read loads the catalog");

        create_table(&connection, 16386, "notes");
        bump_catalog_version(&connection).expect("bumping the version succeeds");

        let reloaded = catalog(&connection, &cache, true).expect("the second read reloads");
        assert!(!Arc::ptr_eq(&first, &reloaded));
        assert!(reloaded
            .tables
            .contains_key(&TableName::from_catalog(PROOF, "notes")));
    }

    #[test]
    fn a_rolled_back_write_is_never_published_and_a_later_write_to_the_same_version_is_not_poisoned(
    ) {
        let db = test_database();
        let connection = crate::engine::open(db.connect().expect("connecting succeeds"));
        let cache = CatalogCache::new();
        catalog(&connection, &cache, true).expect("the warm-up read loads the catalog");

        connection
            .begin_immediate()
            .expect("beginning an immediate transaction succeeds");
        create_table(&connection, 16386, "poisoned");
        bump_catalog_version(&connection).expect("bumping the version succeeds");
        let mid_transaction = catalog(&connection, &cache, false)
            .expect("reading inside the open transaction succeeds");
        assert!(mid_transaction
            .tables
            .contains_key(&TableName::from_catalog(PROOF, "poisoned")));
        connection.rollback().expect("rolling back succeeds");

        create_table(&connection, 16387, "genuine");
        bump_catalog_version(&connection).expect("bumping the version succeeds");

        let after =
            catalog(&connection, &cache, true).expect("reading after the real write succeeds");
        assert!(after
            .tables
            .contains_key(&TableName::from_catalog(PROOF, "genuine")));
        assert!(!after
            .tables
            .contains_key(&TableName::from_catalog(PROOF, "poisoned")));
    }

    fn read_state_table(connection: &EngineConnection) -> Result<Vec<Vec<Value>>, HeadError> {
        query(
            connection,
            sql::select(
                STATE_TABLE,
                &[sql::Column::HeadState(sql::HeadStateColumn::CatalogVersion)],
                None,
                &[],
            ),
            Vec::new(),
        )
    }

    #[test]
    fn a_long_lived_read_does_not_block_a_concurrent_read() {
        let db = test_database();
        let connection_a = crate::engine::open(db.connect().expect("connecting succeeds"));
        let connection_b = crate::engine::open(db.connect().expect("connecting succeeds"));

        in_unit_of_work(&connection_a, false, |connection_a| {
            read_state_table(connection_a).expect("the long-lived read succeeds");
            in_unit_of_work(&connection_b, false, read_state_table)
                .expect("a concurrent read is not blocked by the still-open read above");
            Ok(())
        })
        .expect("the long-lived read commits");
    }

    #[test]
    fn a_concurrent_writer_is_refused_while_another_writer_is_open() {
        let db = test_database();
        let connection_a = crate::engine::open(db.connect().expect("connecting succeeds"));
        let connection_b = crate::engine::open(db.connect().expect("connecting succeeds"));

        in_unit_of_work(&connection_a, true, |connection_a| {
            read_state_table(connection_a).expect("the open writer can still read");
            let refused = in_unit_of_work(&connection_b, true, read_state_table);
            let error = refused.expect_err("a second writer cannot open while the first is open");
            assert!(
                error.to_string().to_lowercase().contains("busy"),
                "expected a busy error, got {error}"
            );
            Ok(())
        })
        .expect("the first writer commits once it is the only one open");
    }
}
