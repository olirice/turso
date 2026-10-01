use turso_core::Value;

use crate::catalog::pg;
use crate::catalog::{NewNotNullConstraint, NewPrimaryKey, Oid, FIRST_USER_OID, PUBLIC_NAMESPACE};
use crate::engine::constant;
use crate::engine::EngineConnection;
use crate::error::HeadError;
use crate::ident::TableName;
use crate::lower::sql::{self, Relation};
use crate::parse::statement::ColumnDef;

use super::super::rows::{
    insert_row, write_project_row, DependType, RelKind, ReplicaIdentity, SharedDependType,
};
use super::dependencies::{write_depend_row, write_shdepend_row, Dependency};
use super::indexes::{write_constraint_row, write_index_row, write_not_null_constraint_row};

const HEAP_TABLE_AM_OID: Oid = Oid::new(2);
const BTREE_AM_OID: Oid = Oid::new(403);

pub(super) fn create_table(
    connection: &EngineConnection,
    oid: Oid,
    name: &TableName,
    owner: Oid,
    columns: &[ColumnDef],
    primary_key: Option<NewPrimaryKey>,
    not_null_constraints: &[NewNotNullConstraint],
) -> Result<(), HeadError> {
    let class_oid = pg::pg_class::TABLE.relation_oid();
    let namespace_oid = pg::pg_namespace::TABLE.relation_oid();
    let authid_oid = pg::pg_authid::TABLE.relation_oid();
    let constraint_oid_relid = pg::pg_constraint::TABLE.relation_oid();

    let num_columns = i64::try_from(columns.len())
        .map_err(|_| HeadError::internal("a table has few enough columns to fit in i64"))?;

    write_class_row(
        connection,
        NewClass {
            oid,
            name: name.as_str().to_string(),
            kind: RelKind::Relation,
            owner,
            access_method: HEAP_TABLE_AM_OID,
            relnatts: num_columns,
            relhasindex: primary_key.is_some(),
            relreplident: ReplicaIdentity::Default,
        },
    )?;
    for (attnum, column) in (1i64..).zip(columns) {
        write_attribute_row(connection, oid, attnum, column)?;
    }
    write_depend_row(
        connection,
        Dependency {
            classid: class_oid,
            objid: oid,
            objsubid: 0,
            refclassid: namespace_oid,
            refobjid: PUBLIC_NAMESPACE,
            refobjsubid: 0,
            deptype: DependType::Normal,
        },
    )?;
    if owner.get() >= FIRST_USER_OID {
        write_shdepend_row(
            connection,
            class_oid.as_i64(),
            oid.as_i64(),
            authid_oid.as_i64(),
            owner.as_i64(),
            SharedDependType::Owner,
        )?;
    }
    for not_null in not_null_constraints {
        write_not_null_constraint_row(
            connection,
            not_null.constraint_oid,
            not_null.name.as_str().to_string(),
            oid,
            not_null.attnum,
        )?;
        let attnum_i64 = i64::try_from(not_null.attnum)
            .map_err(|_| HeadError::internal("an attnum fits in i64"))?;
        write_depend_row(
            connection,
            Dependency {
                classid: constraint_oid_relid,
                objid: not_null.constraint_oid,
                objsubid: 0,
                refclassid: class_oid,
                refobjid: oid,
                refobjsubid: attnum_i64,
                deptype: DependType::Auto,
            },
        )?;
    }
    let Some(key) = primary_key else {
        return Ok(());
    };
    let column = columns
        .get(key.attnum - 1)
        .ok_or_else(|| HeadError::internal("a primary key attnum names a declared column"))?;
    write_class_row(
        connection,
        NewClass {
            oid: key.index_oid,
            name: key.name.as_str().to_string(),
            kind: RelKind::Index,
            owner,
            access_method: BTREE_AM_OID,
            relnatts: 1,
            relhasindex: false,
            relreplident: ReplicaIdentity::Nothing,
        },
    )?;
    write_index_row(connection, key.index_oid, oid, column.ty, key.attnum)?;
    write_constraint_row(
        connection,
        key.constraint_oid,
        key.name.as_str().to_string(),
        oid,
        key.index_oid,
        key.attnum,
    )?;
    let attnum_i64 =
        i64::try_from(key.attnum).map_err(|_| HeadError::internal("an attnum fits in i64"))?;
    write_depend_row(
        connection,
        Dependency {
            classid: constraint_oid_relid,
            objid: key.constraint_oid,
            objsubid: 0,
            refclassid: class_oid,
            refobjid: oid,
            refobjsubid: attnum_i64,
            deptype: DependType::Auto,
        },
    )?;
    write_depend_row(
        connection,
        Dependency {
            classid: class_oid,
            objid: key.index_oid,
            objsubid: 0,
            refclassid: constraint_oid_relid,
            refobjid: key.constraint_oid,
            refobjsubid: 0,
            deptype: DependType::Internal,
        },
    )
}

struct NewClass {
    oid: Oid,
    name: String,
    kind: RelKind,
    owner: Oid,
    access_method: Oid,
    relnatts: i64,
    relhasindex: bool,
    relreplident: ReplicaIdentity,
}

/// Every `pg_class` column for a row this head creates (`CREATE TABLE`'s
/// relation itself, or the single-column index backing its primary key):
/// one struct literal naming every column, so a new `pg_class` column
/// forces a decision here instead of silently reaching the engine as NULL.
/// `relam`/`reltablespace` and the rest of what every row the head writes
/// agrees on (never a real TOAST table, never partitioned, no rules or
/// triggers this head can express) live only in this literal.
fn class_row(fields: NewClass) -> pg::pg_class::Row {
    let is_index = matches!(fields.kind, RelKind::Index);
    let storage = super::super::rows::class_storage_facts(fields.oid, is_index);
    pg::pg_class::Row {
        oid: fields.oid.as_i64(),
        relname: fields.name,
        relnamespace: PUBLIC_NAMESPACE.as_i64(),
        reltype: storage.reltype,
        reloftype: 0,
        relowner: fields.owner.as_i64(),
        relam: fields.access_method.as_i64(),
        relfilenode: storage.relfilenode,
        reltablespace: 0,
        relpages: storage.relpages,
        reltuples: storage.reltuples,
        relallvisible: storage.relallvisible,
        relallfrozen: storage.relallfrozen,
        reltoastrelid: storage.reltoastrelid,
        relhasindex: fields.relhasindex,
        relisshared: false,
        relpersistence: 'p',
        relkind: fields.kind.code(),
        relnatts: fields.relnatts,
        relchecks: 0,
        relhasrules: false,
        relhastriggers: false,
        relhassubclass: false,
        relrowsecurity: false,
        relforcerowsecurity: false,
        relispopulated: true,
        relreplident: fields.relreplident.code(),
        relispartition: false,
        relrewrite: 0,
        relfrozenxid: storage.relfrozenxid,
        relminmxid: storage.relminmxid,
        relacl: None,
        reloptions: None,
        relpartbound: None,
    }
}

fn write_class_row(connection: &EngineConnection, fields: NewClass) -> Result<(), HeadError> {
    let object = constant::ProjectObject::new(fields.oid)?;
    insert_row(
        connection,
        object,
        &pg::pg_class::TABLE,
        class_row(fields).into_cells()?,
    )
}

/// Every `pg_attribute` column for one declared column of a `CREATE
/// TABLE`: no generated identity, no stored default, no dropped or
/// inherited history, all of which this head cannot express yet.
fn attribute_row(table_oid: Oid, attnum: i64, column: &ColumnDef) -> pg::pg_attribute::Row {
    pg::pg_attribute::Row {
        attrelid: table_oid.as_i64(),
        attname: column.name.as_str().to_string(),
        atttypid: column.ty.oid(),
        attlen: column.ty.len(),
        attnum,
        atttypmod: -1,
        attndims: 0,
        attbyval: column.ty.by_val(),
        attalign: column.ty.align().code(),
        attstorage: column.ty.storage().code(),
        attcompression: '\0',
        attnotnull: column.primary_key.is_some() || column.not_null,
        atthasdef: false,
        atthasmissing: false,
        attidentity: '\0',
        attgenerated: '\0',
        attisdropped: false,
        attislocal: true,
        attinhcount: 0,
        attcollation: column.ty.collation(),
        attstattarget: None,
        attacl: None,
        attoptions: None,
        attfdwoptions: None,
        attmissingval: None,
    }
}

fn write_attribute_row(
    connection: &EngineConnection,
    table_oid: Oid,
    attnum: i64,
    column: &ColumnDef,
) -> Result<(), HeadError> {
    let object = constant::ProjectObject::new(table_oid)?;
    insert_row(
        connection,
        object,
        &pg::pg_attribute::TABLE,
        attribute_row(table_oid, attnum, column).into_cells()?,
    )
}

pub(super) fn set_row_security(
    connection: &EngineConnection,
    table: Oid,
    enabled: bool,
    forced: bool,
) -> Result<(), HeadError> {
    let object = constant::ProjectObject::new(table)?;
    let mut params = Vec::new();
    let sets = vec![
        (
            pg::pg_class::RELROWSECURITY.into(),
            sql::bind(&mut params, Value::from_i64(i64::from(enabled)))?,
        ),
        (
            pg::pg_class::RELFORCEROWSECURITY.into(),
            sql::bind(&mut params, Value::from_i64(i64::from(forced)))?,
        ),
    ];
    let filter = sql::equals(
        pg::pg_class::OID,
        &mut params,
        Value::from_i64(table.as_i64()),
    )?;
    write_project_row(
        connection,
        object,
        sql::update(
            Relation::Table(pg::pg_class::TABLE.relation_oid()),
            sets,
            filter,
        ),
        params,
    )
}
