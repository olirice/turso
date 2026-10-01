//! One typed error per message the head raises (`PgError`), one closed
//! feature enum for every "not supported" refusal (`NotSupportedFeature`),
//! and one renderer (`render`, `not_supported_phrase`) turning either into
//! PostgreSQL's own `(SQLSTATE, message, detail, hint)`. Neither match has
//! a wildcard arm (enforced below), so a new variant forces its template.
#![deny(clippy::wildcard_enum_match_arm)]

use std::fmt;

use turso_core::LimboError;

use crate::analyze::types::TypeHandle;
use crate::ident::{
    ColumnName, ConstraintName, FunctionName, MessageName, PolicyName, PreparedName, RoleName,
    SchemaName, TableName,
};
use crate::parse::statement::CommandTag;
use crate::parse::Location;
use crate::security::privileges::ObjectKind;

include!(concat!(env!("OUT_DIR"), "/sqlstate_generated.rs"));

/// Every error the head raises, one variant per distinct message
/// PostgreSQL 18 shows (checked live; see each variant's raise sites). The
/// only two escapes from "one variant, one PostgreSQL message" are
/// [`PgError::NotSupported`], one variant carrying the closed
/// [`NotSupportedFeature`] enum (every one of its variants renders through
/// the same "X is not supported" template PostgreSQL itself does not use,
/// since these are refusals by design, not messages PostgreSQL raises),
/// and [`PgError::Internal`] and [`PgError::Syntax`], which carry a
/// message that is not PostgreSQL's own: an internal error is an
/// invariant violation this head's own code detected, and a syntax error
/// is `libpg_query`'s own reproduction of PostgreSQL's grammar-dependent
/// text, which cannot be templated.
///
/// Raise sites pass typed data (the head's own identifier newtypes,
/// `TypeHandle`), never a formatted message; [`render`] is the only place
/// a `PgError` becomes text, and its match has no wildcard arm, so adding
/// a variant here forces a template there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PgError {
    TransactionAborted,
    AuthRoleDoesNotExist(RoleName),
    AuthRoleNotPermittedToLogin(RoleName),
    NewRowViolatesRowSecurity {
        table: TableName,
    },
    ReservedRoleName(RoleName),
    RoleAlreadyExists(RoleName),
    PolicyAlreadyExists {
        policy: PolicyName,
        table: TableName,
    },
    RowSecurityPolicyAffectsQuery {
        table: TableName,
        caller_is_owner: bool,
    },
    MultiplePrimaryKeys(TableName),
    ColumnSpecifiedMoreThanOnce(ColumnName),
    RelationAlreadyExists(TableName),
    NotNullViolation {
        column: ColumnName,
        table: TableName,
        failing_row: String,
    },
    /// `Privileges::parse`'s only caller is its own `#[cfg(test)]` tests;
    /// production code always builds `Privileges` from already-validated
    /// catalog ACL text, never from an arbitrary letter that could fail.
    #[cfg(test)]
    UnknownPrivilegeLetter(char),
    ReadOnlyTransaction {
        command: CommandTag,
    },
    PermissionDeniedForSchema(SchemaName),
    PermissionDeniedToCreateRole,
    MustBeOwnerOfTable(TableName),
    PermissionDeniedForTable(TableName),
    PermissionDeniedForObject {
        kind: ObjectKind,
        name: MessageName,
    },
    ArgumentMustBeBoolean {
        what: &'static str,
        actual: TypeHandle,
    },
    IndeterminateEmptyArray,
    UndefinedFunction {
        name: String,
        arg_types: Vec<TypeHandle>,
    },
    CaseTypesCannotBeMatched {
        left: TypeHandle,
        right: TypeHandle,
    },
    /// Two `CASE` arms share a `typcategory` but neither implicitly casts
    /// to the other (`regclass`/`regproc`, both only through `oid`):
    /// PostgreSQL 18 raises a different error than
    /// `CaseTypesCannotBeMatched` there (probed).
    CaseCouldNotConvertType {
        from: TypeHandle,
        to: TypeHandle,
    },
    SubqueryInFromRequiresAlias,
    WithCheckNotApplicableToSelectOrDelete,
    UnionColumnCountMismatch,
    ColumnListLengthMismatchDerivedTable,
    ColumnListLengthMismatchFunction,
    UndefinedRelation {
        qualified: String,
    },
    CrossDatabaseReference(String),
    ImproperRelationName(String),
    InvalidNameSyntax,
    UndefinedColumn {
        qualifier: Option<TableName>,
        column: ColumnName,
    },
    AmbiguousColumnReference(ColumnName),
    MissingFromClauseEntry(TableName),
    OperatorDoesNotExist {
        op: &'static str,
        left: OperandType,
        right: OperandType,
    },
    IntegerOutOfRange,
    InvalidInputSyntax {
        ty: TypeHandle,
        text: String,
    },
    ValueOutOfRangeForType {
        text: String,
        ty: TypeHandle,
    },
    NestedAggregateCall,
    UngroupedColumn {
        relation: TableName,
        column: ColumnName,
    },
    UndefinedParameter(u32),
    DuplicatePreparedStatement(PreparedName),
    UnknownPreparedStatement(PreparedName),
    WrongParameterCount {
        name: PreparedName,
        expected: usize,
        got: usize,
    },
    ActiveSqlTransactionIsolationLevel,
    ActiveSqlTransactionReadWriteMode,
    SchemaDoesNotExist(SchemaName),
    UndefinedType(String),
    RoleDoesNotExist(RoleName),
    UnrecognizedConfigurationParameter(String),
    InvalidPrivilegeType {
        privilege: String,
        kind: &'static str,
    },
    PermissionDeniedToCreateInPgCatalog(TableName),
    NoSchemaSelectedToCreateIn,
    CannotInsertIntoView(TableName),
    ColumnOfRelationDoesNotExist {
        column: ColumnName,
        table: TableName,
    },
    ValuesListsMustBeSameLength,
    InsertMoreExpressionsThanTargets,
    InsertMoreTargetsThanExpressions,
    OrderByPositionNotInSelectList(i64),
    LockTableOutsideTransaction,
    CannotUseSubqueryInPosition {
        position: &'static str,
    },
    AggregateNotAllowedInPosition {
        position: &'static str,
    },
    SetReturningNotAllowedInPosition {
        position: &'static str,
    },
    SetReturningMustAppearAtTopLevelOfFrom,
    /// `key` and `value` are already rendered text (the constraint's key
    /// column, SQL-quoted, and the conflicting value's own display form),
    /// not typed identifiers or a `turso_core::Value`: `engine::check_row_unique`
    /// builds both from data the engine returned, not from a parsed name.
    DuplicateKeyValue {
        constraint: ConstraintName,
        key: String,
        value: String,
    },
    /// `libpg_query`'s own reproduction of PostgreSQL's grammar-dependent
    /// parse-error text: not templated here because PostgreSQL's grammar,
    /// not this table, decides its wording.
    Syntax(String),
    NotSupported(NotSupportedFeature),
    /// An invariant violation, not a PostgreSQL message: this head's own
    /// code detected a state it never expects, so there is no PostgreSQL
    /// wording to match.
    Internal(String),
}

/// The closed set of "not supported" refusals: every one renders through
/// PostgreSQL's own "X is not supported" convention (`0A000`), with `X`
/// this variant's own phrase (see [`not_supported_phrase`]). A handful of
/// variants carry a `&'static str` drawn from a small fixed array at the
/// one raise site that builds it (for example the clause names `CREATE
/// TABLE` checks for); that is still typed data, never a runtime-built
/// message, so the raise site never formats text itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NotSupportedFeature {
    PrepareNonSelect,
    LockTableMode,
    LockTableNowait,
    TransactionChain,
    SavepointsAndTwoPhaseCommands,
    IsolationLevel(String),
    DeferrableTransactions,
    TransactionOption(String),
    SetLocalTransaction,
    ResetSetting,
    SetToDefaultOrFromCurrent,
    SetValue,
    IdentifierTooLong,
    InsertClause(&'static str),
    InsertIntoColumnSubscriptOrField,
    InsertFromNonValues,
    FetchWithTies,
    SelectClause(&'static str),
    LimitOrOffset,
    SelectWithNoColumns,
    SetOperation(&'static str),
    DistinctOn,
    SelectListSubscriptOrField,
    ParenthesizedJoinRhs,
    JoinOperandNotTable,
    NaturalJoin,
    AliasedJoin,
    JoinUsing,
    RightJoin,
    FullJoin,
    ThisJoinType,
    JoinWithoutOn,
    OrderByUsing,
    ColumnAliasesOnTableReference,
    Lateral,
    WithOrdinality,
    ColumnDefinitionList,
    RowsFromMultipleFunctions,
    ThisTableFunction,
    AggregateOrderByWithAnotherModifier,
    AggregateOrderByWithQueryOrderBy,
    AggregateOrderByNotArrayAgg,
    ArrayAggOrderByMultipleArgs,
    CreateTableClause(&'static str),
    TemporaryOrUnloggedTables,
    CreateTableConstraints,
    TableWithNoColumns,
    ColumnClause(&'static str),
    ColumnConstraintKind(&'static str),
    ThisColumnConstraint,
    CreateUserAndCreateGroup,
    RoleOption(String),
    GrantClause(&'static str),
    ColumnPrivileges,
    GrantToPseudoRole,
    GrantOnUnsupportedObjectType,
    PolicyCommandNotSelect,
    PolicyRestrictive,
    PolicyWithoutUsing,
    PolicyForPseudoRole,
    PublicCombinedWithOtherRoles,
    AlterOnUnsupportedObjectType,
    AlterTableIfExists,
    AlterTableSubcommand,
    MultipleStatements,
    UnicodeEscapeIdentifiers,
    /// Any admitted-statement or admitted-expression form outside the
    /// closed grammar this head accepts: PostgreSQL's own parser accepts
    /// it, so the only description available is `libpg_query`'s own
    /// deparse of the rejected node (or, when even that fails, a fallback
    /// phrase from the one place this head tried to admit it). The set of
    /// rejected forms is unbounded (anything outside `pg/head/ARCH.md`'s
    /// admitted scope), so this is the one `NotSupportedFeature` variant
    /// that cannot carry a closed enum.
    Node(String),
    CatalogQualifiedRelationName,
    UnsupportedSchema,
    TableAliases,
    ParameterNumberOutOfRange,
    PositionalParameterZero,
    ParameterWithoutDeclaredType(u32),
    FunctionCallClause(&'static str),
    NamedFunctionArguments,
    SchemaQualifiedFunctionCall,
    StarMixedWithOtherSelectItems,
    QualifiedColumnReferences,
    NonIntegerNumericLiteral,
    BitStringLiterals,
    ColumnType(String),
    TypeModifiers,
    ArraySetofPctType,
    AnyWithOperator(String),
    ChainedSubscript,
    FieldAccessOnValue,
    ArraySlice,
    Operator(String),
    ThisExpression,
    SchemaQualifiedOperators,
    UnsupportedSubqueryForm,
    SubqueryOperatorNotIn,
    ArrayLiteralNotBraceDelimited,
    ArrayLiteralElementQuotedOrNested,
    AclDefaultEmptyObjectType,
    AclDefaultObjectType,
    ArrayOfType(TypeHandle),
    RegprocCastFromName,
    SubqueryMultipleColumns,
    UnionOrderByExpression,
    StarWithoutFromClause,
    SubscriptOnType(TypeHandle),
    AnyOverType(TypeHandle),
    ArrayOfElementType(TypeHandle),
    ArrayTypesCannotBeMatched(TypeHandle, TypeHandle),
    NonConstantArgumentToSetReturningFunction(FunctionName),
    ArgumentNotColumnOrConstant(FunctionName),
    InPolicyExpression(PolicyExpressionForm),
    ArrayAggOverThisType,
    ExpressionNotLiteralOrCast,
    ArrayValueInContext,
    NonConstantArgumentOutsideSelectItem {
        function: &'static str,
    },
    FunctionInPosition {
        function: &'static str,
        position: &'static str,
    },
    SubqueryInPosition(&'static str),
    AggregateInPosition(&'static str),
    SetReturningInPosition(&'static str),
    SetVariableTo {
        name: &'static str,
        values: Vec<String>,
    },
    NullArgumentToRegisteredFunction,
    CatalogRelationAction {
        action: CommandTag,
        table: TableName,
    },
    OutputValueType(TypeHandle),
}

/// One side of [`PgError::OperatorDoesNotExist`]: either a resolved type,
/// or (an untyped literal's own PostgreSQL name, when no `TypeHandle` was
/// ever assigned to it, e.g. `"integer"`/`"text"` for a bare literal that
/// failed to coerce before it reached any type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperandType {
    Type(TypeHandle),
    Literal(&'static str),
}

impl OperandType {
    fn display(self) -> &'static str {
        match self {
            OperandType::Type(ty) => ty.display_name(),
            OperandType::Literal(text) => text,
        }
    }
}

/// The closed set of forms `render::policy_expression` refuses: PostgreSQL
/// 18 itself permits each inside a policy's `USING` (`Context::permit`
/// already refuses everything else that could reach a `Typed` tree there),
/// but this renderer cannot yet reproduce PostgreSQL's own `pg_get_expr`
/// text for one, so `security::enforcement::canonicalize` turns it into
/// `CREATE POLICY`'s own `0A000` instead of storing it wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PolicyExpressionForm {
    FunctionCall,
    TableOid,
    ArraySubscript,
    Any,
    InList,
    ArrayLiteral,
}

impl PolicyExpressionForm {
    fn phrase(self) -> &'static str {
        match self {
            PolicyExpressionForm::FunctionCall => "a function call",
            PolicyExpressionForm::TableOid => "tableoid",
            PolicyExpressionForm::ArraySubscript => "an array subscript",
            PolicyExpressionForm::Any => "ANY",
            PolicyExpressionForm::InList => "an IN list",
            PolicyExpressionForm::ArrayLiteral => "an array literal",
        }
    }
}

/// A `PgError`, rendered: PostgreSQL's `(SQLSTATE, message, detail, hint)`.
/// [`render`] is the only function that builds one.
struct Rendered {
    state: SqlState,
    message: String,
    detail: Option<String>,
    hint: Option<String>,
}

impl Rendered {
    fn new(state: SqlState, message: String) -> Self {
        Rendered {
            state,
            message,
            detail: None,
            hint: None,
        }
    }

    fn with_detail(mut self, detail: String) -> Self {
        self.detail = Some(detail);
        self
    }

    fn with_hint(mut self, hint: &'static str) -> Self {
        self.hint = Some(hint.to_string());
        self
    }
}

fn type_list(types: &[TypeHandle]) -> String {
    types
        .iter()
        .map(|ty| ty.display_name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The one renderer: every `PgError` variant to PostgreSQL's own
/// `(SQLSTATE, message, detail, hint)`, checked live against PostgreSQL
/// 18. No wildcard arm (enforced by `#![deny(clippy::wildcard_enum_match_arm)]`
/// below), so a new variant forces a template here.
fn render(error: PgError) -> Rendered {
    match error {
        PgError::TransactionAborted => Rendered::new(
            SqlState::InFailedSqlTransaction,
            "current transaction is aborted, commands ignored until end of transaction block"
                .to_string(),
        ),
        PgError::AuthRoleDoesNotExist(role) => Rendered::new(
            SqlState::InvalidAuthorizationSpecification,
            format!("role \"{}\" does not exist", role.render_message().as_message()),
        ),
        PgError::AuthRoleNotPermittedToLogin(role) => Rendered::new(
            SqlState::InvalidAuthorizationSpecification,
            format!(
                "role \"{}\" is not permitted to log in",
                role.render_message().as_message()
            ),
        ),
        PgError::NewRowViolatesRowSecurity { table } => Rendered::new(
            SqlState::InsufficientPrivilege,
            format!(
                "new row violates row-level security policy for table \"{}\"",
                table.render_message().as_message()
            ),
        ),
        PgError::ReservedRoleName(name) => Rendered::new(
            SqlState::ReservedName,
            format!("role name \"{}\" is reserved", name.render_message().as_message()),
        )
        .with_detail("Role names starting with \"pg_\" are reserved.".to_string()),
        PgError::RoleAlreadyExists(name) => Rendered::new(
            SqlState::DuplicateObject,
            format!("role \"{}\" already exists", name.render_message().as_message()),
        ),
        PgError::PolicyAlreadyExists { policy, table } => Rendered::new(
            SqlState::DuplicateObject,
            format!(
                "policy \"{}\" for table \"{}\" already exists",
                policy.render_message().as_message(),
                table.render_message().as_message()
            ),
        ),
        PgError::RowSecurityPolicyAffectsQuery {
            table,
            caller_is_owner,
        } => {
            let rendered = Rendered::new(
                SqlState::InsufficientPrivilege,
                format!(
                    "query would be affected by row-level security policy for table \"{}\"",
                    table.render_message().as_message()
                ),
            );
            if caller_is_owner {
                rendered.with_hint(
                    "To disable the policy for the table's owner, use ALTER TABLE NO FORCE ROW LEVEL SECURITY.",
                )
            } else {
                rendered
            }
        }
        PgError::MultiplePrimaryKeys(table) => Rendered::new(
            SqlState::InvalidTableDefinition,
            format!(
                "multiple primary keys for table \"{}\" are not allowed",
                table.render_message().as_message()
            ),
        ),
        PgError::ColumnSpecifiedMoreThanOnce(column) => Rendered::new(
            SqlState::DuplicateColumn,
            format!(
                "column \"{}\" specified more than once",
                column.render_message().as_message()
            ),
        ),
        PgError::RelationAlreadyExists(name) => Rendered::new(
            SqlState::DuplicateTable,
            format!("relation \"{}\" already exists", name.render_message().as_message()),
        ),
        PgError::NotNullViolation {
            column,
            table,
            failing_row,
        } => Rendered::new(
            SqlState::NotNullViolation,
            format!(
                "null value in column \"{}\" of relation \"{}\" violates not-null constraint",
                column.render_message().as_message(),
                table.render_message().as_message()
            ),
        )
        .with_detail(format!("Failing row contains ({failing_row}).")),
        #[cfg(test)]
        PgError::UnknownPrivilegeLetter(letter) => Rendered::new(
            SqlState::InvalidTextRepresentation,
            format!("unknown privilege letter: {letter}"),
        ),
        PgError::ReadOnlyTransaction { command } => Rendered::new(
            SqlState::ReadOnlySqlTransaction,
            format!("cannot execute {command} in a read-only transaction"),
        ),
        PgError::PermissionDeniedForSchema(schema) => Rendered::new(
            SqlState::InsufficientPrivilege,
            format!("permission denied for schema {}", schema.as_str()),
        ),
        PgError::PermissionDeniedToCreateRole => Rendered::new(
            SqlState::InsufficientPrivilege,
            "permission denied to create role".to_string(),
        )
        .with_detail("Only roles with the CREATEROLE attribute may create roles.".to_string()),
        PgError::MustBeOwnerOfTable(table) => Rendered::new(
            SqlState::InsufficientPrivilege,
            format!("must be owner of table {}", table.render_message().as_message()),
        ),
        PgError::PermissionDeniedForTable(table) => Rendered::new(
            SqlState::InsufficientPrivilege,
            format!("permission denied for table {}", table.render_message().as_message()),
        ),
        PgError::PermissionDeniedForObject { kind, name } => Rendered::new(
            SqlState::InsufficientPrivilege,
            format!("permission denied for {} {}", kind.name(), name.as_message()),
        ),
        PgError::ArgumentMustBeBoolean { what, actual } => Rendered::new(
            SqlState::DatatypeMismatch,
            format!(
                "argument of {what} must be type boolean, not type {}",
                actual.display_name()
            ),
        ),
        PgError::IndeterminateEmptyArray => Rendered::new(
            SqlState::IndeterminateDatatype,
            "cannot determine type of empty array".to_string(),
        ),
        PgError::UndefinedFunction { name, arg_types } => Rendered::new(
            SqlState::UndefinedFunction,
            format!("function {name}({}) does not exist", type_list(&arg_types)),
        )
        .with_hint(
            "No function matches the given name and argument types. You might need to add explicit type casts.",
        ),
        PgError::CaseTypesCannotBeMatched { left, right } => Rendered::new(
            SqlState::DatatypeMismatch,
            format!(
                "CASE types {} and {} cannot be matched",
                left.display_name(),
                right.display_name()
            ),
        ),
        PgError::CaseCouldNotConvertType { from, to } => Rendered::new(
            SqlState::CannotCoerce,
            format!(
                "CASE/WHEN could not convert type {} to {}",
                from.display_name(),
                to.display_name()
            ),
        ),
        PgError::SubqueryInFromRequiresAlias => Rendered::new(
            SqlState::SyntaxError,
            "subquery in FROM must have an alias".to_string(),
        ),
        PgError::WithCheckNotApplicableToSelectOrDelete => Rendered::new(
            SqlState::SyntaxError,
            "WITH CHECK cannot be applied to SELECT or DELETE".to_string(),
        ),
        PgError::UnionColumnCountMismatch => Rendered::new(
            SqlState::SyntaxError,
            "each UNION query must have the same number of columns".to_string(),
        ),
        PgError::ColumnListLengthMismatchDerivedTable => Rendered::new(
            SqlState::SyntaxError,
            "a column list's length does not match its derived table".to_string(),
        ),
        PgError::ColumnListLengthMismatchFunction => Rendered::new(
            SqlState::SyntaxError,
            "a column list's length does not match its function's columns".to_string(),
        ),
        PgError::UndefinedRelation { qualified } => Rendered::new(
            SqlState::UndefinedTable,
            format!("relation \"{qualified}\" does not exist"),
        ),
        PgError::CrossDatabaseReference(qualified) => Rendered::new(
            SqlState::FeatureNotSupported,
            format!("cross-database references are not implemented: \"{qualified}\""),
        ),
        PgError::ImproperRelationName(qualified) => Rendered::new(
            SqlState::SyntaxError,
            format!("improper relation name (too many dotted names): {qualified}"),
        ),
        PgError::InvalidNameSyntax => {
            Rendered::new(SqlState::InvalidName, "invalid name syntax".to_string())
        }
        PgError::UndefinedColumn { qualifier, column } => Rendered::new(
            SqlState::UndefinedColumn,
            match qualifier {
                Some(qualifier) => {
                    format!("column {}.{} does not exist", qualifier.as_str(), column.as_str())
                }
                None => format!(
                    "column \"{}\" does not exist",
                    column.render_message().as_message()
                ),
            },
        ),
        PgError::AmbiguousColumnReference(name) => Rendered::new(
            SqlState::AmbiguousColumn,
            format!(
                "column reference \"{}\" is ambiguous",
                name.render_message().as_message()
            ),
        ),
        PgError::MissingFromClauseEntry(name) => Rendered::new(
            SqlState::UndefinedTable,
            format!(
                "missing FROM-clause entry for table \"{}\"",
                name.render_message().as_message()
            ),
        ),
        PgError::OperatorDoesNotExist { op, left, right } => Rendered::new(
            SqlState::UndefinedFunction,
            format!(
                "operator does not exist: {} {op} {}",
                left.display(),
                right.display()
            ),
        )
        .with_hint(
            "No operator matches the given name and argument types. You might need to add explicit type casts.",
        ),
        PgError::IntegerOutOfRange => Rendered::new(
            SqlState::NumericValueOutOfRange,
            "integer out of range".to_string(),
        ),
        PgError::InvalidInputSyntax { ty, text } => Rendered::new(
            SqlState::InvalidTextRepresentation,
            format!("invalid input syntax for type {}: \"{text}\"", ty.display_name()),
        ),
        PgError::ValueOutOfRangeForType { text, ty } => Rendered::new(
            SqlState::NumericValueOutOfRange,
            format!("value \"{text}\" is out of range for type {}", ty.display_name()),
        ),
        PgError::NestedAggregateCall => Rendered::new(
            SqlState::GroupingError,
            "aggregate function calls cannot be nested".to_string(),
        ),
        PgError::UngroupedColumn { relation, column } => Rendered::new(
            SqlState::GroupingError,
            format!(
                "column \"{}.{}\" must appear in the GROUP BY clause or be used in an aggregate function",
                relation.render_message().as_message(),
                column.render_message().as_message(),
            ),
        ),
        PgError::UndefinedParameter(number) => Rendered::new(
            SqlState::UndefinedParameter,
            format!("there is no parameter ${number}"),
        ),
        PgError::DuplicatePreparedStatement(name) => Rendered::new(
            SqlState::DuplicatePreparedStatement,
            format!(
                "prepared statement \"{}\" already exists",
                name.render_message().as_message()
            ),
        ),
        PgError::UnknownPreparedStatement(name) => Rendered::new(
            SqlState::InvalidSqlStatementName,
            format!(
                "prepared statement \"{}\" does not exist",
                name.render_message().as_message()
            ),
        ),
        PgError::WrongParameterCount {
            name,
            expected,
            got,
        } => Rendered::new(
            SqlState::SyntaxError,
            format!(
                "wrong number of parameters for prepared statement \"{}\"",
                name.render_message().as_message()
            ),
        )
        .with_detail(format!("Expected {expected} parameters but got {got}.")),
        PgError::ActiveSqlTransactionIsolationLevel => Rendered::new(
            SqlState::ActiveSqlTransaction,
            "SET TRANSACTION ISOLATION LEVEL must be called before any query".to_string(),
        ),
        PgError::ActiveSqlTransactionReadWriteMode => Rendered::new(
            SqlState::ActiveSqlTransaction,
            "transaction read-write mode must be set before any query".to_string(),
        ),
        PgError::SchemaDoesNotExist(name) => Rendered::new(
            SqlState::InvalidSchemaName,
            format!("schema \"{}\" does not exist", name.render_message().as_message()),
        ),
        PgError::UndefinedType(joined) => Rendered::new(
            SqlState::UndefinedObject,
            format!("type \"{joined}\" does not exist"),
        ),
        PgError::RoleDoesNotExist(name) => Rendered::new(
            SqlState::UndefinedObject,
            format!("role \"{}\" does not exist", name.render_message().as_message()),
        ),
        PgError::UnrecognizedConfigurationParameter(name) => Rendered::new(
            SqlState::UndefinedObject,
            format!("unrecognized configuration parameter \"{name}\""),
        ),
        PgError::InvalidPrivilegeType { privilege, kind } => Rendered::new(
            SqlState::InvalidGrantOperation,
            format!("invalid privilege type {privilege} for {kind}"),
        ),
        PgError::PermissionDeniedToCreateInPgCatalog(table) => Rendered::new(
            SqlState::InsufficientPrivilege,
            format!(
                "permission denied to create \"pg_catalog.{}\"",
                table.render_message().as_message()
            ),
        )
        .with_detail("System catalog modifications are currently disallowed.".to_string()),
        PgError::NoSchemaSelectedToCreateIn => Rendered::new(
            SqlState::InvalidSchemaName,
            "no schema has been selected to create in".to_string(),
        ),
        PgError::CannotInsertIntoView(name) => Rendered::new(
            SqlState::WrongObjectType,
            format!("cannot insert into view \"{}\"", name.render_message().as_message()),
        )
        .with_detail(
            "Views that do not select from a single table or view are not automatically updatable."
                .to_string(),
        )
        .with_hint(
            "To enable inserting into the view, provide an INSTEAD OF INSERT trigger or an unconditional ON INSERT DO INSTEAD rule.",
        ),
        PgError::ColumnOfRelationDoesNotExist { column, table } => Rendered::new(
            SqlState::UndefinedColumn,
            format!(
                "column \"{}\" of relation \"{}\" does not exist",
                column.render_message().as_message(),
                table.render_message().as_message()
            ),
        ),
        PgError::ValuesListsMustBeSameLength => Rendered::new(
            SqlState::SyntaxError,
            "VALUES lists must all be the same length".to_string(),
        ),
        PgError::InsertMoreExpressionsThanTargets => Rendered::new(
            SqlState::SyntaxError,
            "INSERT has more expressions than target columns".to_string(),
        ),
        PgError::InsertMoreTargetsThanExpressions => Rendered::new(
            SqlState::SyntaxError,
            "INSERT has more target columns than expressions".to_string(),
        ),
        PgError::OrderByPositionNotInSelectList(position) => Rendered::new(
            SqlState::InvalidColumnReference,
            format!("ORDER BY position {position} is not in select list"),
        ),
        PgError::LockTableOutsideTransaction => Rendered::new(
            SqlState::NoActiveSqlTransaction,
            "LOCK TABLE can only be used in transaction blocks".to_string(),
        ),
        PgError::CannotUseSubqueryInPosition { position } => Rendered::new(
            SqlState::FeatureNotSupported,
            format!("cannot use subquery in {position}"),
        ),
        PgError::AggregateNotAllowedInPosition { position } => Rendered::new(
            SqlState::GroupingError,
            format!("aggregate functions are not allowed in {position}"),
        ),
        PgError::SetReturningNotAllowedInPosition { position } => Rendered::new(
            SqlState::FeatureNotSupported,
            format!("set-returning functions are not allowed in {position}"),
        ),
        PgError::SetReturningMustAppearAtTopLevelOfFrom => Rendered::new(
            SqlState::FeatureNotSupported,
            "set-returning functions must appear at top level of FROM".to_string(),
        ),
        PgError::DuplicateKeyValue {
            constraint,
            key,
            value,
        } => Rendered::new(
            SqlState::UniqueViolation,
            format!(
                "duplicate key value violates unique constraint \"{}\"",
                constraint.render_message().as_message()
            ),
        )
        .with_detail(format!("Key ({key})=({value}) already exists.")),
        PgError::Syntax(message) => Rendered::new(SqlState::SyntaxError, message),
        PgError::NotSupported(feature) => Rendered::new(
            SqlState::FeatureNotSupported,
            format!("{} is not supported", not_supported_phrase(feature)),
        ),
        PgError::Internal(message) => Rendered::new(SqlState::Internal, message),
    }
}

/// Every [`NotSupportedFeature`]'s own phrase, filled into PostgreSQL's "X
/// is not supported" template by [`render`]. No wildcard arm, so a new
/// variant forces a phrase here.
fn not_supported_phrase(feature: NotSupportedFeature) -> String {
    match feature {
        NotSupportedFeature::PrepareNonSelect => {
            "PREPARE of a statement other than SELECT".to_string()
        }
        NotSupportedFeature::LockTableMode => {
            "LOCK TABLE in modes other than ACCESS SHARE".to_string()
        }
        NotSupportedFeature::LockTableNowait => "LOCK TABLE NOWAIT".to_string(),
        NotSupportedFeature::TransactionChain => "AND CHAIN".to_string(),
        NotSupportedFeature::SavepointsAndTwoPhaseCommands => {
            "savepoints and two-phase transaction commands".to_string()
        }
        NotSupportedFeature::IsolationLevel(level) => format!("the {level} isolation level"),
        NotSupportedFeature::DeferrableTransactions => "DEFERRABLE transactions".to_string(),
        NotSupportedFeature::TransactionOption(name) => format!("the transaction option {name}"),
        NotSupportedFeature::SetLocalTransaction => "SET LOCAL TRANSACTION".to_string(),
        NotSupportedFeature::ResetSetting => "RESET".to_string(),
        NotSupportedFeature::SetToDefaultOrFromCurrent => {
            "SET ... TO DEFAULT and SET ... FROM CURRENT".to_string()
        }
        NotSupportedFeature::SetValue => "this SET value".to_string(),
        NotSupportedFeature::IdentifierTooLong => "identifiers longer than 63 bytes".to_string(),
        NotSupportedFeature::InsertClause(clause) => format!("INSERT with {clause}"),
        NotSupportedFeature::InsertIntoColumnSubscriptOrField => {
            "INSERT into a column subscript or field".to_string()
        }
        NotSupportedFeature::InsertFromNonValues => {
            "INSERT from anything other than VALUES".to_string()
        }
        NotSupportedFeature::FetchWithTies => "FETCH ... WITH TIES".to_string(),
        NotSupportedFeature::SelectClause(clause) => format!("SELECT with {clause}"),
        NotSupportedFeature::LimitOrOffset => "LIMIT or OFFSET".to_string(),
        NotSupportedFeature::SelectWithNoColumns => "SELECT with no columns".to_string(),
        NotSupportedFeature::SetOperation(operation) => operation.to_string(),
        NotSupportedFeature::DistinctOn => "DISTINCT ON".to_string(),
        NotSupportedFeature::SelectListSubscriptOrField => {
            "a column subscript or field in the select list".to_string()
        }
        NotSupportedFeature::ParenthesizedJoinRhs => {
            "a parenthesized join as the right side of a JOIN".to_string()
        }
        NotSupportedFeature::JoinOperandNotTable => {
            "a JOIN operand other than one table".to_string()
        }
        NotSupportedFeature::NaturalJoin => "NATURAL JOIN".to_string(),
        NotSupportedFeature::AliasedJoin => "an alias on a joined table".to_string(),
        NotSupportedFeature::JoinUsing => "JOIN ... USING".to_string(),
        NotSupportedFeature::RightJoin => "RIGHT JOIN".to_string(),
        NotSupportedFeature::FullJoin => "FULL JOIN".to_string(),
        NotSupportedFeature::ThisJoinType => "this join type".to_string(),
        NotSupportedFeature::JoinWithoutOn => "JOIN without ON".to_string(),
        NotSupportedFeature::OrderByUsing => "ORDER BY USING".to_string(),
        NotSupportedFeature::ColumnAliasesOnTableReference => {
            "column aliases on a table reference".to_string()
        }
        NotSupportedFeature::Lateral => "LATERAL".to_string(),
        NotSupportedFeature::WithOrdinality => "WITH ORDINALITY".to_string(),
        NotSupportedFeature::ColumnDefinitionList => "a column definition list".to_string(),
        NotSupportedFeature::RowsFromMultipleFunctions => {
            "ROWS FROM with anything other than one function".to_string()
        }
        NotSupportedFeature::ThisTableFunction => "this table function".to_string(),
        NotSupportedFeature::AggregateOrderByWithAnotherModifier => {
            "aggregate ORDER BY with another aggregate modifier".to_string()
        }
        NotSupportedFeature::AggregateOrderByWithQueryOrderBy => {
            "aggregate ORDER BY in a query that itself has ORDER BY".to_string()
        }
        NotSupportedFeature::AggregateOrderByNotArrayAgg => {
            "ORDER BY in a call to an aggregate other than array_agg".to_string()
        }
        NotSupportedFeature::ArrayAggOrderByMultipleArgs => {
            "array_agg with ORDER BY and anything other than one argument".to_string()
        }
        NotSupportedFeature::CreateTableClause(clause) => format!("CREATE TABLE with {clause}"),
        NotSupportedFeature::TemporaryOrUnloggedTables => {
            "TEMPORARY or UNLOGGED tables".to_string()
        }
        NotSupportedFeature::CreateTableConstraints => {
            "CREATE TABLE with table constraints".to_string()
        }
        NotSupportedFeature::TableWithNoColumns => "a table with no columns".to_string(),
        NotSupportedFeature::ColumnClause(clause) => format!("a column with {clause}"),
        NotSupportedFeature::ColumnConstraintKind(kind) => kind.to_string(),
        NotSupportedFeature::ThisColumnConstraint => "this column constraint".to_string(),
        NotSupportedFeature::CreateUserAndCreateGroup => "CREATE USER and CREATE GROUP".to_string(),
        NotSupportedFeature::RoleOption(name) => format!("the role option {name}"),
        NotSupportedFeature::GrantClause(clause) => clause.to_string(),
        NotSupportedFeature::ColumnPrivileges => "column privileges".to_string(),
        NotSupportedFeature::GrantToPseudoRole => {
            "GRANT to CURRENT_ROLE, CURRENT_USER or SESSION_USER".to_string()
        }
        NotSupportedFeature::GrantOnUnsupportedObjectType => {
            "GRANT on objects other than tables and schemas".to_string()
        }
        NotSupportedFeature::PolicyCommandNotSelect => {
            "CREATE POLICY for commands other than SELECT, including an omitted FOR".to_string()
        }
        NotSupportedFeature::PolicyRestrictive => "AS RESTRICTIVE".to_string(),
        NotSupportedFeature::PolicyWithoutUsing => "CREATE POLICY without USING".to_string(),
        NotSupportedFeature::PolicyForPseudoRole => {
            "policies for CURRENT_ROLE, CURRENT_USER or SESSION_USER".to_string()
        }
        NotSupportedFeature::PublicCombinedWithOtherRoles => {
            "PUBLIC combined with other roles".to_string()
        }
        NotSupportedFeature::AlterOnUnsupportedObjectType => {
            "ALTER on objects other than tables".to_string()
        }
        NotSupportedFeature::AlterTableIfExists => "ALTER TABLE IF EXISTS".to_string(),
        NotSupportedFeature::AlterTableSubcommand => {
            "ALTER TABLE subcommands other than ROW LEVEL SECURITY".to_string()
        }
        NotSupportedFeature::MultipleStatements => {
            "anything other than exactly one statement per call".to_string()
        }
        NotSupportedFeature::UnicodeEscapeIdentifiers => {
            "Unicode escape identifiers (U&\"...\")".to_string()
        }
        NotSupportedFeature::Node(description) => description,
        NotSupportedFeature::CatalogQualifiedRelationName => {
            "a catalog-qualified relation name".to_string()
        }
        NotSupportedFeature::UnsupportedSchema => {
            "schemas other than public and pg_catalog".to_string()
        }
        NotSupportedFeature::TableAliases => "table aliases".to_string(),
        NotSupportedFeature::ParameterNumberOutOfRange => {
            "a parameter number outside 1..=4294967295".to_string()
        }
        NotSupportedFeature::PositionalParameterZero => "a positional parameter (?)".to_string(),
        NotSupportedFeature::ParameterWithoutDeclaredType(number) => {
            format!("parameter ${number} without a declared type")
        }
        NotSupportedFeature::FunctionCallClause(clause) => clause.to_string(),
        NotSupportedFeature::NamedFunctionArguments => "named function arguments".to_string(),
        NotSupportedFeature::SchemaQualifiedFunctionCall => {
            "schema-qualified function calls outside pg_catalog".to_string()
        }
        NotSupportedFeature::StarMixedWithOtherSelectItems => {
            "* mixed with other select list entries".to_string()
        }
        NotSupportedFeature::QualifiedColumnReferences => "qualified column references".to_string(),
        NotSupportedFeature::NonIntegerNumericLiteral => {
            "numeric literals other than integers".to_string()
        }
        NotSupportedFeature::BitStringLiterals => "bit string literals".to_string(),
        NotSupportedFeature::ColumnType(joined) => format!("column type {joined}"),
        NotSupportedFeature::TypeModifiers => "type modifiers".to_string(),
        NotSupportedFeature::ArraySetofPctType => "array, SETOF and %TYPE column types".to_string(),
        NotSupportedFeature::AnyWithOperator(operator) => {
            format!("ANY combined with the operator {operator}")
        }
        NotSupportedFeature::ChainedSubscript => "a chained or multi-level subscript".to_string(),
        NotSupportedFeature::FieldAccessOnValue => "field access on a value".to_string(),
        NotSupportedFeature::ArraySlice => "an array slice".to_string(),
        NotSupportedFeature::Operator(operator) => format!("the operator {operator}"),
        NotSupportedFeature::ThisExpression => "this expression".to_string(),
        NotSupportedFeature::SchemaQualifiedOperators => "schema-qualified operators".to_string(),
        NotSupportedFeature::UnsupportedSubqueryForm => {
            "subquery forms other than a scalar subquery, EXISTS or IN".to_string()
        }
        NotSupportedFeature::SubqueryOperatorNotIn => {
            "a subquery combined with an operator other than IN".to_string()
        }
        NotSupportedFeature::ArrayLiteralNotBraceDelimited => {
            "an array literal that is not brace-delimited".to_string()
        }
        NotSupportedFeature::ArrayLiteralElementQuotedOrNested => {
            "a quoted, escaped or nested array literal element".to_string()
        }
        NotSupportedFeature::AclDefaultEmptyObjectType => {
            "acldefault of an empty object type".to_string()
        }
        NotSupportedFeature::AclDefaultObjectType => "acldefault of this object type".to_string(),
        NotSupportedFeature::ArrayOfType(ty) => {
            format!("ARRAY(SELECT ...) of {}", ty.display_name())
        }
        NotSupportedFeature::RegprocCastFromName => "a regproc cast from a name".to_string(),
        NotSupportedFeature::SubqueryMultipleColumns => {
            "a subquery with more than one column".to_string()
        }
        NotSupportedFeature::UnionOrderByExpression => {
            "an ORDER BY expression on a UNION other than a result column name or position"
                .to_string()
        }
        NotSupportedFeature::StarWithoutFromClause => {
            "SELECT * with no tables specified".to_string()
        }
        NotSupportedFeature::SubscriptOnType(ty) => format!("a subscript on {}", ty.display_name()),
        NotSupportedFeature::AnyOverType(ty) => format!("ANY over {}", ty.display_name()),
        NotSupportedFeature::ArrayOfElementType(ty) => format!("an array of {}", ty.display_name()),
        NotSupportedFeature::ArrayTypesCannotBeMatched(left, right) => format!(
            "ARRAY types {} and {} cannot be matched",
            left.display_name(),
            right.display_name()
        ),
        NotSupportedFeature::NonConstantArgumentToSetReturningFunction(name) => {
            format!("a non-constant argument to {}", name.as_str())
        }
        NotSupportedFeature::ArgumentNotColumnOrConstant(name) => format!(
            "an argument to {} that is not a column or a constant",
            name.as_str()
        ),
        NotSupportedFeature::InPolicyExpression(form) => {
            format!("{} in a policy's USING expression", form.phrase())
        }
        NotSupportedFeature::ArrayAggOverThisType => "array_agg over this type".to_string(),
        NotSupportedFeature::ExpressionNotLiteralOrCast => {
            "an expression other than a literal or a cast of a literal".to_string()
        }
        NotSupportedFeature::ArrayValueInContext => "an array value in this context".to_string(),
        NotSupportedFeature::NonConstantArgumentOutsideSelectItem { function } => {
            format!("a non-constant argument to {function} outside a top-level select item")
        }
        NotSupportedFeature::FunctionInPosition { function, position } => {
            format!("{function} in {position}")
        }
        NotSupportedFeature::SubqueryInPosition(position) => format!("a subquery in {position}"),
        NotSupportedFeature::AggregateInPosition(position) => {
            format!("an aggregate function in {position}")
        }
        NotSupportedFeature::SetReturningInPosition(position) => {
            format!("a set-returning function in {position}")
        }
        NotSupportedFeature::SetVariableTo { name, values } => {
            format!("SET {name} TO {}", values.join(", "))
        }
        NotSupportedFeature::NullArgumentToRegisteredFunction => {
            "a null argument to a registered function".to_string()
        }
        NotSupportedFeature::CatalogRelationAction { action, table } => format!(
            "{action} on pg_catalog relation \"{}\"",
            table.render_message().as_message()
        ),
        NotSupportedFeature::OutputValueType(ty) => {
            format!("a value of type {}", ty.display_name())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadError {
    pub state: SqlState,
    pub message: String,
    pub detail: Option<String>,
    /// PostgreSQL's `H` field: a suggestion for fixing the error.
    pub hint: Option<String>,
    /// A node location, not yet converted to PostgreSQL's 1-based character
    /// position. `resolve_position` consumes this into `position`; nothing
    /// outside this module reads it.
    location: Option<Location>,
    /// PostgreSQL's `P` field: a 1-based character position into the
    /// statement text. `None` until `resolve_position` runs, and stays
    /// `None` when `location` was never set (PostgreSQL itself omits `P`
    /// for most errors; the head only attaches `location` where a live
    /// PostgreSQL 18 was probed and shown to report one).
    pub position: Option<u32>,
}

impl HeadError {
    /// The one place a `PgError` becomes a `HeadError`: renders it through
    /// the one table (`render`) and takes on its state, message, detail and
    /// hint. Every raise site outside this module reaches a `HeadError`
    /// only through this, `not_supported` or `internal` below.
    pub(crate) fn raise(kind: PgError) -> Self {
        let Rendered {
            state,
            message,
            detail,
            hint,
        } = render(kind);
        HeadError {
            state,
            message,
            detail,
            hint,
            location: None,
            position: None,
        }
    }

    pub(crate) fn not_supported(feature: NotSupportedFeature) -> Self {
        HeadError::raise(PgError::NotSupported(feature))
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        HeadError::raise(PgError::Internal(message.into()))
    }

    pub(crate) fn syntax(message: impl Into<String>) -> Self {
        HeadError::raise(PgError::Syntax(message.into()))
    }

    /// Attaches the position of the parse-tree node this error is about, at
    /// exactly the raise sites where a live PostgreSQL 18 was probed and
    /// shown to report one, taken as-is from the parsed name or expression
    /// the error belongs to, never threaded in ad hoc. Accepts a bare
    /// `Location` or an `Option<Location>` alike, so a raise site holding
    /// either never has to wrap one to call this.
    pub(crate) fn at(mut self, location: impl Into<Option<Location>>) -> Self {
        self.location = location.into();
        self
    }

    /// Converts a pending byte-offset `location` into PostgreSQL's 1-based
    /// character position, against the statement text the location was
    /// measured against. Called exactly once, at the boundary
    /// (`Session::execute`) that is the last place still holding that text.
    pub(crate) fn resolve_position(mut self, sql: &str) -> Self {
        if let Some(location) = self.location.take() {
            self.position = char_position(sql, location);
        }
        self
    }
}

/// PostgreSQL's `P` field is a character index, not a byte index: a 1-based
/// count of characters preceding the located byte offset. A `Location`
/// only ever points at a token boundary, so it is always a valid `str`
/// boundary here.
fn char_position(sql: &str, location: Location) -> Option<u32> {
    let prefix = sql.get(..location.offset())?;
    u32::try_from(prefix.chars().count() + 1).ok()
}

impl fmt::Display for HeadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.state.code(), self.message)
    }
}

impl std::error::Error for HeadError {}

impl From<LimboError> for HeadError {
    fn from(error: LimboError) -> Self {
        HeadError::internal(error.to_string())
    }
}
