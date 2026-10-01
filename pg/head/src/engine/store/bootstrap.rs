use turso_core::Value;

use crate::catalog::pg;
use crate::catalog::{
    Oid, DATABASE_OID, FIRST_USER_OID, PG_DATABASE_OWNER_ROLE, POSTGRES_ROLE, PUBLIC_NAMESPACE,
};
use crate::engine::constant;
use crate::engine::EngineConnection;
use crate::error::HeadError;
use crate::lower::sql;

use super::rows::{insert_rows, integer, query, row_cells, text_value};
use super::{in_unit_of_work, CATALOG_FORMAT_VERSION, STATE_TABLE};
use crate::lower::sql::Relation;

pub(crate) fn bootstrap(connection: &EngineConnection) -> Result<(), HeadError> {
    in_unit_of_work(connection, true, |connection| {
        let existed = state_table_exists(connection)?;
        create_state_table_schema(connection)?;
        if !existed {
            seed_state_row(connection)?;
        }
        check_catalog_format_version(connection)?;
        seed_authid(connection)?;
        seed_namespace(connection)?;
        seed_database(connection)?;
        seed_init_privs(connection)?;
        seed_description(connection)?;
        Ok(())
    })
}

fn state_table_exists(connection: &EngineConnection) -> Result<bool, HeadError> {
    let mut params = Vec::new();
    let filter = sql::equals(
        sql::Column::EngineSchemaName,
        &mut params,
        text_value(&sql::engine_name(STATE_TABLE)),
    )?;
    let rows = query(
        connection,
        sql::select(
            Relation::EngineSchema,
            &[sql::Column::EngineSchemaName],
            Some(filter),
            &[],
        ),
        params,
    )?;
    Ok(!rows.is_empty())
}

fn create_state_table_schema(connection: &EngineConnection) -> Result<(), HeadError> {
    connection.write(
        sql::create_table(
            STATE_TABLE,
            true,
            vec![
                sql::column(
                    sql::Column::HeadState(sql::HeadStateColumn::Id),
                    pg::EngineStorageType::Integer,
                    sql::ColumnConstraints {
                        primary_key: true,
                        not_null: true,
                        unique: false,
                    },
                ),
                sql::column(
                    sql::Column::HeadState(sql::HeadStateColumn::NextOid),
                    pg::EngineStorageType::Integer,
                    sql::ColumnConstraints {
                        primary_key: false,
                        not_null: true,
                        unique: false,
                    },
                ),
                sql::column(
                    sql::Column::HeadState(sql::HeadStateColumn::CatalogVersion),
                    pg::EngineStorageType::Integer,
                    sql::ColumnConstraints {
                        primary_key: false,
                        not_null: true,
                        unique: false,
                    },
                ),
                sql::column(
                    sql::Column::HeadState(sql::HeadStateColumn::FormatVersion),
                    pg::EngineStorageType::Integer,
                    sql::ColumnConstraints {
                        primary_key: false,
                        not_null: true,
                        unique: false,
                    },
                ),
            ],
            Vec::new(),
        ),
        Vec::new(),
    )
}

fn seed_state_row(connection: &EngineConnection) -> Result<(), HeadError> {
    let (cmd, params) = sql::insert(
        STATE_TABLE,
        true,
        &[
            sql::Column::HeadState(sql::HeadStateColumn::Id),
            sql::Column::HeadState(sql::HeadStateColumn::NextOid),
            sql::Column::HeadState(sql::HeadStateColumn::CatalogVersion),
            sql::Column::HeadState(sql::HeadStateColumn::FormatVersion),
        ],
        vec![vec![
            Value::from_i64(1),
            Value::from_i64(i64::from(FIRST_USER_OID)),
            Value::from_i64(1),
            Value::from_i64(CATALOG_FORMAT_VERSION),
        ]],
    )?;
    connection.write(cmd, params)
}

fn check_catalog_format_version(connection: &EngineConnection) -> Result<(), HeadError> {
    let stored = query(
        connection,
        sql::select(
            STATE_TABLE,
            &[sql::Column::HeadState(sql::HeadStateColumn::FormatVersion)],
            None,
            &[],
        ),
        Vec::new(),
    )
    .ok()
    .and_then(|rows| match rows.as_slice() {
        [row] => match row.as_slice() {
            [version] => integer(version).ok(),
            _ => None,
        },
        _ => None,
    });
    match stored {
        Some(version) if version == CATALOG_FORMAT_VERSION => Ok(()),
        Some(version) => Err(HeadError::internal(format!(
            "this database's catalog format version is {version}, but this build of turso_pg_head only understands format version {CATALOG_FORMAT_VERSION}; refusing to open a catalog of another format"
        ))),
        None => Err(HeadError::internal(format!(
            "this database has no readable catalog format version; refusing to open a catalog of another format (this build of turso_pg_head only understands format version {CATALOG_FORMAT_VERSION})"
        ))),
    }
}

fn seed_authid(connection: &EngineConnection) -> Result<(), HeadError> {
    let table = &pg::pg_authid::TABLE;
    for row in pg::authid_rows().iter() {
        let oid = match row.text(pg::pg_authid::ROLNAME) {
            Ok("postgres") => POSTGRES_ROLE,
            Ok("pg_database_owner") => PG_DATABASE_OWNER_ROLE,
            _ => continue,
        };
        let object = constant::ProjectObject::new(oid)?;
        insert_rows(
            connection,
            object,
            table,
            true,
            vec![row_cells(table, row)?],
        )?;
    }
    Ok(())
}

fn seed_namespace(connection: &EngineConnection) -> Result<(), HeadError> {
    let table = &pg::pg_namespace::TABLE;
    let row = pg::namespace_rows()
        .iter()
        .find(|row| row.text(pg::pg_namespace::NSPNAME) == Ok("public"))
        .ok_or_else(|| HeadError::internal("the public namespace is captured"))?;
    let object = constant::ProjectObject::new(PUBLIC_NAMESPACE)?;
    insert_rows(
        connection,
        object,
        table,
        true,
        vec![row_cells(table, row)?],
    )
}

fn seed_database(connection: &EngineConnection) -> Result<(), HeadError> {
    let table = &pg::pg_database::TABLE;
    let database_oid = DATABASE_OID;
    let row = pg::database_rows()
        .iter()
        .find(|row| row.oid(pg::pg_database::OID) == Ok(database_oid))
        .ok_or_else(|| HeadError::internal("the postgres database is captured"))?;
    let object = constant::ProjectObject::new(Oid::new(DATABASE_OID))?;
    insert_rows(
        connection,
        object,
        table,
        true,
        vec![row_cells(table, row)?],
    )
}

fn seed_init_privs(connection: &EngineConnection) -> Result<(), HeadError> {
    let table = &pg::pg_init_privs::TABLE;
    let public_oid = PUBLIC_NAMESPACE.get();
    let row = pg::init_privs_rows()
        .iter()
        .find(|row| {
            row.oid(pg::pg_init_privs::OBJOID)
                .is_ok_and(|oid| oid == public_oid)
        })
        .ok_or_else(|| HeadError::internal("public's initial privileges are captured"))?;
    let object = constant::ProjectObject::new(PUBLIC_NAMESPACE)?;
    insert_rows(
        connection,
        object,
        table,
        true,
        vec![row_cells(table, row)?],
    )
}

fn seed_description(connection: &EngineConnection) -> Result<(), HeadError> {
    let table = &pg::pg_description::TABLE;
    let public_oid = PUBLIC_NAMESPACE.get();
    let row = pg::description_rows()
        .iter()
        .find(|row| {
            row.oid(pg::pg_description::OBJOID)
                .is_ok_and(|oid| oid == public_oid)
        })
        .ok_or_else(|| HeadError::internal("public's description is captured"))?;
    let object = constant::ProjectObject::new(PUBLIC_NAMESPACE)?;
    insert_rows(
        connection,
        object,
        table,
        true,
        vec![row_cells(table, row)?],
    )
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use turso_parser::ast;

    use super::super::rows::table_exists;
    use super::super::tests::{test_connection, test_database};
    use super::*;

    fn set_stored_format_version(connection: &EngineConnection, version: i64) {
        let mut params = Vec::new();
        let filter = sql::equals(
            sql::Column::HeadState(sql::HeadStateColumn::Id),
            &mut params,
            Value::from_i64(1),
        )
        .expect("building the filter succeeds");
        let expr = Box::new(ast::Expr::Literal(ast::Literal::Numeric(
            version.to_string(),
        )));
        connection
            .write(
                sql::update(
                    STATE_TABLE,
                    vec![(
                        sql::Column::HeadState(sql::HeadStateColumn::FormatVersion),
                        expr,
                    )],
                    filter,
                ),
                params,
            )
            .expect("overwriting the stored format version succeeds");
    }

    #[test]
    fn a_database_whose_stored_format_version_differs_is_refused_on_open() {
        let db = test_database();
        let connection = crate::engine::open(db.connect().expect("connecting succeeds"));
        set_stored_format_version(&connection, CATALOG_FORMAT_VERSION + 1);

        let reopened = crate::engine::open(db.connect().expect("connecting succeeds"));
        let error =
            bootstrap(&reopened).expect_err("a mismatched catalog format version is refused");
        assert!(
            error.to_string().contains("catalog format version"),
            "expected a catalog format version error, got {error}"
        );
    }

    #[test]
    fn a_database_whose_state_table_predates_the_format_version_column_is_refused_on_open() {
        let db = test_database();
        let connection = crate::engine::open(db.connect().expect("connecting succeeds"));
        connection
            .write(sql::drop_table(STATE_TABLE, false), Vec::new())
            .expect("dropping the state table succeeds");
        connection
            .write(
                sql::create_table(
                    STATE_TABLE,
                    false,
                    vec![sql::column(
                        sql::Column::HeadState(sql::HeadStateColumn::Id),
                        pg::EngineStorageType::Integer,
                        sql::ColumnConstraints {
                            primary_key: true,
                            not_null: true,
                            unique: false,
                        },
                    )],
                    Vec::new(),
                ),
                Vec::new(),
            )
            .expect("recreating a state table without a format version column succeeds");
        let (cmd, params) = sql::insert(
            STATE_TABLE,
            true,
            &[sql::Column::HeadState(sql::HeadStateColumn::Id)],
            vec![vec![Value::from_i64(1)]],
        )
        .expect("building the insert succeeds");
        connection
            .write(cmd, params)
            .expect("seeding the state table succeeds");

        let reopened = crate::engine::open(db.connect().expect("connecting succeeds"));
        let error = bootstrap(&reopened).expect_err("a missing catalog format version is refused");
        assert!(
            error.to_string().contains("catalog format version"),
            "expected a catalog format version error, got {error}"
        );
    }

    #[test]
    fn bootstrap_creates_no_table_for_a_constant_only_relation() {
        let connection = test_connection();
        let pg_type = pg::tables()
            .iter()
            .find(|table| table.name == "pg_type")
            .expect("pg_type is captured");
        assert!(!table_exists(&connection, pg_type).expect("checking table existence succeeds"));
    }
}
