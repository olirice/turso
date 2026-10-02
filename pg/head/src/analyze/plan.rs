use crate::analyze::functions::FunctionHandle;
use crate::analyze::typing::{RelationSlot, Typed};
use crate::catalog::{Backing, Oid};
use crate::parse::statement::JoinKind;
use crate::security::row_security::RowSecurityDecision;

#[derive(Debug, Clone)]
pub(crate) struct ResolvedQuery {
    pub(crate) first: ResolvedSimpleSelect,
    pub(crate) combined: Vec<ResolvedSimpleSelect>,
    pub(crate) order_by: Vec<ResolvedOrderItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedOrderItem {
    pub(crate) target: ResolvedOrderTarget,
    pub(crate) desc: bool,
    pub(crate) nulls_first: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedOrderTarget {
    Expr(Typed),
    OutputPosition(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedSimpleSelect {
    pub(crate) distinct: bool,
    pub(crate) output: Vec<Typed>,
    pub(crate) from: Option<ResolvedFrom>,
    pub(crate) filter: Option<Typed>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedFrom {
    pub(crate) first: ResolvedFromItem,
    pub(crate) joins: Vec<ResolvedJoin>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedJoin {
    pub(crate) kind: JoinKind,
    pub(crate) item: ResolvedFromItem,
    pub(crate) on: Option<Typed>,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedFromItem {
    Relation {
        slot: RelationSlot,
        oid: Oid,
        backing: Backing,
        security: RowSecurityDecision,
    },
    Derived {
        slot: RelationSlot,
        query: Box<ResolvedQuery>,
    },
    Function {
        slot: RelationSlot,
        handle: FunctionHandle,
        args: Vec<Typed>,
    },
}

impl ResolvedFromItem {
    pub(crate) fn slot(&self) -> RelationSlot {
        match self {
            ResolvedFromItem::Relation { slot, .. }
            | ResolvedFromItem::Derived { slot, .. }
            | ResolvedFromItem::Function { slot, .. } => *slot,
        }
    }
}
