use pg_query::protobuf::{node::Node as PgNode, Node, RangeVar, Token};

use crate::error::{HeadError, NotSupportedFeature};
use crate::ident::TableName;
use crate::parse::statement::{Query, RelationName, RelationSchema, SimpleSelect, Statement};

mod ddl;
pub(crate) mod expr;
pub(crate) mod qualified_name;
mod query;
mod session;
pub(crate) mod statement;
pub(crate) mod walk;

const MAX_IDENTIFIER_BYTES: usize = 63;

/// A pg_query node location (a byte offset into the parsed statement text).
/// `from_raw`, its only constructor, is private to this module and the
/// parse submodules that read pg_query's structs directly, so a `Location`
/// past that edge is always a real offset, never pg_query's own `-1`
/// sentinel; a node this crate synthesizes rather than parses carries
/// `None` by never calling `from_raw` at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Location(u32);

impl Location {
    /// The parse edge: a raw pg_query location becomes a real offset, or
    /// `None` for pg_query's negative "no location" sentinel.
    fn from_raw(raw: i32) -> Option<Location> {
        u32::try_from(raw).ok().map(Location)
    }

    /// Read only by `HeadError::resolve_position`, the one place a
    /// location becomes PostgreSQL's `P` field.
    pub(crate) fn offset(self) -> usize {
        usize::try_from(self.0).unwrap_or(usize::MAX)
    }

    /// A location for a test fixture with no statement text to parse one
    /// from.
    #[cfg(test)]
    pub(crate) fn for_test(offset: u32) -> Location {
        Location(offset)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FromParser(());

const PROOF: FromParser = FromParser(());

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "pg_query's Node oneof enumerates every node kind PostgreSQL's parser can produce; this refuses every kind not explicitly admitted here, never silently accepting one"
)]
pub(crate) fn statement(sql: &str) -> Result<Statement, HeadError> {
    let parsed = pg_query::parse(sql).map_err(syntax_error)?;
    refuse_identifiers_postgres_would_truncate(sql)?;
    let [raw] = parsed.protobuf.stmts.as_slice() else {
        return Err(HeadError::not_supported(
            NotSupportedFeature::MultipleStatements,
        ));
    };
    match node(raw.stmt.as_deref())? {
        PgNode::CreateStmt(create) => ddl::create_table(create),
        PgNode::InsertStmt(insert) => query::insert_rows(insert),
        PgNode::SelectStmt(select) => query::query(select).map(Statement::Select),
        PgNode::CreateRoleStmt(create) => ddl::create_role(create),
        PgNode::GrantStmt(grant) => ddl::grant_privileges(grant),
        PgNode::CreatePolicyStmt(create) => ddl::create_policy(create),
        PgNode::AlterTableStmt(alter) => ddl::alter_row_security(alter),
        PgNode::LockStmt(lock) => session::lock_tables(lock),
        PgNode::TransactionStmt(transaction) => session::transaction_control(transaction),
        PgNode::VariableSetStmt(set) => session::set_variable(set),
        PgNode::PrepareStmt(prepare) => session::prepare_statement(prepare),
        PgNode::ExecuteStmt(execute) => session::execute_statement(execute),
        other => Err(unsupported_node(other, "this statement")),
    }
}

/// The one entry a stored expression (today, only a policy's `USING` text)
/// re-enters through: `statement`, the same client SQL uses, wrapped so a
/// bare boolean expression parses as one. A stored expression can
/// therefore never see a grammar `statement` itself does not.
pub(crate) fn stored_expression(sql: &str) -> Result<expr::Expr, HeadError> {
    match statement(&format!("SELECT 1 WHERE {sql}"))? {
        Statement::Select(Query {
            first:
                SimpleSelect {
                    filter: Some(filter),
                    ..
                },
            ..
        }) => Ok(filter),
        Statement::CreateTable { .. }
        | Statement::Insert { .. }
        | Statement::Select(_)
        | Statement::CreateRole { .. }
        | Statement::Grant { .. }
        | Statement::CreatePolicy { .. }
        | Statement::AlterRowSecurity { .. }
        | Statement::Lock { .. }
        | Statement::Begin { .. }
        | Statement::Commit
        | Statement::Rollback
        | Statement::SetTransaction { .. }
        | Statement::Set(_)
        | Statement::Prepare { .. }
        | Statement::Execute { .. } => Err(HeadError::internal(
            "a stored policy expression did not parse to a SELECT with a WHERE clause",
        )),
    }
}

fn refuse_identifiers_postgres_would_truncate(sql: &str) -> Result<(), HeadError> {
    let scanned = pg_query::scan(sql).map_err(syntax_error)?;
    if scanned
        .tokens
        .iter()
        .any(|token| matches!(Token::try_from(token.token), Ok(Token::Uident)))
    {
        return Err(HeadError::not_supported(
            NotSupportedFeature::UnicodeEscapeIdentifiers,
        ));
    }
    for token in scanned
        .tokens
        .iter()
        .filter(|token| matches!(Token::try_from(token.token), Ok(Token::Ident)))
    {
        let text = usize::try_from(token.start)
            .ok()
            .zip(usize::try_from(token.end).ok())
            .and_then(|(start, end)| sql.get(start..end))
            .ok_or_else(|| HeadError::internal("a scanned token lies outside the statement"))?;
        let length = match text
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
        {
            Some(quoted) => quoted.replace("\"\"", "\"").len(),
            None => text.len(),
        };
        if length > MAX_IDENTIFIER_BYTES {
            return Err(HeadError::not_supported(
                NotSupportedFeature::IdentifierTooLong,
            ));
        }
    }
    Ok(())
}

fn syntax_error(error: pg_query::Error) -> HeadError {
    match error {
        pg_query::Error::Parse(message) => HeadError::syntax(message),
        other @ pg_query::Error::Conversion(_)
        | other @ pg_query::Error::Decode(_)
        | other @ pg_query::Error::InvalidJson(_)
        | other @ pg_query::Error::InvalidPointer
        | other @ pg_query::Error::Scan(_)
        | other @ pg_query::Error::Split(_) => HeadError::syntax(other.to_string()),
    }
}

pub(super) fn node(node: Option<&Node>) -> Result<&PgNode, HeadError> {
    node.and_then(|node| node.node.as_ref())
        .ok_or_else(|| HeadError::internal("the parser returned an empty node"))
}

pub(super) fn unsupported_node(node: &PgNode, fallback: &str) -> HeadError {
    match node.deparse() {
        Ok(sql) => HeadError::not_supported(NotSupportedFeature::Node(sql)),
        Err(_) => HeadError::not_supported(NotSupportedFeature::Node(fallback.to_string())),
    }
}

pub(super) fn bare_relation_name(relation: &RangeVar) -> Result<RelationName, HeadError> {
    let RangeVar {
        catalogname,
        schemaname,
        relname,
        // `ONLY` (excluding descendant tables from the scan): always a
        // no-op here, since nothing this head creates (`create_table`
        // refuses `INHERITS` and every partitioning clause) ever has a
        // descendant, so admitting or refusing `ONLY` reads the same rows
        // either way.
        inh: _,
        // Only meaningful on the relation a `CREATE TABLE` itself defines;
        // `ddl::create_table` reads this same field straight off the
        // `CreateStmt`'s own `relation` before this function ever sees it,
        // and every other caller's `RangeVar` (a reference, not a
        // definition) always carries `"p"`.
        relpersistence: _,
        // Read by `table_name` (from its own `relation`, not through this
        // destructure) for callers where an alias is never valid; left to
        // the caller here because a `FROM`-item reference admits one
        // (`from_item_table`).
        alias: _,
        location,
    } = relation;
    if !catalogname.is_empty() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::CatalogQualifiedRelationName,
        ));
    }
    let schema = match schemaname.as_str() {
        "" => RelationSchema::Unqualified,
        "public" => RelationSchema::Public,
        "pg_catalog" => RelationSchema::PgCatalog,
        _ => {
            return Err(HeadError::not_supported(
                NotSupportedFeature::UnsupportedSchema,
            ))
        }
    };
    Ok(RelationName {
        name: TableName::from_parse_tree(PROOF, relname.clone())?,
        schema,
        location: Location::from_raw(*location),
    })
}

pub(super) fn table_name(relation: &RangeVar) -> Result<RelationName, HeadError> {
    if relation.alias.is_some() {
        return Err(HeadError::not_supported(NotSupportedFeature::TableAliases));
    }
    bare_relation_name(relation)
}
