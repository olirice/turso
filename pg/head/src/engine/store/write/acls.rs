use turso_core::Value;

use crate::catalog::pg::{self, ColumnId};
use crate::catalog::{AclEntry, Grantee, Oid};
use crate::engine::constant;
use crate::engine::EngineConnection;
use crate::error::HeadError;
use crate::lower::sql::{self, Relation};
use crate::security::privileges::ObjectKind;

use super::super::rows::{
    acl_item_value, code_value, ensure_table, write_project_row, SharedDependType,
};
use super::dependencies::write_shdepend_row;

pub(super) fn replace_acl(
    connection: &EngineConnection,
    kind: ObjectKind,
    object: Oid,
    owner: Oid,
    entries: &[AclEntry],
) -> Result<(), HeadError> {
    let (table, column): (&pg::Table, ColumnId) = match kind {
        ObjectKind::Table => (&pg::pg_class::TABLE, pg::pg_class::RELACL),
        ObjectKind::Schema => (&pg::pg_namespace::TABLE, pg::pg_namespace::NSPACL),
    };
    let oid_column: ColumnId = match kind {
        ObjectKind::Table => pg::pg_class::OID,
        ObjectKind::Schema => pg::pg_namespace::OID,
    };
    let checked = constant::ProjectObject::new(object)?;
    let elements: Result<Vec<Value>, _> =
        entries.iter().map(|entry| acl_item_value(*entry)).collect();
    let elements = elements?;
    let mut params = Vec::new();
    let expr = sql::array(&mut params, elements)?;
    let filter = sql::equals(oid_column, &mut params, Value::from_i64(object.as_i64()))?;
    write_project_row(
        connection,
        checked,
        sql::update(
            Relation::Table(table.relation_oid()),
            vec![(column.into(), expr)],
            filter,
        ),
        params,
    )?;

    let class_oid = pg::pg_class::TABLE.relation_oid().as_i64();
    let namespace_oid = pg::pg_namespace::TABLE.relation_oid().as_i64();
    let authid_oid = pg::pg_authid::TABLE.relation_oid().as_i64();
    let owning_classid = match kind {
        ObjectKind::Table => class_oid,
        ObjectKind::Schema => namespace_oid,
    };
    let shdepend = &pg::pg_shdepend::TABLE;
    ensure_table(connection, shdepend)?;
    let mut delete_params = Vec::new();
    let delete_filter = sql::and_all(vec![
        *sql::equals(
            pg::pg_shdepend::CLASSID,
            &mut delete_params,
            Value::from_i64(owning_classid),
        )?,
        *sql::equals(
            pg::pg_shdepend::OBJID,
            &mut delete_params,
            Value::from_i64(object.as_i64()),
        )?,
        *sql::equals(
            pg::pg_shdepend::DEPTYPE,
            &mut delete_params,
            code_value(SharedDependType::Acl.code()),
        )?,
    ])
    .ok_or_else(|| HeadError::internal("three conditions always join into one"))?;
    write_project_row(
        connection,
        checked,
        sql::delete(Relation::Table(shdepend.relation_oid()), delete_filter),
        delete_params,
    )?;
    for entry in entries {
        let Grantee::Role(role_oid) = entry.grantee else {
            continue;
        };
        if role_oid == owner {
            continue;
        }
        write_shdepend_row(
            connection,
            owning_classid,
            object.as_i64(),
            authid_oid,
            role_oid.as_i64(),
            SharedDependType::Acl,
        )?;
    }
    Ok(())
}
