use crate::analyze::types::TypeHandle;
use crate::catalog::pg;
use crate::catalog::{Oid, PUBLIC_NAMESPACE};
use crate::engine::constant;
use crate::engine::EngineConnection;
use crate::error::HeadError;

use super::super::rows::{insert_row, ConstraintType, ReferentialAction};

/// Every `pg_index` column for the single-column B-tree backing a
/// `PRIMARY KEY`: never a partial or expression index, never clustered,
/// always immediately visible.
fn index_row(
    index_oid: Oid,
    table_oid: Oid,
    column_type: TypeHandle,
    attnum: i64,
) -> Result<pg::pg_index::Row, HeadError> {
    Ok(pg::pg_index::Row {
        indexrelid: index_oid.as_i64(),
        indrelid: table_oid.as_i64(),
        indnatts: 1,
        indnkeyatts: 1,
        indisunique: true,
        indnullsnotdistinct: false,
        indisprimary: true,
        indisexclusion: false,
        indimmediate: true,
        indisclustered: false,
        indisvalid: true,
        indcheckxmin: false,
        indisready: true,
        indislive: true,
        indisreplident: false,
        indkey: vec![attnum],
        indcollation: vec![column_type.collation()],
        indclass: vec![column_type.btree_opclass().ok_or_else(|| {
            HeadError::internal(format!(
                "no default btree operator class for type oid {}",
                column_type.oid()
            ))
        })?],
        indoption: vec![0],
        indexprs: None,
        indpred: None,
    })
}

pub(super) fn write_index_row(
    connection: &EngineConnection,
    index_oid: Oid,
    table_oid: Oid,
    column_type: TypeHandle,
    attnum: usize,
) -> Result<(), HeadError> {
    let object = constant::ProjectObject::new(index_oid)?;
    let attnum_i64 =
        i64::try_from(attnum).map_err(|_| HeadError::internal("an attnum fits in i64"))?;
    insert_row(
        connection,
        object,
        &pg::pg_index::TABLE,
        index_row(index_oid, table_oid, column_type, attnum_i64)?.into_cells()?,
    )
}

/// Every `pg_constraint` column shared by a primary key and a NOT NULL
/// constraint row: neither is deferrable, a foreign key, or an exclusion
/// constraint, and both are local (never inherited from a parent this
/// head cannot express).
fn constraint_common(oid: Oid, name: String, table_oid: Oid) -> pg::pg_constraint::Row {
    let not_applicable = ReferentialAction::NotApplicable.code();
    pg::pg_constraint::Row {
        oid: oid.as_i64(),
        conname: name,
        connamespace: PUBLIC_NAMESPACE.as_i64(),
        contype: ' ',
        condeferrable: false,
        condeferred: false,
        conenforced: true,
        convalidated: true,
        conrelid: table_oid.as_i64(),
        contypid: 0,
        conindid: 0,
        conparentid: 0,
        confrelid: 0,
        confupdtype: not_applicable,
        confdeltype: not_applicable,
        confmatchtype: not_applicable,
        conislocal: true,
        coninhcount: 0,
        connoinherit: false,
        conperiod: false,
        conkey: None,
        confkey: None,
        conpfeqop: None,
        conppeqop: None,
        conffeqop: None,
        confdelsetcols: None,
        conexclop: None,
        conbin: None,
    }
}

pub(super) fn write_constraint_row(
    connection: &EngineConnection,
    constraint_oid: Oid,
    name: String,
    table_oid: Oid,
    index_oid: Oid,
    attnum: usize,
) -> Result<(), HeadError> {
    let attnum_i64 =
        i64::try_from(attnum).map_err(|_| HeadError::internal("an attnum fits in i64"))?;
    let object = constant::ProjectObject::new(constraint_oid)?;
    let row = pg::pg_constraint::Row {
        contype: ConstraintType::PrimaryKey.code(),
        conindid: index_oid.as_i64(),
        conkey: Some(vec![attnum_i64]),
        connoinherit: true,
        ..constraint_common(constraint_oid, name, table_oid)
    };
    insert_row(
        connection,
        object,
        &pg::pg_constraint::TABLE,
        row.into_cells()?,
    )
}

/// PostgreSQL 18 gives every NOT NULL column (explicit, or implied by
/// PRIMARY KEY) its own `pg_constraint` row: `contype = 'n'`, no backing
/// index (`conindid = 0`), and `connoinherit = false` (unlike a primary
/// key's `true`) since a not-null constraint a child table declares for
/// itself is still inherited. Probed against a live PostgreSQL 18.6 with
/// `SELECT * FROM pg_constraint WHERE contype = 'n'` after the same
/// `CREATE TABLE`.
pub(super) fn write_not_null_constraint_row(
    connection: &EngineConnection,
    constraint_oid: Oid,
    name: String,
    table_oid: Oid,
    attnum: usize,
) -> Result<(), HeadError> {
    let attnum_i64 =
        i64::try_from(attnum).map_err(|_| HeadError::internal("an attnum fits in i64"))?;
    let object = constant::ProjectObject::new(constraint_oid)?;
    let row = pg::pg_constraint::Row {
        contype: ConstraintType::NotNull.code(),
        conkey: Some(vec![attnum_i64]),
        ..constraint_common(constraint_oid, name, table_oid)
    };
    insert_row(
        connection,
        object,
        &pg::pg_constraint::TABLE,
        row.into_cells()?,
    )
}
