use std::collections::BTreeSet;

use turso_core::Value;

use crate::analyze::plan::ResolvedQuery;
use crate::analyze::typing::{self, RelationSlot, Scope, Typed};
use crate::analyze::{
    GrantTarget, InsertColumn, OutputSpec, RelationFact, Resolved, SelectPlan, TableRef,
};
use crate::catalog::{Catalog, NewNotNullConstraint, NewPrimaryKey, Oid, Role, Table};
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::{PolicyName, PreparedName, RoleName, TableName};
use crate::parse::statement::{ColumnDef, CommandTag, RowSecurityChange};
use crate::render;
use crate::session::settings::{KnownSetting, SettingValue};
use crate::{catalog, OutputColumn};

pub(crate) enum Kind {
    CreateTable {
        oid: Oid,
        name: TableName,
        columns: Vec<ColumnDef>,
        primary_key: Option<NewPrimaryKey>,
        not_null_constraints: Vec<NewNotNullConstraint>,
        next_oid: Oid,
    },
    Insert {
        table: TableRef,
        columns: Vec<InsertColumn>,
        rows: Vec<Vec<Value>>,
    },
    Select {
        query: Box<ResolvedQuery>,
        output: Vec<OutputSpec>,
        columns: Vec<OutputColumn>,
    },
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
        table: Oid,
        roles: Vec<catalog::Grantee>,
        using: Stored,
        referenced_columns: BTreeSet<catalog::Attnum>,
        next_oid: Oid,
    },
    SetRowSecurity {
        table: Oid,
        enabled: bool,
        forced: bool,
    },
    Lock,
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

pub(crate) struct Checked(Kind);

impl Checked {
    pub(crate) fn into_kind(self) -> Kind {
        self.0
    }
}

enum RowSecurity {
    Unfiltered,
    Filtered,
}

pub(crate) struct Context {
    pub(crate) role: Role,
}

pub(crate) fn check(
    resolved: Resolved,
    context: &Context,
    catalog: &Catalog,
) -> Result<Checked, HeadError> {
    match resolved {
        Resolved::CreateTable {
            oid,
            name,
            columns,
            primary_key,
            not_null_constraints,
            next_oid,
            location: _,
        } => {
            check_table_definition(&name, &columns, catalog)?;
            Ok(Checked(Kind::CreateTable {
                oid,
                name,
                columns,
                primary_key,
                not_null_constraints,
                next_oid,
            }))
        }
        Resolved::Insert {
            table,
            columns,
            rows,
        } => {
            refuse_catalog_write(&table, CommandTag::Insert)?;
            if let RowSecurity::Filtered = row_security(&table, context, catalog)? {
                return Err(HeadError::raise(PgError::NewRowViolatesRowSecurity {
                    table: table.name.clone(),
                }));
            }
            Ok(Checked(Kind::Insert {
                table,
                columns,
                rows,
            }))
        }
        Resolved::Select(plan) => {
            let SelectPlan {
                query,
                columns,
                output,
                ..
            } = *plan;
            Ok(Checked(Kind::Select {
                query: Box::new(query),
                output,
                columns,
            }))
        }
        Resolved::CreateRole {
            oid,
            name,
            can_login,
            next_oid,
        } => {
            if name.as_str().starts_with("pg_") {
                return Err(HeadError::raise(PgError::ReservedRoleName(name)));
            }
            if catalog.role(&name).is_some() {
                return Err(HeadError::raise(PgError::RoleAlreadyExists(name)));
            }
            Ok(Checked(Kind::CreateRole {
                oid,
                name,
                can_login,
                next_oid,
            }))
        }
        Resolved::Grant { targets } => Ok(Checked(Kind::Grant { targets })),
        Resolved::CreatePolicy {
            oid,
            name,
            table,
            roles,
            using,
            next_oid,
        } => {
            refuse_catalog_write(&table, CommandTag::CreatePolicy)?;
            let found = found_table(&table, catalog)?;
            if found.policies.iter().any(|policy| policy.name == name) {
                return Err(HeadError::raise(PgError::PolicyAlreadyExists {
                    policy: name,
                    table: table.name.clone(),
                }));
            }
            let (using, referenced_columns) = type_check_policy_using(using, &table.name, found)?;
            Ok(Checked(Kind::CreatePolicy {
                oid,
                name,
                table: table.oid,
                roles,
                using,
                referenced_columns,
                next_oid,
            }))
        }
        Resolved::Lock { .. } => Ok(Checked(Kind::Lock)),
        Resolved::Begin { read_only } => Ok(Checked(Kind::Begin { read_only })),
        Resolved::Commit => Ok(Checked(Kind::Commit)),
        Resolved::Rollback => Ok(Checked(Kind::Rollback)),
        Resolved::SetTransaction { read_only } => Ok(Checked(Kind::SetTransaction { read_only })),
        Resolved::Set {
            target,
            value,
            local,
        } => Ok(Checked(Kind::Set {
            target,
            value,
            local,
        })),
        Resolved::Prepare { name, statement } => Ok(Checked(Kind::Prepare { name, statement })),
        Resolved::AlterRowSecurity { table, changes } => {
            refuse_catalog_write(&table, CommandTag::AlterTable)?;
            let found = found_table(&table, catalog)?;
            let (enabled, forced) = changes.iter().fold(
                (found.rls_enabled, found.rls_forced),
                |(enabled, forced), change| match change {
                    RowSecurityChange::Enable => (true, forced),
                    RowSecurityChange::Disable => (false, forced),
                    RowSecurityChange::Force => (enabled, true),
                    RowSecurityChange::NoForce => (enabled, false),
                },
            );
            Ok(Checked(Kind::SetRowSecurity {
                table: table.oid,
                enabled,
                forced,
            }))
        }
    }
}

pub(crate) fn refuse_when_row_security_is_off(
    resolved: &Resolved,
    role: Role,
    row_security: bool,
    catalog: &Catalog,
) -> Result<(), HeadError> {
    let table = match resolved {
        Resolved::Select(plan) => return refuse_first_refused_reference(&plan.references),
        Resolved::Insert { table, .. } => table,
        Resolved::CreateTable { .. }
        | Resolved::CreateRole { .. }
        | Resolved::Grant { .. }
        | Resolved::CreatePolicy { .. }
        | Resolved::AlterRowSecurity { .. }
        | Resolved::Lock { .. }
        | Resolved::Begin { .. }
        | Resolved::Commit
        | Resolved::Rollback
        | Resolved::SetTransaction { .. }
        | Resolved::Set { .. }
        | Resolved::Prepare { .. } => return Ok(()),
    };
    let found = found_table(table, catalog)?;
    if row_security || exempt(found, role) {
        return Ok(());
    }
    Err(row_security_off_error(&table.name, role.oid == found.owner))
}

fn refuse_first_refused_reference(references: &[RelationFact]) -> Result<(), HeadError> {
    if let Some(reference) = references.iter().find(|reference| reference.refused) {
        return Err(row_security_off_error(
            &reference.name,
            reference.checked_as == reference.owner,
        ));
    }
    Ok(())
}

/// PostgreSQL 18 adds a hint (probed live) only when the caller it refused
/// is the table's own owner: only the owner can act on the suggestion
/// (`ALTER TABLE NO FORCE ROW LEVEL SECURITY`), so a non-owner caller (who
/// merely holds a grant) gets the bare message.
fn row_security_off_error(name: &TableName, caller_is_owner: bool) -> HeadError {
    HeadError::raise(PgError::RowSecurityPolicyAffectsQuery {
        table: name.clone(),
        caller_is_owner,
    })
}

fn row_security(
    table: &TableRef,
    context: &Context,
    catalog: &Catalog,
) -> Result<RowSecurity, HeadError> {
    let role = context.role;
    let found = found_table(table, catalog)?;
    if exempt(found, role) {
        return Ok(RowSecurity::Unfiltered);
    }
    Ok(RowSecurity::Filtered)
}

fn exempt(table: &Table, role: Role) -> bool {
    !table.rls_enabled || role.superuser || (table.owner == role.oid && !table.rls_forced)
}

/// Type-checks a policy's `USING` expression, whether it came from a live
/// `CREATE POLICY` or (`engine::store::load`) a stored policy's own text
/// re-entering through `parse::stored_expression`: the same type checker,
/// the same `Position::Policy` rules, either way; then `canonicalize`s the
/// result, so a stored policy written by any older build is verified once,
/// the first time it is read back, not just at the moment it is written.
pub(crate) fn type_check_policy_using(
    using: crate::parse::expr::Expr,
    table_name: &TableName,
    table: &Table,
) -> Result<(Stored, BTreeSet<catalog::Attnum>), HeadError> {
    let scope = Scope::single(RelationSlot::SELF, table_name.clone(), table);
    // `Position::Policy`'s own `reports_position: false` rule (see
    // `analyze/typing/context.rs`) is what keeps every error below
    // positionless; nothing here needs to strip one after the fact.
    let cx = typing::Context::new(typing::Position::Policy);
    let using = typing::type_check(using, &scope, &cx)?;
    cx.require_boolean(&using)?;
    let referenced_columns = crate::analyze::referenced_columns(&using);
    let stored = canonicalize(using, table_name, table, &cx)?;
    Ok((stored, referenced_columns))
}

/// A policy's `USING` expression, proven to store as PostgreSQL's own
/// `pg_get_expr` text: rendered once by `render::policy_expression` (the
/// only list of what a policy may store, since it is the one place that
/// must name every form it produces), then parsed and type-checked again
/// through the one entry every stored expression re-enters through
/// (`parse::stored_expression`, the same `Position::Policy` rules), and
/// required to render right back to the identical text: `canonicalize`'s
/// own fixed-point check, not a structural comparison of the typed tree.
/// Private fields: only `canonicalize` can build one, so a catalog write
/// for a policy that skipped this round trip does not compile.
pub(crate) struct Stored {
    text: String,
    typed: Typed,
}

impl Stored {
    pub(crate) fn into_text(self) -> String {
        self.text
    }

    pub(crate) fn into_typed(self) -> Typed {
        self.typed
    }
}

fn canonicalize(
    typed: Typed,
    table_name: &TableName,
    table: &Table,
    cx: &typing::Context,
) -> Result<Stored, HeadError> {
    let text = render::policy_expression(&typed, table)?;
    let scope = Scope::single(RelationSlot::SELF, table_name.clone(), table);
    let reparsed = crate::parse::stored_expression(&text)?;
    let retyped = typing::type_check(reparsed, &scope, cx)?;
    let retext = render::policy_expression(&retyped, table)?;
    // Not a refusal PostgreSQL 18 would show: the renderer producing text
    // whose own reparse retypes and re-renders to something else is an
    // invariant violation in this head, not a user-triggered one.
    if text != retext {
        return Err(HeadError::internal(
            "a policy expression's canonical text is not its own fixed point",
        ));
    }
    Ok(Stored { text, typed })
}

fn found_table<'a>(table: &TableRef, catalog: &'a Catalog) -> Result<&'a Table, HeadError> {
    let found = if table.backing.is_catalog() {
        catalog.catalog_relation(&table.name)
    } else {
        catalog.table(&table.name)
    };
    found
        .filter(|found| found.oid == table.oid)
        .ok_or_else(|| HeadError::internal("a resolved table is missing from the catalog"))
}

fn refuse_catalog_write(table: &TableRef, action: CommandTag) -> Result<(), HeadError> {
    if table.backing.is_catalog() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::CatalogRelationAction {
                action,
                table: table.name.clone(),
            },
        ));
    }
    Ok(())
}

fn check_table_definition(
    name: &TableName,
    columns: &[ColumnDef],
    catalog: &Catalog,
) -> Result<(), HeadError> {
    let primary_key_locations: Vec<crate::parse::Location> = columns
        .iter()
        .filter_map(|column| column.primary_key)
        .collect();
    if let [_, second, ..] = primary_key_locations.as_slice() {
        return Err(HeadError::raise(PgError::MultiplePrimaryKeys(name.clone())).at(*second));
    }
    let mut seen = BTreeSet::new();
    if let Some(duplicate) = columns.iter().find(|column| !seen.insert(&column.name)) {
        return Err(HeadError::raise(PgError::ColumnSpecifiedMoreThanOnce(
            duplicate.name.clone(),
        )));
    }
    if catalog.relation_exists(name.ident()) {
        return Err(HeadError::raise(PgError::RelationAlreadyExists(
            name.clone(),
        )));
    }
    Ok(())
}

pub(crate) fn check_row_not_null(
    table: &TableRef,
    columns: &[InsertColumn],
    row: &[Value],
) -> Result<(), HeadError> {
    if let Some((column, _)) = columns
        .iter()
        .zip(row)
        .find(|(column, value)| column.not_null && matches!(value, Value::Null))
    {
        return Err(HeadError::raise(PgError::NotNullViolation {
            column: column.name.clone(),
            table: table.name.clone(),
            failing_row: failing_row(row),
        }));
    }
    Ok(())
}

/// PostgreSQL 18's own "Failing row contains (...)" rendering
/// (`ExecBuildSlotValueDescription`): values joined by ", ", a NULL as the
/// bare word "null", everything else through the wire's own value-to-text
/// path, unquoted, verbatim, even when it contains a comma, parenthesis,
/// quote or backslash. Checked against PostgreSQL 18 directly (see
/// rows.sql): this message is not `record_out`, and applies none of
/// `record_out`'s quoting.
fn failing_row(row: &[Value]) -> String {
    row.iter()
        .map(|value| crate::render::wire_text(value).unwrap_or_else(|| "null".to_string()))
        .collect::<Vec<_>>()
        .join(", ")
}
