use std::collections::{BTreeMap, BTreeSet};

use turso_core::Value;

use crate::analyze::types::TypeHandle;
use crate::catalog::pg;
use crate::catalog::{
    AclEntry, Attnum, Catalog, Column, Grantee, Namespace, NotNullConstraint, Oid, Policy,
    PrimaryKey, Role, Table, PUBLIC_NAMESPACE,
};
use crate::engine::constant;
use crate::engine::EngineConnection;
use crate::error::HeadError;
use crate::ident::{
    ColumnName, ConstraintName, Ident, PolicyName, RoleName, SchemaName, TableName,
};
use crate::lower::sql;
use crate::security::privileges::ObjectKind;
use crate::security::row_security::PolicyCommand;

use super::rows::{
    code_value, decode_acl_element, integer, malformed, read_array, select_rows,
    stored_catalog_tables, text, ConstraintType, RelKind,
};
use super::{read_state_scalar, PROOF};

pub(crate) fn load(connection: &EngineConnection) -> Result<Catalog, HeadError> {
    let roles = load_roles(connection)?;
    let relations = load_relation_names(connection)?;
    let next_oid = load_next_oid(connection)?;

    let columns = load_columns(connection)?;
    let primary_keys = load_primary_keys(connection)?;
    let not_null_constraints = load_not_null_constraints(connection)?;
    let mut tables = load_tables(connection, columns, primary_keys, not_null_constraints)?;

    // Policies name the table they are on, so tables must exist first.
    let mut policies = load_policies(connection, &tables)?;
    for table in tables.values_mut() {
        table.policies = policies.remove(&table.oid.as_i64()).unwrap_or_default();
    }

    let mut acls = BTreeMap::new();
    load_object_acls(
        connection,
        &pg::pg_class::TABLE,
        pg::pg_class::OID,
        pg::pg_class::RELACL,
        ObjectKind::Table,
        &mut acls,
    )?;
    load_object_acls(
        connection,
        &pg::pg_namespace::TABLE,
        pg::pg_namespace::OID,
        pg::pg_namespace::NSPACL,
        ObjectKind::Schema,
        &mut acls,
    )?;

    let namespaces = load_namespaces(connection)?;

    let catalog_relations = load_catalog_relations(connection, &mut acls)?;
    load_catalog_view_acls(&mut acls)?;

    Ok(Catalog {
        roles,
        relations,
        tables,
        catalog_relations,
        acls,
        namespaces,
        next_oid,
    })
}

// select_rows with no filter and no ordering: every row of `table`.
fn select_all(
    connection: &EngineConnection,
    table: &pg::Table,
    columns: &[sql::Column],
) -> Result<Vec<Vec<Value>>, HeadError> {
    select_rows(connection, table, columns, None, Vec::new(), &[])
}

fn load_roles(connection: &EngineConnection) -> Result<BTreeMap<RoleName, Role>, HeadError> {
    select_all(
        connection,
        &pg::pg_authid::TABLE,
        &[
            pg::pg_authid::OID.into(),
            pg::pg_authid::ROLNAME.into(),
            pg::pg_authid::ROLSUPER.into(),
            pg::pg_authid::ROLCANLOGIN.into(),
        ],
    )?
    .iter()
    .map(|row| match row.as_slice() {
        [oid, name, superuser, can_login] => Ok((
            RoleName::from_catalog(PROOF, text(name)?),
            Role {
                oid: Oid::from_i64(integer(oid)?)?,
                superuser: integer(superuser)? != 0,
                can_login: integer(can_login)? != 0,
            },
        )),
        _ => Err(malformed("pg_authid")),
    })
    .collect()
}

fn load_relation_names(connection: &EngineConnection) -> Result<BTreeSet<Ident>, HeadError> {
    let mut params = Vec::new();
    let filter = sql::equals(
        pg::pg_class::RELNAMESPACE,
        &mut params,
        Value::from_i64(PUBLIC_NAMESPACE.as_i64()),
    )?;
    select_rows(
        connection,
        &pg::pg_class::TABLE,
        &[pg::pg_class::RELNAME.into()],
        Some(filter),
        params,
        &[],
    )?
    .iter()
    .map(|row| match row.as_slice() {
        [name] => Ok(Ident::from_catalog(PROOF, text(name)?)),
        _ => Err(malformed("pg_class")),
    })
    .collect()
}

fn load_next_oid(connection: &EngineConnection) -> Result<Oid, HeadError> {
    read_state_scalar(connection, sql::HeadStateColumn::NextOid, |value| {
        Oid::from_i64(integer(value)?)
    })
}

fn load_columns(connection: &EngineConnection) -> Result<BTreeMap<i64, Vec<Column>>, HeadError> {
    let mut columns = BTreeMap::<i64, Vec<Column>>::new();
    for row in select_rows(
        connection,
        &pg::pg_attribute::TABLE,
        &[
            pg::pg_attribute::ATTRELID.into(),
            pg::pg_attribute::ATTNAME.into(),
            pg::pg_attribute::ATTTYPID.into(),
            pg::pg_attribute::ATTNOTNULL.into(),
        ],
        None,
        Vec::new(),
        &[
            pg::pg_attribute::ATTRELID.into(),
            pg::pg_attribute::ATTNUM.into(),
        ],
    )? {
        let [table, name, type_oid, not_null] = row.as_slice() else {
            return Err(malformed("pg_attribute"));
        };
        let type_oid = integer(type_oid)?;
        columns.entry(integer(table)?).or_default().push(Column {
            name: ColumnName::from_catalog(PROOF, text(name)?),
            ty: TypeHandle::by_oid(type_oid)?,
            not_null: integer(not_null)? != 0,
        });
    }
    Ok(columns)
}

fn load_primary_keys(
    connection: &EngineConnection,
) -> Result<BTreeMap<i64, PrimaryKey>, HeadError> {
    let mut params = Vec::new();
    let filter = sql::equals(
        pg::pg_constraint::CONTYPE,
        &mut params,
        code_value(ConstraintType::PrimaryKey.code()),
    )?;
    let mut primary_keys = BTreeMap::new();
    for row in select_rows(
        connection,
        &pg::pg_constraint::TABLE,
        &[
            pg::pg_constraint::OID.into(),
            pg::pg_constraint::CONRELID.into(),
            pg::pg_constraint::CONNAME.into(),
            pg::pg_constraint::CONINDID.into(),
        ],
        Some(filter),
        params,
        &[],
    )? {
        let [oid, table, name, index] = row.as_slice() else {
            return Err(malformed("pg_constraint"));
        };
        let constraint_oid = integer(oid)?;
        let elements = read_array(
            connection,
            &pg::pg_constraint::TABLE,
            pg::pg_constraint::CONKEY,
            &[(pg::pg_constraint::OID, Value::from_i64(constraint_oid))],
        )?
        .ok_or_else(|| malformed("pg_constraint"))?;
        let [attnum] = elements.as_slice() else {
            return Err(malformed("pg_constraint"));
        };
        let attnum = usize::try_from(integer(attnum)?).map_err(|_| malformed("pg_constraint"))?;
        primary_keys.insert(
            integer(table)?,
            PrimaryKey {
                name: ConstraintName::from_catalog(PROOF, text(name)?),
                index_oid: Oid::from_i64(integer(index)?)?,
                constraint_oid: Oid::from_i64(constraint_oid)?,
                attnum: Attnum::new(attnum),
            },
        );
    }
    Ok(primary_keys)
}

fn load_not_null_constraints(
    connection: &EngineConnection,
) -> Result<BTreeMap<i64, Vec<NotNullConstraint>>, HeadError> {
    let mut params = Vec::new();
    let filter = sql::equals(
        pg::pg_constraint::CONTYPE,
        &mut params,
        code_value(ConstraintType::NotNull.code()),
    )?;
    let mut not_null_constraints: BTreeMap<i64, Vec<NotNullConstraint>> = BTreeMap::new();
    for row in select_rows(
        connection,
        &pg::pg_constraint::TABLE,
        &[
            pg::pg_constraint::OID.into(),
            pg::pg_constraint::CONRELID.into(),
        ],
        Some(filter),
        params,
        &[],
    )? {
        let [oid, table] = row.as_slice() else {
            return Err(malformed("pg_constraint"));
        };
        let constraint_oid = integer(oid)?;
        let elements = read_array(
            connection,
            &pg::pg_constraint::TABLE,
            pg::pg_constraint::CONKEY,
            &[(pg::pg_constraint::OID, Value::from_i64(constraint_oid))],
        )?
        .ok_or_else(|| malformed("pg_constraint"))?;
        let [attnum] = elements.as_slice() else {
            return Err(malformed("pg_constraint"));
        };
        let attnum = usize::try_from(integer(attnum)?).map_err(|_| malformed("pg_constraint"))?;
        not_null_constraints
            .entry(integer(table)?)
            .or_default()
            .push(NotNullConstraint {
                constraint_oid: Oid::from_i64(constraint_oid)?,
                attnum: Attnum::new(attnum),
            });
    }
    Ok(not_null_constraints)
}

fn load_tables(
    connection: &EngineConnection,
    mut columns: BTreeMap<i64, Vec<Column>>,
    mut primary_keys: BTreeMap<i64, PrimaryKey>,
    mut not_null_constraints: BTreeMap<i64, Vec<NotNullConstraint>>,
) -> Result<BTreeMap<TableName, Table>, HeadError> {
    let mut params = Vec::new();
    let filter = sql::and_all(vec![
        *sql::equals(
            pg::pg_class::RELKIND,
            &mut params,
            code_value(RelKind::Relation.code()),
        )?,
        *sql::equals(
            pg::pg_class::RELNAMESPACE,
            &mut params,
            Value::from_i64(PUBLIC_NAMESPACE.as_i64()),
        )?,
    ]);
    select_rows(
        connection,
        &pg::pg_class::TABLE,
        &[
            pg::pg_class::OID.into(),
            pg::pg_class::RELNAME.into(),
            pg::pg_class::RELOWNER.into(),
            pg::pg_class::RELROWSECURITY.into(),
            pg::pg_class::RELFORCEROWSECURITY.into(),
        ],
        filter,
        params,
        &[],
    )?
    .iter()
    .map(|row| match row.as_slice() {
        [oid, name, owner, enabled, forced] => {
            let oid = integer(oid)?;
            Ok((
                TableName::from_catalog(PROOF, text(name)?),
                Table {
                    oid: Oid::from_i64(oid)?,
                    owner: Oid::from_i64(integer(owner)?)?,
                    columns: columns.remove(&oid).unwrap_or_default(),
                    primary_key: primary_keys.remove(&oid),
                    not_null_constraints: not_null_constraints.remove(&oid).unwrap_or_default(),
                    rls_enabled: integer(enabled)? != 0,
                    rls_forced: integer(forced)? != 0,
                    policies: Vec::new(),
                    backing: crate::catalog::Backing::User,
                },
            ))
        }
        _ => Err(malformed("pg_class")),
    })
    .collect()
}

fn load_policies(
    connection: &EngineConnection,
    tables: &BTreeMap<TableName, Table>,
) -> Result<BTreeMap<i64, Vec<Policy>>, HeadError> {
    let mut policies = BTreeMap::<i64, Vec<Policy>>::new();
    for row in select_rows(
        connection,
        &pg::pg_policy::TABLE,
        &[
            pg::pg_policy::OID.into(),
            pg::pg_policy::POLRELID.into(),
            pg::pg_policy::POLNAME.into(),
            pg::pg_policy::POLCMD.into(),
            pg::pg_policy::POLQUAL.into(),
        ],
        None,
        Vec::new(),
        &[
            pg::pg_policy::POLRELID.into(),
            pg::pg_policy::POLNAME.into(),
        ],
    )? {
        let [oid, table, name, polcmd, using_tree] = row.as_slice() else {
            return Err(malformed("pg_policy"));
        };
        let policy_oid = integer(oid)?;
        let roles = read_array(
            connection,
            &pg::pg_policy::TABLE,
            pg::pg_policy::POLROLES,
            &[(pg::pg_policy::OID, Value::from_i64(policy_oid))],
        )?
        .ok_or_else(|| malformed("pg_policy"))?
        .iter()
        .map(|role| {
            let oid = integer(role)?;
            Ok(if oid == 0 {
                Grantee::Public
            } else {
                Grantee::Role(Oid::from_i64(oid)?)
            })
        })
        .collect::<Result<Vec<_>, HeadError>>()?;
        let table_oid = integer(table)?;
        let (table_name, found_table) = tables
            .iter()
            .find(|(_, table)| table.oid.as_i64() == table_oid)
            .ok_or_else(|| malformed("pg_policy"))?;
        let command = text(polcmd)?
            .chars()
            .next()
            .ok_or_else(|| malformed("pg_policy"))
            .and_then(PolicyCommand::from_code)?;
        let stored = text(using_tree)?;
        let using = crate::parse::stored_expression(&stored)
            .and_then(|expr| {
                crate::security::enforcement::type_check_policy_using(expr, table_name, found_table)
            })
            .map(|(stored, _)| stored.into_predicate())
            .map_err(|error| {
                HeadError::internal(format!(
                    "a stored policy expression is corrupt: {}",
                    error.message
                ))
            })?;
        policies.entry(table_oid).or_default().push(Policy {
            name: PolicyName::from_catalog(PROOF, text(name)?),
            roles,
            command,
            using,
        });
    }
    Ok(policies)
}

// pg_class and pg_namespace read their ACLs the same way.
fn load_object_acls(
    connection: &EngineConnection,
    table: &pg::Table,
    oid_column: pg::ColumnId,
    acl_column: pg::ColumnId,
    kind: ObjectKind,
    acls: &mut BTreeMap<(ObjectKind, Oid), Vec<AclEntry>>,
) -> Result<(), HeadError> {
    let oids = select_all(connection, table, &[oid_column.into()])?
        .iter()
        .map(|row| match row.as_slice() {
            [oid] => integer(oid),
            _ => Err(malformed(table.name)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    for oid in oids {
        if let Some(elements) = read_array(
            connection,
            table,
            acl_column,
            &[(oid_column, Value::from_i64(oid))],
        )? {
            let entries = elements
                .iter()
                .map(decode_acl_element)
                .collect::<Result<Vec<_>, _>>()?;
            acls.insert((kind, Oid::from_i64(oid)?), entries);
        }
    }
    Ok(())
}

fn load_namespaces(
    connection: &EngineConnection,
) -> Result<BTreeMap<SchemaName, Namespace>, HeadError> {
    let (pg_catalog_name, pg_catalog_namespace) = constant::namespace()?;
    let mut namespaces = BTreeMap::from([(
        SchemaName::from_catalog(PROOF, pg_catalog_name),
        pg_catalog_namespace,
    )]);
    namespaces.extend(
        select_all(
            connection,
            &pg::pg_namespace::TABLE,
            &[
                pg::pg_namespace::OID.into(),
                pg::pg_namespace::NSPNAME.into(),
                pg::pg_namespace::NSPOWNER.into(),
            ],
        )?
        .iter()
        .map(|row| match row.as_slice() {
            [oid, name, owner] => Ok((
                SchemaName::from_catalog(PROOF, text(name)?),
                Namespace {
                    oid: Oid::from_i64(integer(oid)?)?,
                    owner: Oid::from_i64(integer(owner)?)?,
                },
            )),
            _ => Err(malformed("pg_namespace")),
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?,
    );
    Ok(namespaces)
}

fn load_catalog_view_acls(
    acls: &mut BTreeMap<(ObjectKind, Oid), Vec<AclEntry>>,
) -> Result<(), HeadError> {
    for oid in crate::analyze::views::oids() {
        let descriptor = catalog_relation_descriptor(oid)?;
        let entries: Vec<AclEntry> = descriptor
            .acl(pg::pg_class::RELACL)?
            .iter()
            .map(|item| AclEntry::from(*item))
            .collect();
        if !entries.is_empty() {
            acls.insert((ObjectKind::Table, Oid::new(oid)), entries);
        }
    }
    Ok(())
}

fn catalog_relation_descriptor(oid: u32) -> Result<&'static pg::Row, HeadError> {
    pg::class_rows()
        .iter()
        .find(|row| row.oid(pg::pg_class::OID) == Ok(oid))
        .ok_or_else(|| HeadError::internal("every pg_catalog table has its own pg_class row"))
}

fn load_catalog_relations(
    connection: &EngineConnection,
    acls: &mut BTreeMap<(ObjectKind, Oid), Vec<AclEntry>>,
) -> Result<BTreeMap<TableName, Table>, HeadError> {
    let stored = stored_catalog_tables(connection)?;
    let mut relations = BTreeMap::new();
    for table in pg::tables() {
        let descriptor = catalog_relation_descriptor(table.oid)?;
        let owner = Oid::new(descriptor.oid(pg::pg_class::RELOWNER)?);
        let entries: Vec<AclEntry> = descriptor
            .acl(pg::pg_class::RELACL)?
            .iter()
            .map(|item| AclEntry::from(*item))
            .collect();
        if !entries.is_empty() {
            acls.insert((ObjectKind::Table, table.relation_oid()), entries);
        }
        let constant = constant::ConstantRelation::ALL
            .iter()
            .any(|relation| relation.table().oid == table.oid);
        let project = stored.contains(&table.oid);
        let columns = table
            .columns
            .iter()
            .filter(|column| !column.is_system)
            .map(|column| {
                Ok(Column {
                    name: ColumnName::from_catalog(PROOF, column.name),
                    ty: TypeHandle::by_oid(i64::from(column.type_oid))?,
                    not_null: column.not_null,
                })
            })
            .collect::<Result<Vec<_>, HeadError>>()?;
        relations.insert(
            TableName::literal(table.name),
            Table {
                oid: table.relation_oid(),
                owner,
                columns,
                primary_key: None,
                not_null_constraints: Vec::new(),
                rls_enabled: false,
                rls_forced: false,
                policies: Vec::new(),
                backing: crate::catalog::Backing::Catalog { constant, project },
            },
        );
    }
    Ok(relations)
}
