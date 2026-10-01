//! ```compile_fail,E0603
//! use turso_pg_head::pipeline::Lowered;
//! ```

use crate::analyze::{InsertColumn, OutputSpec, TableRef};
use crate::catalog::{CatalogWrite, Role};
use crate::engine::Command;
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::RoleName;
use crate::lower;
use crate::parse::statement::{CommandTag, Statement};
use crate::security::enforcement::{Checked, Kind};
use crate::security::{authorization, enforcement};
use crate::session::session_functions;
use crate::session::{Context, SessionEffect, Settings};
use crate::{analyze, OutputColumn};

pub use crate::catalog::Catalog;

/// The current transaction's state, as `admit` needs it: whether a
/// statement may write (`Open { read_only: false, .. }`), whether a
/// snapshot has already been taken (bearing on `SET TRANSACTION`), or
/// whether the transaction has already failed. A thinner view than
/// `session::Transaction`, which additionally carries the settings to
/// restore on rollback; `Transaction::state` is the one place that maps
/// one to the other.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TransactionState {
    Idle,
    Open { read_only: bool, snapshot: bool },
    Failed,
}

/// The pipeline's entry token: only [`admit`] can build one, so a statement
/// that never passes through the gate cannot reach [`analyze`].
pub(crate) struct Admitted {
    statement: Statement,
}

/// The one statement-admission gate (`ARCH.md`'s `SQL text -> admit ->
/// Admitted -> analyze -> ...`): whether the current transaction state
/// allows this statement to run at all, before analysis, authorization or
/// enforcement ever sees it. Everything this refuses is a transaction-state
/// rule from `Statement::rules()`, or (`SET TRANSACTION`'s own isolation
/// level and read-write mode) how this one statement's own clauses read
/// against a snapshot already taken; nothing here is per-input special
/// casing.
pub(crate) fn admit(statement: Statement, state: &TransactionState) -> Result<Admitted, HeadError> {
    let rules = statement.rules();
    if let TransactionState::Failed = state {
        return if rules.runs_when_aborted {
            Ok(Admitted { statement })
        } else {
            Err(HeadError::raise(PgError::TransactionAborted))
        };
    }
    if let TransactionState::Open {
        read_only: true, ..
    } = state
    {
        if rules.refused_when_read_only {
            return Err(HeadError::raise(PgError::ReadOnlyTransaction {
                command: rules.tag,
            }));
        }
    }
    if let TransactionState::Open {
        read_only,
        snapshot: true,
    } = state
    {
        if let Statement::SetTransaction {
            sets_isolation,
            read_only: requested,
        } = &statement
        {
            if *sets_isolation {
                return Err(HeadError::raise(
                    PgError::ActiveSqlTransactionIsolationLevel,
                ));
            }
            if *read_only && *requested == Some(false) {
                return Err(HeadError::raise(PgError::ActiveSqlTransactionReadWriteMode));
            }
        }
    }
    Ok(Admitted { statement })
}

pub(crate) struct Analyzed {
    resolved: analyze::Resolved,
    role: Role,
    identity: RoleName,
    row_security: bool,
}

pub(crate) struct Authorized {
    analyzed: Analyzed,
}

pub(crate) struct Enforced {
    checked: Checked,
    role: Role,
    identity: RoleName,
}

pub(crate) struct Lowered {
    command: Engine,
    catalog_writes: Vec<CatalogWrite>,
    result: ResultShape,
    effects: Vec<SessionEffect>,
}

pub(crate) enum Engine {
    None,
    One(Box<Command>),
    InsertRows(InsertRows),
}

pub(crate) struct InsertRows {
    pub(crate) table: TableRef,
    pub(crate) columns: Vec<InsertColumn>,
    pub(crate) rows: Vec<Vec<turso_core::Value>>,
}

pub(crate) enum ResultShape {
    Command(CommandTag),
    /// `INSERT`'s own row-count form (`INSERT 0 n`): the one command this
    /// head produces whose tag carries data beyond its name.
    Insert(usize),
    Rows {
        columns: Vec<OutputColumn>,
        output: Vec<analyze::OutputSpec>,
        settings: Settings,
    },
}

pub(crate) fn analyze(
    admitted: Admitted,
    catalog: &Catalog,
    context: &Context,
) -> Result<Analyzed, HeadError> {
    let role = *catalog
        .role(context.identity)
        .ok_or_else(|| HeadError::internal("the session role is not in the catalog"))?;
    let lookup = analyze::Lookup {
        catalog,
        role,
        search_path_includes_public: context.settings.search_path_includes_public(catalog),
        in_transaction: context.in_transaction,
        row_security: context.settings.row_security,
        prepared: context.prepared,
    };
    Ok(Analyzed {
        resolved: analyze::resolve(admitted.statement, &lookup)?,
        role,
        identity: context.identity.clone(),
        row_security: context.settings.row_security,
    })
}

pub(crate) fn authorize(analyzed: Analyzed, catalog: &Catalog) -> Result<Authorized, HeadError> {
    enforcement::refuse_when_row_security_is_off(
        &analyzed.resolved,
        analyzed.role,
        analyzed.row_security,
        catalog,
    )?;
    authorization::check(&analyzed.resolved, analyzed.role, catalog)?;
    Ok(Authorized { analyzed })
}

pub(crate) fn enforce(authorized: Authorized, catalog: &Catalog) -> Result<Enforced, HeadError> {
    let Analyzed {
        resolved,
        role,
        identity,
        ..
    } = authorized.analyzed;
    let context = enforcement::Context { role };
    Ok(Enforced {
        checked: enforcement::check(resolved, &context, catalog)?,
        role,
        identity,
    })
}

pub(crate) fn lower(
    enforced: Enforced,
    catalog: &Catalog,
    context: &Context,
) -> Result<Lowered, HeadError> {
    let role = enforced.role;
    let identity = enforced.identity;
    Ok(match enforced.checked.into_kind() {
        Kind::CreateTable {
            oid,
            name,
            columns,
            primary_key,
            not_null_constraints,
            next_oid,
        } => Lowered {
            result: ResultShape::Command(CommandTag::CreateTable),
            command: Engine::One(Box::new(lower::create_table(oid, &columns)?)),
            catalog_writes: vec![
                CatalogWrite::CreateTable {
                    oid,
                    name,
                    owner: role.oid,
                    columns,
                    primary_key,
                    not_null_constraints,
                },
                CatalogWrite::SetNextOid(next_oid),
            ],
            effects: Vec::new(),
        },
        Kind::Insert {
            table,
            columns,
            rows,
        } => Lowered {
            result: ResultShape::Insert(rows.len()),
            command: Engine::InsertRows(InsertRows {
                table,
                columns,
                rows,
            }),
            catalog_writes: Vec::new(),
            effects: Vec::new(),
        },
        Kind::Select {
            query,
            output,
            columns,
        } => {
            if let Some(ty) = output.iter().find_map(|spec| match spec {
                OutputSpec::Unsupported(ty) => Some(*ty),
                OutputSpec::Column
                | OutputSpec::Bool
                | OutputSpec::Float4
                | OutputSpec::RegClass
                | OutputSpec::RegProc
                | OutputSpec::AclArray
                | OutputSpec::Vector
                | OutputSpec::CatalogRendered(_)
                | OutputSpec::SessionCall(_) => None,
            }) {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::OutputValueType(ty),
                ));
            }
            let mut scratch = context.settings.clone();
            let mut effects = Vec::new();
            let query = session_functions::fold_query(*query, &mut scratch, &mut effects)?;
            let command = lower::select_query(&query, &output, catalog, &identity)?;
            Lowered {
                result: ResultShape::Rows {
                    columns,
                    output,
                    settings: scratch,
                },
                command: Engine::One(Box::new(command)),
                catalog_writes: Vec::new(),
                effects,
            }
        }
        Kind::Lock => Lowered {
            result: ResultShape::Command(CommandTag::Lock),
            command: Engine::None,
            catalog_writes: Vec::new(),
            effects: Vec::new(),
        },
        Kind::CreateRole {
            oid,
            name,
            can_login,
            next_oid,
        } => Lowered {
            result: ResultShape::Command(CommandTag::CreateRole),
            command: Engine::None,
            catalog_writes: vec![
                CatalogWrite::CreateRole {
                    oid,
                    name,
                    can_login,
                },
                CatalogWrite::SetNextOid(next_oid),
            ],
            effects: Vec::new(),
        },
        Kind::Grant { targets } => Lowered {
            result: ResultShape::Command(CommandTag::Grant),
            command: Engine::None,
            catalog_writes: targets
                .into_iter()
                .map(|target| CatalogWrite::ReplaceAcl {
                    kind: target.kind,
                    object: target.oid,
                    owner: target.owner,
                    entries: target.new_acl,
                })
                .collect(),
            effects: Vec::new(),
        },
        Kind::CreatePolicy {
            oid,
            name,
            table,
            roles,
            using,
            referenced_columns,
            next_oid,
        } => Lowered {
            result: ResultShape::Command(CommandTag::CreatePolicy),
            command: Engine::None,
            catalog_writes: vec![
                CatalogWrite::CreatePolicy {
                    oid,
                    name,
                    table,
                    roles,
                    using: using.into_text(),
                    referenced_columns,
                },
                CatalogWrite::SetNextOid(next_oid),
            ],
            effects: Vec::new(),
        },
        Kind::SetRowSecurity {
            table,
            enabled,
            forced,
        } => Lowered {
            result: ResultShape::Command(CommandTag::AlterTable),
            command: Engine::None,
            catalog_writes: vec![CatalogWrite::SetRowSecurity {
                table,
                enabled,
                forced,
            }],
            effects: Vec::new(),
        },
        Kind::Begin { read_only } => Lowered {
            result: ResultShape::Command(CommandTag::Begin),
            command: Engine::None,
            catalog_writes: Vec::new(),
            effects: vec![SessionEffect::Begin { read_only }],
        },
        Kind::Commit => Lowered {
            result: ResultShape::Command(CommandTag::Commit),
            command: Engine::None,
            catalog_writes: Vec::new(),
            effects: vec![SessionEffect::Commit],
        },
        Kind::Rollback => Lowered {
            result: ResultShape::Command(CommandTag::Rollback),
            command: Engine::None,
            catalog_writes: Vec::new(),
            effects: vec![SessionEffect::Rollback],
        },
        Kind::SetTransaction { read_only } => Lowered {
            result: ResultShape::Command(CommandTag::Set),
            command: Engine::None,
            catalog_writes: Vec::new(),
            effects: vec![SessionEffect::SetTransaction { read_only }],
        },
        Kind::Set {
            target,
            value,
            local,
        } => Lowered {
            result: ResultShape::Command(CommandTag::Set),
            command: Engine::None,
            catalog_writes: Vec::new(),
            effects: vec![SessionEffect::Set {
                target,
                value,
                local,
            }],
        },
        Kind::Prepare { name, statement } => Lowered {
            result: ResultShape::Command(CommandTag::Prepare),
            command: Engine::None,
            catalog_writes: Vec::new(),
            effects: vec![SessionEffect::Prepare { name, statement }],
        },
    })
}

impl Lowered {
    pub(crate) fn into_parts(self) -> (Engine, Vec<CatalogWrite>, ResultShape, Vec<SessionEffect>) {
        (self.command, self.catalog_writes, self.result, self.effects)
    }
}
