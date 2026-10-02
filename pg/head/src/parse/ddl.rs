use pg_query::protobuf::{
    node::Node as PgNode, AccessPriv, AlterTableStmt, AlterTableType, ColumnDef as PgColumnDef,
    ConstrType, Constraint as PgConstraint, CreatePolicyStmt, CreateRoleStmt, CreateStmt, DefElem,
    DefElemAction, GrantStmt, GrantTargetType, Node, ObjectType, OnCommitAction, RoleSpec,
    RoleSpecType, RoleStmtType,
};

use super::{node, table_name, PROOF};
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::{ColumnName, PolicyName, RoleName, SchemaName};
use crate::parse::expr;
use crate::parse::statement::{
    ColumnDef, GrantObjects, GrantedPrivileges, GranteeName, RowSecurityChange, Statement,
};
use crate::parse::Location;
use crate::security::privileges::PrivilegeKeyword;
use crate::security::row_security::PolicyCommand;

fn create_table_clause(clause: &'static str) -> HeadError {
    HeadError::not_supported(NotSupportedFeature::CreateTableClause(clause))
}

pub(super) fn create_table(create: &CreateStmt) -> Result<Statement, HeadError> {
    let CreateStmt {
        relation,
        table_elts,
        inh_relations,
        partbound,
        partspec,
        of_typename,
        constraints,
        options,
        oncommit,
        tablespacename,
        access_method,
        if_not_exists,
    } = create;
    if !inh_relations.is_empty() {
        return Err(create_table_clause("INHERITS"));
    }
    if partspec.is_some() || partbound.is_some() {
        return Err(create_table_clause("partitioning"));
    }
    if of_typename.is_some() {
        return Err(create_table_clause("OF type"));
    }
    if !constraints.is_empty() {
        return Err(create_table_clause("table constraints"));
    }
    if !options.is_empty() {
        return Err(create_table_clause("WITH options"));
    }
    if !matches!(
        OnCommitAction::try_from(*oncommit),
        Ok(OnCommitAction::OncommitNoop)
    ) {
        return Err(create_table_clause("ON COMMIT"));
    }
    if !tablespacename.is_empty() {
        return Err(create_table_clause("TABLESPACE"));
    }
    if !access_method.is_empty() {
        return Err(create_table_clause("USING"));
    }
    if *if_not_exists {
        return Err(create_table_clause("IF NOT EXISTS"));
    }
    let relation = relation
        .as_ref()
        .ok_or_else(|| HeadError::internal("CREATE TABLE without a relation"))?;
    if relation.relpersistence != "p" {
        return Err(HeadError::not_supported(
            NotSupportedFeature::TemporaryOrUnloggedTables,
        ));
    }
    let relation = table_name(relation)?;
    let columns = table_elts
        .iter()
        .map(|element| {
            let PgNode::ColumnDef(column) = node(Some(element))? else {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::CreateTableConstraints,
                ));
            };
            column_def(column)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if columns.is_empty() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::TableWithNoColumns,
        ));
    }
    Ok(Statement::CreateTable { relation, columns })
}

fn column_clause(clause: &'static str) -> HeadError {
    HeadError::not_supported(NotSupportedFeature::ColumnClause(clause))
}

fn column_def(column: &PgColumnDef) -> Result<ColumnDef, HeadError> {
    let PgColumnDef {
        colname,
        type_name,
        compression,
        // Inheritance bookkeeping (how many parents propagated this
        // column, and whether it originates in this table): always `0`
        // and `true` for a column this head ever sees, since a raw
        // `ColumnDef` from a real `CREATE TABLE` statement text is never
        // itself an inherited copy.
        inhcount: _,
        is_local: _,
        // PostgreSQL's raw parser never sets this for a real `ColumnDef`:
        // `NOT NULL` always arrives as its own `Constraint` node (matched
        // below), never as this flag.
        is_not_null: _,
        // Set only when a column is expanded from `LIKE` or `%TYPE`,
        // neither of which produces a `ColumnDef` node in `table_elts` in
        // the first place (`create_table` refuses anything there that is
        // not a `ColumnDef`).
        is_from_type: _,
        storage,
        storage_name,
        raw_default,
        // Only ever set by the analyzer rewriting a `DEFAULT` expression
        // into its stored form; the raw parser leaves it empty and
        // `raw_default` (checked below) is what a `DEFAULT` clause parses
        // to instead.
        cooked_default: _,
        identity,
        // Only ever set once the analyzer resolves `GENERATED ... AS
        // IDENTITY`'s backing sequence; `identity` (checked below) is the
        // raw parser's own signal, and it is always empty regardless (see
        // below).
        identity_sequence: _,
        generated,
        coll_clause,
        // Only ever set by the analyzer resolving a `COLLATE` clause's
        // collation; `coll_clause` (checked below) is the raw parser's
        // own signal.
        coll_oid: _,
        constraints,
        fdwoptions,
        // No PostgreSQL error is reported at a bare column definition's own
        // position; every error this head raises about a column instead
        // points at the constraint, type name or expression inside it.
        location: _,
    } = column;
    // `identity`/`generated` are always empty from `pg_query`'s raw
    // parser: `GENERATED ... AS IDENTITY` and `GENERATED ALWAYS AS (...)`
    // both arrive as their own `Constraint` nodes (`ConstrIdentity`,
    // `ConstrGenerated`, matched in `constraint_kind` below), the same way
    // `NOT NULL` does. These checks stay as a second, defensive line in
    // case a future `pg_query` release changes that.
    if coll_clause.is_some() {
        return Err(column_clause("COLLATE"));
    }
    if raw_default.is_some() {
        return Err(column_clause("DEFAULT"));
    }
    if !identity.is_empty() {
        return Err(column_clause("GENERATED AS IDENTITY"));
    }
    if !generated.is_empty() {
        return Err(column_clause("GENERATED ALWAYS AS"));
    }
    if !compression.is_empty() {
        return Err(column_clause("COMPRESSION"));
    }
    // `storage` is itself always empty from the raw parser; `STORAGE
    // <mode>` is carried in `storage_name` instead, so both are checked.
    if !storage.is_empty() || !storage_name.is_empty() {
        return Err(column_clause("STORAGE"));
    }
    if !fdwoptions.is_empty() {
        return Err(column_clause("OPTIONS"));
    }
    let type_name = type_name
        .as_ref()
        .ok_or_else(|| HeadError::internal("a column without a type"))?;
    let mut definition = ColumnDef {
        name: ColumnName::from_parse_tree(PROOF, colname.clone())?,
        ty: expr::cast_target_type(PROOF, type_name)?,
        primary_key: None,
        not_null: false,
    };
    for constraint in constraints {
        let PgNode::Constraint(constraint) = node(Some(constraint))? else {
            return Err(HeadError::internal(
                "a column constraint that is not a Constraint",
            ));
        };
        constraint_kind(constraint, &mut definition)?;
    }
    Ok(definition)
}

/// Every field of a bare `PRIMARY KEY` or `NOT NULL` column constraint
/// besides its own kind: named (`CONSTRAINT name ...`), `DEFERRABLE`/
/// `INITIALLY DEFERRED`, a storage parameter (`WITH (...)`) and an index
/// tablespace or name (`USING INDEX ...`) are real PostgreSQL clauses this
/// head cannot honor (a `PRIMARY KEY`'s catalog name and storage are
/// always synthesized, see `analyze/ddl.rs::create_table`), so each is its
/// own refusal. Every remaining field is one only a constraint kind this
/// head never admits (`CHECK`, `UNIQUE`, `EXCLUDE`, `FOREIGN KEY`) sets,
/// so seeing one set here is a parser inconsistency, not a clause to name.
fn refuse_constraint_extras(constraint: &PgConstraint) -> Result<(), HeadError> {
    let PgConstraint {
        contype: _,
        conname,
        deferrable,
        initdeferred,
        skip_validation,
        // Always `false` from the raw parser: this is the analyzer's own
        // record of whether a constraint still needs `VALIDATE
        // CONSTRAINT`, set opposite `skip_validation` only after the
        // constraint is actually created.
        initially_valid: _,
        is_no_inherit,
        // A `CHECK` constraint's own expression; unreachable here since
        // `ConstrCheck` is refused by name without inspecting it.
        raw_expr: _,
        cooked_expr,
        generated_when,
        inhcount,
        nulls_not_distinct,
        keys,
        including,
        exclusions,
        options,
        indexname,
        indexspace,
        reset_default_tblspc,
        access_method,
        where_clause,
        pktable,
        fk_attrs,
        pk_attrs,
        fk_matchtype,
        fk_upd_action,
        fk_del_action,
        fk_del_set_cols,
        old_conpfeqop,
        old_pktable_oid,
        // A `PRIMARY KEY`/`NOT NULL` constraint's own location is read by
        // `constraint_kind` below (for `PRIMARY KEY`'s position); nothing
        // here needs it a second time.
        location: _,
    } = constraint;
    if !conname.is_empty() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ColumnConstraintKind("a named constraint"),
        ));
    }
    if *deferrable || *initdeferred {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ColumnConstraintKind("DEFERRABLE or INITIALLY DEFERRED"),
        ));
    }
    if !options.is_empty() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ColumnConstraintKind("a constraint storage parameter"),
        ));
    }
    if !indexname.is_empty() || !indexspace.is_empty() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ColumnConstraintKind("USING INDEX"),
        ));
    }
    if *skip_validation
        || !cooked_expr.is_empty()
        || !generated_when.is_empty()
        || *inhcount != 0
        || *nulls_not_distinct
        || !keys.is_empty()
        || !including.is_empty()
        || !exclusions.is_empty()
        || *reset_default_tblspc
        || !access_method.is_empty()
        || where_clause.is_some()
        || pktable.is_some()
        || !fk_attrs.is_empty()
        || !pk_attrs.is_empty()
        || !fk_matchtype.is_empty()
        || !fk_upd_action.is_empty()
        || !fk_del_action.is_empty()
        || !fk_del_set_cols.is_empty()
        || !old_conpfeqop.is_empty()
        || *old_pktable_oid != 0
        || *is_no_inherit
    {
        return Err(HeadError::internal(
            "a PRIMARY KEY or NOT NULL column constraint carries a field only CHECK, UNIQUE, \
             EXCLUDE or FOREIGN KEY constraints set",
        ));
    }
    Ok(())
}

fn constraint_kind(constraint: &PgConstraint, definition: &mut ColumnDef) -> Result<(), HeadError> {
    match ConstrType::try_from(constraint.contype) {
        Ok(ConstrType::ConstrPrimary) => {
            refuse_constraint_extras(constraint)?;
            definition.primary_key =
                Some(Location::from_raw(constraint.location).ok_or_else(|| {
                    HeadError::internal("a PRIMARY KEY constraint has no location")
                })?);
            Ok(())
        }
        Ok(ConstrType::ConstrNotnull) => {
            refuse_constraint_extras(constraint)?;
            definition.not_null = true;
            Ok(())
        }
        Ok(ConstrType::ConstrDefault) => Err(column_clause("DEFAULT")),
        Ok(ConstrType::ConstrCheck) => Err(HeadError::not_supported(
            NotSupportedFeature::ColumnConstraintKind("CHECK"),
        )),
        Ok(ConstrType::ConstrUnique) => Err(HeadError::not_supported(
            NotSupportedFeature::ColumnConstraintKind("UNIQUE"),
        )),
        Ok(ConstrType::ConstrForeign) => Err(HeadError::not_supported(
            NotSupportedFeature::ColumnConstraintKind("REFERENCES"),
        )),
        // `ConstrNull` (a bare, no-op `NULL`), `ConstrIdentity`,
        // `ConstrGenerated`, `ConstrExclusion` and the `ConstrAttr*` forms
        // (`DEFERRABLE`/`INITIALLY DEFERRED`/etc. as their own node, the
        // shape a raw column constraint list actually uses; see
        // `refuse_constraint_extras`'s doc comment) are every remaining
        // `ConstrType`: none is admitted.
        _ => Err(HeadError::not_supported(
            NotSupportedFeature::ThisColumnConstraint,
        )),
    }
}

pub(super) fn create_role(create: &CreateRoleStmt) -> Result<Statement, HeadError> {
    let CreateRoleStmt {
        stmt_type,
        role,
        options,
    } = create;
    if !matches!(
        RoleStmtType::try_from(*stmt_type),
        Ok(RoleStmtType::RolestmtRole)
    ) {
        return Err(HeadError::not_supported(
            NotSupportedFeature::CreateUserAndCreateGroup,
        ));
    }
    let mut can_login = false;
    for option in options {
        let PgNode::DefElem(option) = node(Some(option))? else {
            return Err(HeadError::internal("a role option that is not a DefElem"));
        };
        let DefElem {
            // A role option is never namespaced or itself an ALTER-style
            // add/drop/set action; both are structurally impossible for
            // `CREATE ROLE`'s own option-list grammar production.
            defnamespace,
            defname,
            arg,
            defaction,
            // PostgreSQL never attaches a position to "unrecognized role
            // option" or similar `CREATE ROLE` errors.
            location: _,
        } = &**option;
        if !defnamespace.is_empty() {
            return Err(HeadError::internal(
                "a CREATE ROLE option carries a namespace",
            ));
        }
        if !matches!(
            DefElemAction::try_from(*defaction),
            Ok(DefElemAction::DefelemUnspec)
        ) {
            return Err(HeadError::internal(
                "a CREATE ROLE option carries an ALTER-style action",
            ));
        }
        match (defname.as_str(), arg.as_deref().map(|arg| node(Some(arg)))) {
            ("canlogin", Some(Ok(PgNode::Boolean(value)))) => can_login = value.boolval,
            (name, _) => {
                return Err(HeadError::not_supported(NotSupportedFeature::RoleOption(
                    name.to_string(),
                )))
            }
        }
    }
    Ok(Statement::CreateRole {
        name: RoleName::from_parse_tree(PROOF, role.clone())?,
        can_login,
    })
}

pub(super) fn grant_privileges(grant: &GrantStmt) -> Result<Statement, HeadError> {
    let GrantStmt {
        is_grant,
        targtype,
        objtype,
        objects,
        privileges,
        grantees,
        grant_option,
        grantor,
        // Only meaningful for `REVOKE` (`CASCADE`/`RESTRICT`), which
        // `is_grant` already refuses unconditionally regardless of this
        // field's value.
        behavior: _,
    } = grant;
    let grant_clause =
        |clause: &'static str| HeadError::not_supported(NotSupportedFeature::GrantClause(clause));
    if !is_grant {
        return Err(grant_clause("REVOKE"));
    }
    if !matches!(
        GrantTargetType::try_from(*targtype),
        Ok(GrantTargetType::AclTargetObject)
    ) {
        return Err(grant_clause("GRANT ON ALL ... IN SCHEMA"));
    }
    if *grant_option {
        return Err(grant_clause("GRANT with GRANT OPTION"));
    }
    if grantor.is_some() {
        return Err(grant_clause("GRANT with GRANTED BY"));
    }
    let privileges = if privileges.is_empty() {
        GrantedPrivileges::All
    } else {
        GrantedPrivileges::Named(
            privileges
                .iter()
                .map(|privilege| {
                    let PgNode::AccessPriv(privilege) = node(Some(privilege))? else {
                        return Err(HeadError::not_supported(
                            NotSupportedFeature::ColumnPrivileges,
                        ));
                    };
                    let AccessPriv { priv_name, cols } = privilege;
                    if !cols.is_empty() {
                        return Err(HeadError::not_supported(
                            NotSupportedFeature::ColumnPrivileges,
                        ));
                    }
                    Ok(PrivilegeKeyword::parsed(priv_name))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    let grantees = grantees
        .iter()
        .map(|grantee| grantee_name(grantee, NotSupportedFeature::GrantToPseudoRole))
        .collect::<Result<Vec<_>, _>>()?;
    let objects = match ObjectType::try_from(*objtype) {
        Ok(ObjectType::ObjectTable) => GrantObjects::Tables(
            objects
                .iter()
                .map(|object| {
                    let PgNode::RangeVar(relation) = node(Some(object))? else {
                        return Err(HeadError::internal("a GRANT table that is not a RangeVar"));
                    };
                    table_name(relation)
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Ok(ObjectType::ObjectSchema) => GrantObjects::Schemas(
            objects
                .iter()
                .map(|object| {
                    let PgNode::String(schema) = node(Some(object))? else {
                        return Err(HeadError::internal("a GRANT schema that is not a name"));
                    };
                    SchemaName::from_parse_tree(PROOF, schema.sval.clone())
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        _ => {
            return Err(HeadError::not_supported(
                NotSupportedFeature::GrantOnUnsupportedObjectType,
            ))
        }
    };
    Ok(Statement::Grant {
        privileges,
        objects,
        grantees,
    })
}

pub(super) fn create_policy(create: &CreatePolicyStmt) -> Result<Statement, HeadError> {
    let CreatePolicyStmt {
        policy_name,
        table,
        cmd_name,
        permissive,
        roles: role_list,
        qual,
        with_check,
    } = create;
    let command = PolicyCommand::from_parse_tree(cmd_name)?;
    if with_check.is_some() && matches!(command, PolicyCommand::Select | PolicyCommand::Delete) {
        return Err(HeadError::raise(
            PgError::WithCheckNotApplicableToSelectOrDelete,
        ));
    }
    if command != PolicyCommand::Select {
        return Err(HeadError::not_supported(
            NotSupportedFeature::PolicyCommandNotSelect,
        ));
    }
    if !permissive {
        return Err(HeadError::not_supported(
            NotSupportedFeature::PolicyRestrictive,
        ));
    }
    let Some(qual) = qual.as_deref() else {
        return Err(HeadError::not_supported(
            NotSupportedFeature::PolicyWithoutUsing,
        ));
    };
    let relation = table
        .as_ref()
        .ok_or_else(|| HeadError::internal("CREATE POLICY without a table"))?;
    let mut roles = Vec::new();
    for role in role_list {
        roles.push(grantee_name(
            role,
            NotSupportedFeature::PolicyForPseudoRole,
        )?);
    }
    if roles.len() > 1 && roles.iter().any(|role| matches!(role, GranteeName::Public)) {
        return Err(HeadError::not_supported(
            NotSupportedFeature::PublicCombinedWithOtherRoles,
        ));
    }
    if roles.is_empty() {
        roles.push(GranteeName::Public);
    }
    Ok(Statement::CreatePolicy {
        name: PolicyName::from_parse_tree(PROOF, policy_name.clone())?,
        table: table_name(relation)?,
        roles,
        command,
        using: expr::admit(PROOF, node(Some(qual))?)?,
    })
}

pub(super) fn alter_row_security(alter: &AlterTableStmt) -> Result<Statement, HeadError> {
    let AlterTableStmt {
        relation,
        cmds,
        objtype,
        missing_ok,
    } = alter;
    if !matches!(ObjectType::try_from(*objtype), Ok(ObjectType::ObjectTable)) {
        return Err(HeadError::not_supported(
            NotSupportedFeature::AlterOnUnsupportedObjectType,
        ));
    }
    if *missing_ok {
        return Err(HeadError::not_supported(
            NotSupportedFeature::AlterTableIfExists,
        ));
    }
    let relation = relation
        .as_ref()
        .ok_or_else(|| HeadError::internal("ALTER TABLE without a relation"))?;
    let changes = cmds
        .iter()
        .map(|command| {
            let PgNode::AlterTableCmd(command) = node(Some(command))? else {
                return Err(HeadError::internal(
                    "an ALTER TABLE command that is not a command",
                ));
            };
            row_security_change(command)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Statement::AlterRowSecurity {
        table: table_name(relation)?,
        changes,
    })
}

fn row_security_change(
    command: &pg_query::protobuf::AlterTableCmd,
) -> Result<RowSecurityChange, HeadError> {
    let pg_query::protobuf::AlterTableCmd {
        subtype,
        // None of `ENABLE`/`DISABLE`/`FORCE`/`NO FORCE ROW LEVEL SECURITY`
        // takes a name, a column number, a new owner, a definition node,
        // `CASCADE`/`RESTRICT`, `IF EXISTS` or recursion: PostgreSQL's own
        // grammar production for these four subcommands never sets any of
        // them, so any non-default value here is a parser inconsistency,
        // not a clause to refuse by name.
        name,
        num,
        newowner,
        def,
        behavior,
        missing_ok,
        recurse,
    } = command;
    let change = match AlterTableType::try_from(*subtype) {
        Ok(AlterTableType::AtEnableRowSecurity) => RowSecurityChange::Enable,
        Ok(AlterTableType::AtDisableRowSecurity) => RowSecurityChange::Disable,
        Ok(AlterTableType::AtForceRowSecurity) => RowSecurityChange::Force,
        Ok(AlterTableType::AtNoForceRowSecurity) => RowSecurityChange::NoForce,
        _ => {
            return Err(HeadError::not_supported(
                NotSupportedFeature::AlterTableSubcommand,
            ))
        }
    };
    if !name.is_empty()
        || *num != 0
        || newowner.is_some()
        || def.is_some()
        || !matches!(
            pg_query::protobuf::DropBehavior::try_from(*behavior),
            Ok(pg_query::protobuf::DropBehavior::DropRestrict)
        )
        || *missing_ok
        || *recurse
    {
        return Err(HeadError::internal(
            "a ROW LEVEL SECURITY subcommand carries a field only another ALTER TABLE subcommand sets",
        ));
    }
    Ok(change)
}

/// A grantee in GRANT or a policy's TO list: a named role or PUBLIC.
/// `CURRENT_USER`, `SESSION_USER` and `CURRENT_ROLE` are refused with
/// `pseudo_role`.
fn grantee_name(raw: &Node, pseudo_role: NotSupportedFeature) -> Result<GranteeName, HeadError> {
    let PgNode::RoleSpec(role) = node(Some(raw))? else {
        return Err(HeadError::not_supported(pseudo_role));
    };
    let RoleSpec {
        roletype,
        rolename,
        location: _,
    } = role;
    match RoleSpecType::try_from(*roletype) {
        Ok(RoleSpecType::RolespecCstring) => Ok(GranteeName::Role(RoleName::from_parse_tree(
            PROOF,
            rolename.clone(),
        )?)),
        Ok(RoleSpecType::RolespecPublic) => Ok(GranteeName::Public),
        Ok(
            RoleSpecType::Undefined
            | RoleSpecType::RolespecCurrentRole
            | RoleSpecType::RolespecCurrentUser
            | RoleSpecType::RolespecSessionUser,
        )
        | Err(_) => Err(HeadError::not_supported(pseudo_role)),
    }
}
