use crate::error::{HeadError, PgError};

use super::scope::Scope;
use super::walk::{walk_typed, TypedVisitor};
use super::Typed;

struct ContainsAggregate;

impl TypedVisitor for ContainsAggregate {
    type Stop = ();

    fn enter(&mut self, typed: &Typed) -> Result<(), ()> {
        if let Typed::Call(handle, _) = typed {
            if handle.evaluation() == crate::analyze::functions::Evaluation::Aggregate {
                return Err(());
            }
        }
        walk_typed(self, typed)
    }
}

pub(crate) fn contains_aggregate(typed: &Typed) -> bool {
    ContainsAggregate.enter(typed).is_err()
}

fn nested_aggregate_call() -> HeadError {
    HeadError::raise(PgError::NestedAggregateCall)
}

struct RefuseUngroupedColumns<'a, 'b, 'c> {
    scope: &'b Scope<'a, 'c>,
}

impl<'a> TypedVisitor for RefuseUngroupedColumns<'a, '_, '_> {
    type Stop = HeadError;

    fn enter(&mut self, typed: &Typed) -> Result<(), HeadError> {
        match typed {
            Typed::Column(slot, attnum, _, location) => {
                match self
                    .scope
                    .relations
                    .iter()
                    .find(|relation| relation.slot == *slot)
                {
                    Some(relation) => {
                        let (column, _) = relation.shape.column_at(*attnum)?;
                        Err(HeadError::raise(PgError::UngroupedColumn {
                            relation: relation.visible_as.clone(),
                            column,
                        })
                        .at(*location))
                    }
                    None => Ok(()),
                }
            }
            Typed::Call(handle, args)
                if handle.evaluation() == crate::analyze::functions::Evaluation::Aggregate =>
            {
                if args.iter().any(contains_aggregate) {
                    return Err(nested_aggregate_call());
                }
                Ok(())
            }
            other @ Typed::TableOid(_)
            | other @ Typed::Value(..)
            | other @ Typed::CurrentUser
            | other @ Typed::Cast(..)
            | other @ Typed::Not(_)
            | other @ Typed::And(_)
            | other @ Typed::Or(_)
            | other @ Typed::Compare(..)
            | other @ Typed::IsNull(..)
            | other @ Typed::Is(..)
            | other @ Typed::DistinctFrom(..)
            | other @ Typed::In(..)
            | other @ Typed::Concat(..)
            | other @ Typed::AlwaysFalse
            | other @ Typed::Case { .. }
            | other @ Typed::Subquery(_)
            | other @ Typed::Exists(_)
            | other @ Typed::InSelect(..)
            | other @ Typed::Call(..)
            | other @ Typed::Subscript(..)
            | other @ Typed::AnyEq(..)
            | other @ Typed::ArrayLiteral(..)
            | other @ Typed::ArrayAgg(..) => walk_typed(self, other),
        }
    }
}

pub(crate) fn refuse_ungrouped_columns(
    typed: &Typed,
    scope: &Scope<'_, '_>,
) -> Result<(), HeadError> {
    RefuseUngroupedColumns { scope }.enter(typed)
}
