use crate::catalog::pg;
use crate::catalog::{Oid, DATABASE_OID};
use crate::engine::constant;
use crate::engine::EngineConnection;
use crate::error::HeadError;

use super::super::rows::{insert_row, DependType, SharedDependType};

pub(super) struct Dependency {
    pub(super) classid: Oid,
    pub(super) objid: Oid,
    pub(super) objsubid: i64,
    pub(super) refclassid: Oid,
    pub(super) refobjid: Oid,
    pub(super) refobjsubid: i64,
    pub(super) deptype: DependType,
}

pub(super) fn write_depend_row(
    connection: &EngineConnection,
    dependency: Dependency,
) -> Result<(), HeadError> {
    let object = constant::ProjectObject::new(dependency.objid)?;
    let row = pg::pg_depend::Row {
        classid: dependency.classid.as_i64(),
        objid: dependency.objid.as_i64(),
        objsubid: dependency.objsubid,
        refclassid: dependency.refclassid.as_i64(),
        refobjid: dependency.refobjid.as_i64(),
        refobjsubid: dependency.refobjsubid,
        deptype: dependency.deptype.code(),
    };
    insert_row(connection, object, &pg::pg_depend::TABLE, row.into_cells()?)
}

pub(super) fn write_shdepend_row(
    connection: &EngineConnection,
    classid: i64,
    objid: i64,
    refclassid: i64,
    refobjid: i64,
    deptype: SharedDependType,
) -> Result<(), HeadError> {
    let object = constant::ProjectObject::new(Oid::from_i64(objid)?)?;
    let row = pg::pg_shdepend::Row {
        dbid: i64::from(DATABASE_OID),
        classid,
        objid,
        objsubid: 0,
        refclassid,
        refobjid,
        deptype: deptype.code(),
    };
    insert_row(
        connection,
        object,
        &pg::pg_shdepend::TABLE,
        row.into_cells()?,
    )
}
