//! The one row-security decision (`ARCH.md`'s "Row security"): the
//! exemption rule and policy applicability are read nowhere else. `decide`
//! is the only constructor of [`RowSecurityDecision`], whose fields are
//! private, so a scan or a write against a user table can only read one by
//! holding a value this function minted.

use crate::analyze::typing::{self, RelationSlot, Typed};
use crate::catalog::{Role, Table};
use crate::error::HeadError;

/// What a user-table reference needs a decision for: the only two kinds of
/// access this head admits (`ARCH.md`'s scope list has no `UPDATE` or
/// `DELETE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Read,
    Insert,
}

/// A stored policy's command, as `CREATE POLICY ... FOR <command>` parses
/// it (`postgres_deparse.c` matches the same five words back the other
/// way). `parse::ddl::create_policy` refuses creating anything but
/// `Select`, so every policy `decide` ever reads is one; `decide` still
/// matches every variant, so admitting a new command needs this match
/// updated before it can silently start applying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PolicyCommand {
    All,
    Select,
    Insert,
    Update,
    Delete,
}

impl PolicyCommand {
    pub(crate) fn from_parse_tree(cmd_name: &str) -> Result<Self, HeadError> {
        match cmd_name {
            "all" => Ok(PolicyCommand::All),
            "select" => Ok(PolicyCommand::Select),
            "insert" => Ok(PolicyCommand::Insert),
            "update" => Ok(PolicyCommand::Update),
            "delete" => Ok(PolicyCommand::Delete),
            _ => Err(HeadError::internal(
                "CREATE POLICY with a command name outside PostgreSQL's ALL/SELECT/INSERT/UPDATE/DELETE set",
            )),
        }
    }

    /// PostgreSQL's own `pg_policy.polcmd` encoding (`policy.c`'s
    /// `CreatePolicy`, reusing the ACL privilege characters): `*` all,
    /// `r` select, `a` insert, `w` update, `d` delete.
    pub(crate) fn code(self) -> char {
        match self {
            PolicyCommand::All => '*',
            PolicyCommand::Select => 'r',
            PolicyCommand::Insert => 'a',
            PolicyCommand::Update => 'w',
            PolicyCommand::Delete => 'd',
        }
    }

    /// The inverse of `code`: refuses any byte that is not one of
    /// PostgreSQL's five, so a `pg_policy` row written by anything else
    /// fails closed rather than `decide` silently treating it as some
    /// other command.
    pub(crate) fn from_code(code: char) -> Result<Self, HeadError> {
        match code {
            '*' => Ok(PolicyCommand::All),
            'r' => Ok(PolicyCommand::Select),
            'a' => Ok(PolicyCommand::Insert),
            'w' => Ok(PolicyCommand::Update),
            'd' => Ok(PolicyCommand::Delete),
            _ => Err(HeadError::internal(
                "pg_policy.polcmd is not one of PostgreSQL's */r/a/w/d codes",
            )),
        }
    }
}

/// A policy's stored `USING` predicate: the field is readable only inside
/// `security` (`enforcement::Stored::into_predicate` is the only
/// constructor), so a future `WITH CHECK` predicate, stored the same way,
/// is read only by `decide`, never by `catalog`, `analyze` or `lower`
/// reaching into a `Policy` directly.
pub(crate) struct PolicyPredicate {
    pub(super) typed: Typed,
}

#[derive(Debug, Clone)]
enum Decision {
    Exempt,
    Refused,
    Enforced(Typed),
}

/// The outcome of [`decide`] for one reference to a user table: private
/// fields, so nothing outside this function can mint one, reshape one into
/// an admission it never earned, or read a policy's predicate except
/// through the accessors below.
#[derive(Debug, Clone)]
pub(crate) struct RowSecurityDecision(Decision);

impl RowSecurityDecision {
    pub(crate) fn is_refused(&self) -> bool {
        matches!(self.0, Decision::Refused)
    }

    /// Moves an enforced read's predicate from `RelationSlot::SELF` to the
    /// slot it was resolved against: the only reshape a minted decision
    /// ever gets, kept here so `analyze::walk` cannot build an `Enforced`
    /// decision of its own.
    pub(crate) fn retargeted(self, to: RelationSlot) -> Self {
        match self.0 {
            Decision::Enforced(predicate) => {
                RowSecurityDecision(Decision::Enforced(typing::retarget(&predicate, to)))
            }
            other @ Decision::Exempt | other @ Decision::Refused => RowSecurityDecision(other),
        }
    }

    /// Folds an enforced read's predicate through `f` (session-function
    /// evaluation, constant folding): the only way `session::session_functions`
    /// can change a minted decision, since it cannot match `Decision` itself.
    pub(crate) fn map_predicate(
        self,
        f: impl FnOnce(Typed) -> Result<Typed, HeadError>,
    ) -> Result<Self, HeadError> {
        match self.0 {
            Decision::Enforced(predicate) => {
                Ok(RowSecurityDecision(Decision::Enforced(f(predicate)?)))
            }
            other @ Decision::Exempt | other @ Decision::Refused => Ok(RowSecurityDecision(other)),
        }
    }

    /// What `lower::query` may scan a user-table reference with: `None`
    /// for an exempt reference, the OR'd applicable policy predicate
    /// otherwise. `Refused` reaching here is this head's own bug:
    /// `pipeline::authorize` must raise it first.
    pub(crate) fn scan_predicate(&self) -> Result<Option<&Typed>, HeadError> {
        match &self.0 {
            Decision::Exempt => Ok(None),
            Decision::Enforced(predicate) => Ok(Some(predicate)),
            Decision::Refused => Err(HeadError::internal(
                "a relation reached lowering with row security refused; authorize must raise this first",
            )),
        }
    }

    /// Whether an insert may reach the engine: `false` for an enforced
    /// decision, since no `INSERT` policy is ever admissible (`decide`'s
    /// own exhaustive match), which the insert path turns into
    /// PostgreSQL's row-security violation. `Refused` reaching here is
    /// this head's own bug: `pipeline::authorize` must raise it first.
    pub(crate) fn admits_insert(&self) -> Result<bool, HeadError> {
        match self.0 {
            Decision::Exempt => Ok(true),
            Decision::Enforced(_) => Ok(false),
            Decision::Refused => Err(HeadError::internal(
                "an insert reached enforcement with row security refused; authorize must raise this first",
            )),
        }
    }
}

/// The one place PostgreSQL's row-security exemption rule and a table's
/// policies are read: `!rls_enabled || superuser || (owner && !forced)`
/// exempts outright; otherwise `row_security_setting` (PostgreSQL's
/// `row_security` GUC) must be on, and the applicable policies for
/// `access` decide the rest.
pub(crate) fn decide(
    table: &Table,
    role: Role,
    access: Access,
    row_security_setting: bool,
) -> Result<RowSecurityDecision, HeadError> {
    if !table.rls_enabled || role.superuser || (table.owner == role.oid && !table.rls_forced) {
        return Ok(RowSecurityDecision(Decision::Exempt));
    }
    if !row_security_setting {
        return Ok(RowSecurityDecision(Decision::Refused));
    }
    let mut applicable = Vec::new();
    for policy in &table.policies {
        if !Table::applies_to(policy, role.oid) {
            continue;
        }
        match (access, policy.command) {
            (Access::Read, PolicyCommand::Select) => applicable.push(policy.using.typed.clone()),
            // PostgreSQL's own rule: a `SELECT` policy never admits an
            // `INSERT`.
            (Access::Insert, PolicyCommand::Select) => {}
            // `parse::ddl::create_policy` refuses creating any of these;
            // fail closed rather than decide what a command this head
            // cannot yet store should do.
            (_, PolicyCommand::All)
            | (_, PolicyCommand::Insert)
            | (_, PolicyCommand::Update)
            | (_, PolicyCommand::Delete) => {
                return Err(HeadError::internal("policy command not admitted"));
            }
        }
    }
    let predicate = match applicable.len() {
        0 => Typed::AlwaysFalse,
        1 => applicable.remove(0),
        _ => Typed::Or(applicable),
    };
    Ok(RowSecurityDecision(Decision::Enforced(predicate)))
}
