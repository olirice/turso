use std::collections::BTreeMap;

use crate::analyze::functions::FunctionHandle;
use crate::analyze::plan::{ResolvedQuery, ResolvedSimpleSelect};
use crate::analyze::types::{TextOutput, TypeHandle};
use crate::analyze::typing::{self, Typed};
use crate::error::HeadError;
use crate::ident::ColumnName;
use crate::parse::statement::Query;
use crate::OutputColumn;

use super::walk::{resolve_query, WalkCtx};
use super::{Lookup, RelationFact};

pub(crate) struct SelectPlan {
    pub(crate) query: ResolvedQuery,
    pub(crate) references: Vec<RelationFact>,
    pub(crate) columns: Vec<OutputColumn>,
    pub(crate) output: Vec<OutputSpec>,
}

#[derive(Debug, Clone)]
pub(crate) enum OutputSpec {
    Column,
    Bool,
    Float4,
    RegClass,
    RegProc,
    AclArray,
    Vector,
    CatalogRendered(FunctionHandle),
    SessionCall(FunctionHandle),
    Unsupported(TypeHandle),
}

pub(super) fn select(query: Query, lookup: &Lookup) -> Result<SelectPlan, HeadError> {
    let mut ctx = WalkCtx {
        lookup,
        next_slot: 0,
        references: Vec::new(),
        slot_relations: BTreeMap::new(),
        privilege_actor: lookup.role.oid,
    };
    let (query, names) = resolve_query(query, &mut ctx, None)?;
    let (columns, output) = projection_shape(&query.first, &names, &ctx)?;
    Ok(SelectPlan {
        query,
        references: ctx.references,
        columns,
        output,
    })
}

fn projection_shape(
    first: &ResolvedSimpleSelect,
    names: &[ColumnName],
    ctx: &WalkCtx,
) -> Result<(Vec<OutputColumn>, Vec<OutputSpec>), HeadError> {
    let output = first
        .output
        .iter()
        .map(|item| match item {
            Typed::Column(slot, attnum, _, _) => {
                let shape = ctx.slot_relations.get(slot).ok_or_else(|| {
                    HeadError::internal("a projected column's relation slot was never resolved")
                })?;
                let (_, ty) = shape.column_at(*attnum)?;
                Ok(output_spec(ty))
            }
            Typed::Call(handle, _)
                if handle.evaluation()
                    == crate::analyze::functions::Evaluation::CatalogRendered =>
            {
                Ok(OutputSpec::CatalogRendered(*handle))
            }
            Typed::Call(handle, args)
                if handle.evaluation() == crate::analyze::functions::Evaluation::Session
                    && args.iter().any(|arg| !typing::is_session_constant(arg)) =>
            {
                Ok(OutputSpec::SessionCall(*handle))
            }
            Typed::TableOid(_)
            | Typed::Value(..)
            | Typed::CurrentUser
            | Typed::Cast(..)
            | Typed::Not(_)
            | Typed::And(_)
            | Typed::Or(_)
            | Typed::Compare(..)
            | Typed::IsNull(..)
            | Typed::Is(..)
            | Typed::DistinctFrom(..)
            | Typed::In(..)
            | Typed::Concat(..)
            | Typed::AlwaysFalse
            | Typed::Case { .. }
            | Typed::Subquery(_)
            | Typed::Exists(_)
            | Typed::InSelect(..)
            | Typed::Call(..)
            | Typed::Subscript(..)
            | Typed::AnyEq(..)
            | Typed::ArrayLiteral(..)
            | Typed::ArrayAgg(..) => Ok(output_spec(item.result_type()?)),
        })
        .collect::<Result<Vec<_>, HeadError>>()?;
    let columns = names
        .iter()
        .zip(&first.output)
        .map(|(name, item)| {
            Ok(OutputColumn {
                name: name.as_str().to_string(),
                type_oid: item.result_type()?.oid(),
            })
        })
        .collect::<Result<Vec<_>, HeadError>>()?;
    Ok((columns, output))
}

fn output_spec(ty: TypeHandle) -> OutputSpec {
    match ty.text_output() {
        Some(TextOutput::Column) => OutputSpec::Column,
        Some(TextOutput::Bool) => OutputSpec::Bool,
        Some(TextOutput::Float4) => OutputSpec::Float4,
        Some(TextOutput::RegClass) => OutputSpec::RegClass,
        Some(TextOutput::RegProc) => OutputSpec::RegProc,
        Some(TextOutput::Vector) => OutputSpec::Vector,
        Some(TextOutput::AclArray) => OutputSpec::AclArray,
        None => OutputSpec::Unsupported(ty),
    }
}
