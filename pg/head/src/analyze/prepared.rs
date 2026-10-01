use crate::analyze::types::TypeHandle;
use crate::analyze::typing::{self, Typed};
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::PreparedName;
use crate::parse::expr::Expr;
use crate::parse::statement::Query;
use crate::parse::walk::ExprMap;

use super::{Lookup, Resolved};

fn duplicate_prepared_statement(name: &PreparedName) -> HeadError {
    HeadError::raise(PgError::DuplicatePreparedStatement(name.clone()))
}

fn unknown_prepared_statement(name: &PreparedName) -> HeadError {
    HeadError::raise(PgError::UnknownPreparedStatement(name.clone()))
}

pub(super) fn prepare_statement(
    name: PreparedName,
    param_types: Vec<TypeHandle>,
    query: Query,
    lookup: &Lookup,
) -> Result<Resolved, HeadError> {
    if lookup.prepared.contains_key(&name) {
        return Err(duplicate_prepared_statement(&name));
    }
    Ok(Resolved::Prepare {
        name,
        statement: Box::new(crate::session::PreparedStatement { param_types, query }),
    })
}

pub(super) fn execute_statement(
    name: PreparedName,
    args: Vec<Expr>,
    lookup: &Lookup,
) -> Result<Query, HeadError> {
    let prepared = lookup
        .prepared
        .get(&name)
        .ok_or_else(|| unknown_prepared_statement(&name))?;
    if args.len() != prepared.param_types.len() {
        return Err(HeadError::raise(PgError::WrongParameterCount {
            name: name.clone(),
            expected: prepared.param_types.len(),
            got: args.len(),
        }));
    }
    let cx = typing::Context::new(typing::Position::ExecuteParameter);
    let bound = args
        .into_iter()
        .zip(&prepared.param_types)
        .map(|(arg, ty)| {
            let scalar = typing::coerce_constant(arg, *ty, &cx)?;
            Ok(Typed::Value(scalar, *ty))
        })
        .collect::<Result<Vec<Typed>, HeadError>>()?;
    SubstituteParams { bound: &bound }.map_query(prepared.query.clone())
}

struct SubstituteParams<'a> {
    bound: &'a [Typed],
}

impl ExprMap for SubstituteParams<'_> {
    fn map(&mut self, expr: Expr) -> Result<Expr, HeadError> {
        match expr {
            Expr::Param(number, _) => {
                let typed = usize::try_from(number)
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|index| self.bound.get(index))
                    .ok_or_else(|| {
                        HeadError::not_supported(NotSupportedFeature::ParameterWithoutDeclaredType(
                            number,
                        ))
                    })?;
                Ok(Expr::Resolved(Box::new(typed.clone())))
            }
            other @ Expr::Column(..)
            | other @ Expr::CurrentUser
            | other @ Expr::Literal(..)
            | other @ Expr::Cast(..)
            | other @ Expr::Not(_)
            | other @ Expr::And(_)
            | other @ Expr::Or(_)
            | other @ Expr::Compare(..)
            | other @ Expr::IsNull(..)
            | other @ Expr::Is(..)
            | other @ Expr::DistinctFrom(..)
            | other @ Expr::In(..)
            | other @ Expr::Concat(..)
            | other @ Expr::Case { .. }
            | other @ Expr::Subquery(..)
            | other @ Expr::Exists(..)
            | other @ Expr::InSelect(..)
            | other @ Expr::ArrayFromQuery(..)
            | other @ Expr::ArrayLiteral(_)
            | other @ Expr::Call(..)
            | other @ Expr::Subscript(..)
            | other @ Expr::AnyEq(..)
            | other @ Expr::Resolved(_) => crate::parse::walk::walk_expr_map(self, other),
        }
    }
}
