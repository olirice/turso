//! `Statement::rules()` is the one table every statement kind's admission
//! and command-tag rules are read from (`admit`, `pipeline.rs`), modeled on
//! `analyze/typing/context.rs`'s expression gate: an exhaustive match with a
//! full struct literal per statement kind, so a new kind cannot compile
//! until it states every rule.
#![deny(clippy::wildcard_enum_match_arm)]

use std::fmt;

use crate::analyze::types::TypeHandle;
use crate::ident::{
    ColumnName, FunctionName, PolicyName, PreparedName, RoleName, SchemaName, TableName,
};
use crate::parse::expr::Expr;
use crate::parse::Location;
use crate::security::privileges::PrivilegeKeyword;
use crate::session::settings::{KnownSetting, SettingValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelationSchema {
    Unqualified,
    Public,
    PgCatalog,
}

#[derive(Debug, Clone)]
pub(crate) struct RelationName {
    pub(crate) name: TableName,
    pub(crate) schema: RelationSchema,
    /// The relation reference's own location, or `None` when it was not
    /// parsed from a `RangeVar` (a text relation name, admitted through
    /// `qualified_name::relation_name`). Only a lookup PostgreSQL 18 itself
    /// resolves through its query analyzer (a `SELECT`/`INSERT` reference)
    /// uses this to attach a position; a utility-command lookup (`ALTER
    /// TABLE`, `GRANT`, `LOCK`, `CREATE POLICY`) never does.
    pub(crate) location: Option<Location>,
}

#[derive(Debug)]
pub(crate) enum Statement {
    CreateTable {
        relation: RelationName,
        columns: Vec<ColumnDef>,
    },
    Insert {
        table: RelationName,
        /// Each target column's own `ResTarget` location, carried so an
        /// undefined-column or too-many-targets error can point at it the
        /// way PostgreSQL 18 does.
        columns: Option<Vec<(ColumnName, Option<Location>)>>,
        rows: Vec<Vec<Expr>>,
    },
    Select(Query),
    CreateRole {
        name: RoleName,
        can_login: bool,
    },
    Grant {
        privileges: GrantedPrivileges,
        objects: GrantObjects,
        grantees: Vec<GranteeName>,
    },
    CreatePolicy {
        name: PolicyName,
        table: RelationName,
        roles: Vec<GranteeName>,
        using: Expr,
    },
    AlterRowSecurity {
        table: RelationName,
        changes: Vec<RowSecurityChange>,
    },
    Lock {
        tables: Vec<RelationName>,
    },
    Begin {
        read_only: Option<bool>,
    },
    Commit,
    Rollback,
    SetTransaction {
        sets_isolation: bool,
        read_only: Option<bool>,
    },
    Set(SetStatement),
    Prepare {
        name: PreparedName,
        param_types: Vec<TypeHandle>,
        query: Query,
    },
    Execute {
        name: PreparedName,
        args: Vec<Expr>,
    },
}

#[derive(Debug)]
pub(crate) struct SetStatement {
    pub(crate) target: KnownSetting,
    pub(crate) value: SettingValue,
    pub(crate) local: bool,
}

/// PostgreSQL's own command tag for a statement kind: the `CommandComplete`
/// name (`INSERT`, `SELECT`, ...) and the name a "cannot execute X in a
/// read-only transaction" message shows. `name` is the one place either of
/// those texts is spelled; nothing else builds this text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTag {
    CreateTable,
    Insert,
    Select,
    CreateRole,
    Grant,
    CreatePolicy,
    AlterTable,
    Lock,
    Begin,
    Commit,
    Rollback,
    Set,
    Prepare,
}

impl CommandTag {
    pub fn name(self) -> &'static str {
        match self {
            CommandTag::CreateTable => "CREATE TABLE",
            CommandTag::Insert => "INSERT",
            CommandTag::Select => "SELECT",
            CommandTag::CreateRole => "CREATE ROLE",
            CommandTag::Grant => "GRANT",
            CommandTag::CreatePolicy => "CREATE POLICY",
            CommandTag::AlterTable => "ALTER TABLE",
            CommandTag::Lock => "LOCK TABLE",
            CommandTag::Begin => "BEGIN",
            CommandTag::Commit => "COMMIT",
            CommandTag::Rollback => "ROLLBACK",
            CommandTag::Set => "SET",
            CommandTag::Prepare => "PREPARE",
        }
    }
}

impl fmt::Display for CommandTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One statement kind's admission rules, read only through
/// [`Statement::rules`]. Every arm is a full struct literal (no `Default`,
/// no `..`), so a new statement kind cannot compile until it states every
/// rule.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StatementRules {
    /// The `CommandComplete` tag this statement completes with, and the
    /// name a read-only-transaction refusal shows.
    pub(crate) tag: CommandTag,
    /// Whether this statement needs the engine's write lock (`ARCH.md`'s
    /// "Locking": an immediate transaction rather than a deferred one).
    /// `LOCK TABLE` needs it to serialize against concurrent writers but,
    /// unlike every other statement here that does, is not itself refused
    /// in a read-only transaction (probed live: `pg_dump`'s own `LOCK
    /// TABLE ... IN ACCESS SHARE MODE` runs inside one), so this is a
    /// separate rule from `refused_when_read_only`.
    pub(crate) engine_write_lock: bool,
    /// Whether PostgreSQL refuses this statement inside a read-only
    /// transaction ("cannot execute X in a read-only transaction").
    pub(crate) refused_when_read_only: bool,
    /// Whether this statement is one of the two PostgreSQL still runs once
    /// the transaction has failed (`COMMIT`, `ROLLBACK`); every other
    /// statement is refused with "current transaction is aborted".
    pub(crate) runs_when_aborted: bool,
    /// Whether running this statement fixes the transaction's snapshot, so
    /// a later `SET TRANSACTION` that would move it is refused.
    pub(crate) takes_snapshot: bool,
}

impl Statement {
    /// This statement's rules, exhaustive over every kind (no wildcard arm,
    /// enforced by `#![deny(clippy::wildcard_enum_match_arm)]` above).
    pub(crate) fn rules(&self) -> StatementRules {
        match self {
            Statement::CreateTable { .. } => StatementRules {
                tag: CommandTag::CreateTable,
                engine_write_lock: true,
                refused_when_read_only: true,
                runs_when_aborted: false,
                takes_snapshot: true,
            },
            Statement::Insert { .. } => StatementRules {
                tag: CommandTag::Insert,
                engine_write_lock: true,
                refused_when_read_only: true,
                runs_when_aborted: false,
                takes_snapshot: true,
            },
            Statement::Select(_) => StatementRules {
                tag: CommandTag::Select,
                engine_write_lock: false,
                refused_when_read_only: false,
                runs_when_aborted: false,
                takes_snapshot: true,
            },
            Statement::CreateRole { .. } => StatementRules {
                tag: CommandTag::CreateRole,
                engine_write_lock: true,
                refused_when_read_only: true,
                runs_when_aborted: false,
                takes_snapshot: true,
            },
            Statement::Grant { .. } => StatementRules {
                tag: CommandTag::Grant,
                engine_write_lock: true,
                refused_when_read_only: true,
                runs_when_aborted: false,
                takes_snapshot: true,
            },
            Statement::CreatePolicy { .. } => StatementRules {
                tag: CommandTag::CreatePolicy,
                engine_write_lock: true,
                refused_when_read_only: true,
                runs_when_aborted: false,
                takes_snapshot: true,
            },
            Statement::AlterRowSecurity { .. } => StatementRules {
                tag: CommandTag::AlterTable,
                engine_write_lock: true,
                refused_when_read_only: true,
                runs_when_aborted: false,
                takes_snapshot: true,
            },
            Statement::Lock { .. } => StatementRules {
                tag: CommandTag::Lock,
                engine_write_lock: true,
                refused_when_read_only: false,
                runs_when_aborted: false,
                takes_snapshot: false,
            },
            Statement::Begin { .. } => StatementRules {
                tag: CommandTag::Begin,
                engine_write_lock: false,
                refused_when_read_only: false,
                runs_when_aborted: false,
                takes_snapshot: false,
            },
            Statement::Commit => StatementRules {
                tag: CommandTag::Commit,
                engine_write_lock: false,
                refused_when_read_only: false,
                runs_when_aborted: true,
                takes_snapshot: false,
            },
            Statement::Rollback => StatementRules {
                tag: CommandTag::Rollback,
                engine_write_lock: false,
                refused_when_read_only: false,
                runs_when_aborted: true,
                takes_snapshot: false,
            },
            Statement::SetTransaction { .. } => StatementRules {
                tag: CommandTag::Set,
                engine_write_lock: false,
                refused_when_read_only: false,
                runs_when_aborted: false,
                takes_snapshot: false,
            },
            Statement::Set(_) => StatementRules {
                tag: CommandTag::Set,
                engine_write_lock: false,
                refused_when_read_only: false,
                runs_when_aborted: false,
                takes_snapshot: false,
            },
            Statement::Prepare { .. } => StatementRules {
                tag: CommandTag::Prepare,
                engine_write_lock: false,
                refused_when_read_only: false,
                runs_when_aborted: false,
                takes_snapshot: false,
            },
            // An `EXECUTE` of a prepared statement runs only ever a stored
            // SELECT (`PrepareNonSelect` refuses every other kind at
            // `PREPARE` time), and completes the way PostgreSQL 18 itself
            // reports it running one: as `SELECT n`, never `EXECUTE`.
            Statement::Execute { .. } => StatementRules {
                tag: CommandTag::Select,
                engine_write_lock: false,
                refused_when_read_only: false,
                runs_when_aborted: false,
                takes_snapshot: true,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RowSecurityChange {
    Enable,
    Disable,
    Force,
    NoForce,
}

#[derive(Debug)]
pub(crate) enum GrantedPrivileges {
    All,
    Named(Vec<PrivilegeKeyword>),
}

#[derive(Debug)]
pub(crate) enum GrantObjects {
    Tables(Vec<RelationName>),
    Schemas(Vec<SchemaName>),
}

#[derive(Debug)]
pub(crate) enum GranteeName {
    Public,
    Role(RoleName),
}

#[derive(Debug, Clone)]
pub(crate) struct Query {
    pub(crate) first: SimpleSelect,
    pub(crate) combined: Vec<SimpleSelect>,
    pub(crate) order_by: Vec<OrderItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct SimpleSelect {
    pub(crate) distinct: bool,
    pub(crate) items: Vec<SelectItem>,
    pub(crate) from: Option<FromClause>,
    pub(crate) filter: Option<Expr>,
}

#[derive(Debug, Clone)]
pub(crate) enum SelectItem {
    Expr(Expr, Option<ColumnName>),
    AllColumns,
    AllColumnsOf(TableName),
}

#[derive(Debug, Clone)]
pub(crate) struct FromClause {
    pub(crate) first: FromItem,
    pub(crate) joins: Vec<Join>,
}

#[derive(Debug, Clone)]
pub(crate) enum FromItem {
    Table {
        relation: RelationName,
        alias: Option<TableName>,
    },
    Derived {
        query: Box<Query>,
        alias: TableName,
        columns: Option<Vec<ColumnName>>,
    },
    Function {
        name: FunctionName,
        args: Vec<Expr>,
        alias: Option<TableName>,
        columns: Option<Vec<ColumnName>>,
        /// The `FuncCall`'s own location: PostgreSQL 18 points a "function
        /// does not exist" error at the call, not the FROM item. Always
        /// present: this variant is built only from a real, parsed
        /// `FuncCall`, whose grammar production always supplies one.
        location: Location,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Join {
    pub(crate) kind: JoinKind,
    pub(crate) item: FromItem,
    pub(crate) on: Option<Expr>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum JoinKind {
    Inner,
    Left,
    Cross,
}

#[derive(Debug, Clone)]
pub(crate) struct OrderItem {
    pub(crate) expr: Expr,
    pub(crate) desc: bool,
    pub(crate) nulls_first: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ColumnDef {
    pub(crate) name: ColumnName,
    pub(crate) ty: TypeHandle,
    /// The `PRIMARY KEY` constraint's own location, or `None` when the
    /// column has none. Carrying the location here (rather than a bare
    /// `bool`) is what lets `check_table_definition` point PostgreSQL's
    /// "multiple primary keys" error at the offending constraint, the way
    /// PostgreSQL 18 itself does.
    pub(crate) primary_key: Option<Location>,
    pub(crate) not_null: bool,
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    fn can_write(sql: &str) -> bool {
        crate::parse::statement(sql)
            .unwrap_or_else(|error| panic!("{sql} must parse: {error}"))
            .rules()
            .engine_write_lock
    }

    #[test]
    fn only_statements_that_reach_the_engine_or_the_catalog_can_write() {
        for sql in [
            "CREATE TABLE notes (id integer PRIMARY KEY)",
            "INSERT INTO notes VALUES (1)",
            "CREATE ROLE bob LOGIN",
            "GRANT SELECT ON notes TO bob",
            "CREATE POLICY p ON notes FOR SELECT USING (true)",
            "ALTER TABLE notes ENABLE ROW LEVEL SECURITY",
            "LOCK TABLE notes IN ACCESS SHARE MODE",
        ] {
            assert!(can_write(sql), "{sql} must take the write lock");
        }
    }

    #[test]
    fn statements_that_only_read_or_change_session_state_never_write() {
        for sql in [
            "SELECT 1",
            "SET search_path = public",
            "PREPARE p AS SELECT 1",
            "EXECUTE p",
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        ] {
            assert!(!can_write(sql), "{sql} must not take the write lock");
        }
    }
}
