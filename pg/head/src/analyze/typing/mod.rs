use crate::analyze::functions::FunctionHandle;
use crate::analyze::types::{TypeHandle, BOOL_OID, NAME_OID, OID_OID, TEXT_OID};
use crate::catalog::{Attnum, Oid};
use crate::error::{HeadError, NotSupportedFeature};
use crate::parse::expr::{CompareOp, EmptyArrayResult, Truth};
use crate::parse::Location;

mod aggregate;
mod check;
mod coerce;
mod context;
mod scope;
mod walk;

pub(crate) use aggregate::{contains_aggregate, refuse_ungrouped_columns};
pub(crate) use check::{
    is_session_constant, referenced_columns, refuse_row_computed_in_order_by_reference,
    refuse_row_computed_in_select_item, retarget, type_check, undefined_function,
    zero_based_origin_oid,
};
pub(crate) use coerce::{assign, coerce_constant, parse_integer_text};
pub(crate) use context::{Context, Feature, Position};
pub(crate) use scope::{missing_from_clause_entry, RelationShape, Scope, ScopeRelation};
pub(crate) use walk::{walk_typed_map, TypedMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RelationSlot(pub(crate) usize);

impl RelationSlot {
    pub(crate) const SELF: RelationSlot = RelationSlot(0);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Scalar {
    Integer(i64),
    Text(String),
    Null,
    Array(Vec<Scalar>),
}

#[derive(Debug, Clone)]
pub(crate) enum Typed {
    /// The trailing location is the underlying `Expr::Column`'s own,
    /// filtered through `Context::locate` at classification time, carried
    /// so an ungrouped-column error (raised only after the whole `SELECT`
    /// list has been typed, well past any `Context`) can still point at it
    /// the way PostgreSQL 18 does.
    Column(RelationSlot, Attnum, TypeHandle, Option<Location>),
    TableOid(Oid),
    Value(Scalar, TypeHandle),
    CurrentUser,
    Cast(Box<Typed>, TypeHandle),
    Not(Box<Typed>),
    And(Vec<Typed>),
    Or(Vec<Typed>),
    Compare(CompareOp, Box<Typed>, Box<Typed>),
    IsNull(Box<Typed>, bool),
    Is(Box<Typed>, Truth, bool),
    DistinctFrom(Box<Typed>, Box<Typed>, bool),
    In(Box<Typed>, Vec<Typed>, bool),
    Concat(Box<Typed>, Box<Typed>),
    AlwaysFalse,
    Case {
        base: Option<Box<Typed>>,
        arms: Vec<(Typed, Typed)>,
        otherwise: Option<Box<Typed>>,
        result: TypeHandle,
    },
    Subquery(Box<crate::analyze::plan::ResolvedQuery>),
    Exists(Box<crate::analyze::plan::ResolvedQuery>),
    InSelect(Box<Typed>, Box<crate::analyze::plan::ResolvedQuery>, bool),
    Call(FunctionHandle, Vec<Typed>),
    Subscript(Box<Typed>, Box<Typed>, TypeHandle, bool),
    AnyEq(Box<Typed>, Box<Typed>),
    /// `ARRAY[e1, e2, ...]`: a general expression, always with a fixed
    /// element type unified across every element (`check::array_literal`),
    /// unlike `ArrayAgg`'s subquery-shaped `ARRAY(SELECT ...)`.
    ArrayLiteral(Vec<Typed>, TypeHandle),
    ArrayAgg(
        Box<crate::analyze::plan::ResolvedQuery>,
        TypeHandle,
        EmptyArrayResult,
    ),
}

impl Typed {
    pub(crate) fn result_type(&self) -> Result<TypeHandle, HeadError> {
        Ok(match self {
            Typed::Column(_, _, ty, _) | Typed::Value(_, ty) | Typed::Cast(_, ty) => *ty,
            Typed::TableOid(..) => TypeHandle::by_oid(OID_OID)?,
            Typed::CurrentUser => TypeHandle::by_oid(NAME_OID)?,
            Typed::Concat(..) => TypeHandle::by_oid(TEXT_OID)?,
            Typed::Case { result, .. } => *result,
            Typed::Not(_)
            | Typed::And(_)
            | Typed::Or(_)
            | Typed::Compare(..)
            | Typed::IsNull(..)
            | Typed::Is(..)
            | Typed::DistinctFrom(..)
            | Typed::In(..)
            | Typed::AlwaysFalse
            | Typed::Exists(_)
            | Typed::InSelect(..) => TypeHandle::by_oid(BOOL_OID)?,
            Typed::Subquery(query) => query.first.output.first().map_or_else(
                || {
                    Err(HeadError::internal(
                        "a scalar subquery has no output column",
                    ))
                },
                Typed::result_type,
            )?,
            Typed::Call(handle, args) => call_result_type(*handle, args)?,
            Typed::Subscript(_, _, ty, _)
            | Typed::ArrayAgg(_, ty, _)
            | Typed::ArrayLiteral(_, ty) => *ty,
            Typed::AnyEq(..) => TypeHandle::by_oid(BOOL_OID)?,
        })
    }
}

fn call_result_type(handle: FunctionHandle, args: &[Typed]) -> Result<TypeHandle, HeadError> {
    match handle {
        FunctionHandle::ArrayRemove => args
            .first()
            .ok_or_else(|| HeadError::internal("array_remove is registered with an argument"))?
            .result_type(),
        FunctionHandle::ArrayAgg => {
            let arg = args
                .first()
                .ok_or_else(|| HeadError::internal("array_agg is registered with an argument"))?;
            let array_oid = arg.result_type()?.array_type();
            if array_oid == 0 {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::ArrayAggOverThisType,
                ));
            }
            TypeHandle::by_oid(i64::from(array_oid))
        }
        FunctionHandle::CurrentDatabase
        | FunctionHandle::CurrentSchemas
        | FunctionHandle::CurrentSetting
        | FunctionHandle::SetConfig
        | FunctionHandle::CurrentSettingMissingOk
        | FunctionHandle::PgIsInRecovery
        | FunctionHandle::Unnest
        | FunctionHandle::PgOptionsToTable
        | FunctionHandle::GenerateSeries
        | FunctionHandle::HeapTableamHandler
        | FunctionHandle::Bthandler
        | FunctionHandle::Hashhandler
        | FunctionHandle::Gisthandler
        | FunctionHandle::Ginhandler
        | FunctionHandle::Brinhandler
        | FunctionHandle::Spghandler
        | FunctionHandle::ArrayUpper
        | FunctionHandle::ArrayToString
        | FunctionHandle::QuoteIdent
        | FunctionHandle::QuoteLiteral
        | FunctionHandle::FormatType
        | FunctionHandle::AclDefault
        | FunctionHandle::PgGetTriggerdef
        | FunctionHandle::PgGetConstraintdef
        | FunctionHandle::PgGetConstraintdefPretty
        | FunctionHandle::PgGetIndexdef
        | FunctionHandle::PgGetIndexdefColumn
        | FunctionHandle::PgGetExpr => handle.result_type(),
    }
}

pub(super) fn numeric(ty: TypeHandle) -> bool {
    ty.category() == b'N'
}
