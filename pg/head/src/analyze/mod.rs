use std::collections::{BTreeMap, BTreeSet};

use turso_core::Value;

use crate::analyze::typing::Typed;
use crate::catalog::{Attnum, Catalog, Grantee, NewNotNullConstraint, NewPrimaryKey, Oid};
use crate::error::{HeadError, PgError};
use crate::ident::{PolicyName, PreparedName, RoleName, TableName};
use crate::parse::expr::Expr;
use crate::parse::statement::{ColumnDef, GranteeName, RowSecurityChange, SetStatement, Statement};
use crate::security::row_security::RowSecurityDecision;
use crate::session::settings::{KnownSetting, SettingValue};

mod ddl;
pub(crate) mod functions;
pub(crate) mod pg_options_to_table;
pub(crate) mod plan;
mod prepared;
mod select;
pub(crate) mod types;
pub(crate) mod typing;
pub(crate) mod views;
mod walk;

pub(crate) use ddl::{GrantTarget, InsertColumn};
pub(crate) use select::{OutputSpec, SelectPlan};
pub(crate) use walk::{RelationFact, TableRef};

pub(crate) enum Resolved {
    CreateTable {
        oid: Oid,
        name: TableName,
        columns: Vec<ColumnDef>,
        primary_key: Option<NewPrimaryKey>,
        not_null_constraints: Vec<NewNotNullConstraint>,
        next_oid: Oid,
        /// The relation name's own location: PostgreSQL 18 points a
        /// "permission denied for schema" error at the table name, not the
        /// statement start. Always present: `CREATE TABLE` reaches this
        /// only through a real, parsed `RangeVar`.
        location: crate::parse::Location,
    },
    Insert {
        table: TableRef,
        reference: RelationFact,
        security: RowSecurityDecision,
        columns: Vec<InsertColumn>,
        rows: Vec<Vec<Value>>,
    },
    Select(Box<SelectPlan>),
    CreateRole {
        oid: Oid,
        name: RoleName,
        can_login: bool,
        next_oid: Oid,
    },
    Grant {
        targets: Vec<GrantTarget>,
    },
    CreatePolicy {
        oid: Oid,
        name: PolicyName,
        table: TableRef,
        roles: Vec<Grantee>,
        command: crate::security::row_security::PolicyCommand,
        using: Expr,
        next_oid: Oid,
    },
    AlterRowSecurity {
        table: TableRef,
        changes: Vec<RowSecurityChange>,
    },
    Lock {
        tables: Vec<TableRef>,
    },
    Begin {
        read_only: Option<bool>,
    },
    Commit,
    Rollback,
    SetTransaction {
        read_only: Option<bool>,
    },
    Set {
        target: KnownSetting,
        value: SettingValue,
        local: bool,
    },
    Prepare {
        name: PreparedName,
        statement: Box<crate::session::PreparedStatement>,
    },
}

pub(crate) fn referenced_columns(typed: &Typed) -> BTreeSet<Attnum> {
    let mut columns = BTreeSet::new();
    typing::referenced_columns(typed, &mut columns);
    columns
}

pub(crate) struct Lookup<'a> {
    pub(crate) catalog: &'a Catalog,
    pub(crate) role: crate::catalog::Role,
    pub(crate) search_path_includes_public: bool,
    pub(crate) in_transaction: bool,
    pub(crate) row_security: bool,
    pub(crate) prepared: &'a BTreeMap<PreparedName, crate::session::PreparedStatement>,
}

pub(crate) fn resolve(statement: Statement, lookup: &Lookup) -> Result<Resolved, HeadError> {
    let catalog = lookup.catalog;
    match statement {
        Statement::CreateTable { relation, columns } => {
            ddl::create_table(relation, columns, lookup)
        }
        Statement::Insert {
            table,
            columns,
            rows,
        } => ddl::insert(&table, columns, rows, lookup),
        Statement::Select(query) => {
            select::select(query, lookup).map(|plan| Resolved::Select(Box::new(plan)))
        }
        Statement::CreateRole { name, can_login } => {
            let mut oids = catalog.oids();
            Ok(Resolved::CreateRole {
                oid: oids.allocate()?,
                name,
                can_login,
                next_oid: oids.next(),
            })
        }
        Statement::Grant {
            privileges,
            objects,
            grantees,
        } => ddl::grant(privileges, objects, &grantees, lookup),
        Statement::CreatePolicy {
            name,
            table: relation,
            roles,
            command,
            using,
        } => {
            // PostgreSQL 18 resolves a policy's TO role list before it looks
            // up the table (probed live: a nonexistent role in TO reports
            // "role ... does not exist" even when the named relation is
            // also absent, and even when the caller is not the table's
            // owner).
            let roles = roles
                .iter()
                .map(|role| match role {
                    GranteeName::Public => Ok(Grantee::Public),
                    GranteeName::Role(role_name) => catalog
                        .role(role_name)
                        .map(|found| Grantee::Role(found.oid))
                        .ok_or_else(|| {
                            HeadError::raise(PgError::RoleDoesNotExist(role_name.clone()))
                        }),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut oids = catalog.oids();
            Ok(Resolved::CreatePolicy {
                table: walk::table_ref(&relation, walk::table(&relation, lookup)?),
                oid: oids.allocate()?,
                next_oid: oids.next(),
                name,
                roles,
                command,
                using,
            })
        }
        Statement::AlterRowSecurity {
            table: relation,
            changes,
        } => Ok(Resolved::AlterRowSecurity {
            table: walk::table_ref(&relation, walk::table(&relation, lookup)?),
            changes,
        }),
        Statement::Lock { .. } if !lookup.in_transaction => {
            Err(HeadError::raise(PgError::LockTableOutsideTransaction))
        }
        Statement::Lock { tables } => Ok(Resolved::Lock {
            tables: tables
                .iter()
                .map(|relation| Ok(walk::table_ref(relation, walk::table(relation, lookup)?)))
                .collect::<Result<Vec<_>, HeadError>>()?,
        }),
        Statement::Begin { read_only } => Ok(Resolved::Begin { read_only }),
        Statement::Commit => Ok(Resolved::Commit),
        Statement::Rollback => Ok(Resolved::Rollback),
        Statement::SetTransaction { read_only, .. } => Ok(Resolved::SetTransaction { read_only }),
        Statement::Set(SetStatement {
            target,
            value,
            local,
        }) => Ok(Resolved::Set {
            target,
            value,
            local,
        }),
        Statement::Prepare {
            name,
            param_types,
            query,
        } => prepared::prepare_statement(name, param_types, query, lookup),
        Statement::Execute { name, args } => prepared::execute_statement(name, args, lookup)
            .and_then(|query| {
                select::select(query, lookup).map(|plan| Resolved::Select(Box::new(plan)))
            }),
    }
}
