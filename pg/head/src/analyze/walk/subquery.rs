use crate::analyze::plan::ResolvedQuery;
use crate::analyze::types::{TypeHandle, REGCLASS_OID, REGPROC_OID};
use crate::analyze::typing::{self, Context, Feature, Scalar, Scope, Typed};
use crate::catalog::Oid;
use crate::error::{HeadError, NotSupportedFeature};
use crate::parse::expr::{Expr, Literal};
use crate::parse::qualified_name;
use crate::parse::walk::{walk_expr_map, ExprMap};
use crate::parse::Location;

use super::{resolve_query, table, WalkCtx};

pub(super) fn type_check_expr<'a>(
    expr: Expr,
    scope: &Scope<'a, '_>,
    ctx: &mut WalkCtx<'a>,
    cx: &Context,
) -> Result<Typed, HeadError> {
    let presolved = Presolver { scope, ctx, cx }.map(expr)?;
    typing::type_check(presolved, scope, cx)
}

struct Presolver<'a, 'b, 'c> {
    scope: &'b Scope<'a, 'c>,
    ctx: &'b mut WalkCtx<'a>,
    cx: &'b Context,
}

impl<'a> ExprMap for Presolver<'a, '_, '_> {
    fn map(&mut self, expr: Expr) -> Result<Expr, HeadError> {
        match expr {
            Expr::Subquery(query, location) => {
                self.permit_subquery(location)?;
                let (resolved, _) = resolve_query(*query, self.ctx, Some(self.scope))?;
                require_single_column(&resolved)?;
                Ok(Expr::Resolved(Box::new(Typed::Subquery(Box::new(
                    resolved,
                )))))
            }
            Expr::Exists(query, location) => {
                self.permit_subquery(location)?;
                let (resolved, _) = resolve_query(*query, self.ctx, Some(self.scope))?;
                Ok(Expr::Resolved(Box::new(Typed::Exists(Box::new(resolved)))))
            }
            Expr::InSelect(needle, query, negated, location) => {
                self.permit_subquery(location)?;
                let needle = type_check_expr(*needle, self.scope, self.ctx, self.cx)?;
                let (resolved, _) = resolve_query(*query, self.ctx, Some(self.scope))?;
                require_single_column(&resolved)?;
                Ok(Expr::Resolved(Box::new(Typed::InSelect(
                    Box::new(needle),
                    Box::new(resolved),
                    negated,
                ))))
            }
            Expr::ArrayFromQuery(query, empty_result, location) => {
                self.permit_subquery(location)?;
                let (resolved, _) = resolve_query(*query, self.ctx, Some(self.scope))?;
                require_single_column(&resolved)?;
                let element_ty = resolved
                    .first
                    .output
                    .first()
                    .ok_or_else(|| {
                        HeadError::internal("a single-column query has no output column")
                    })?
                    .result_type()?;
                let array_oid = element_ty.array_type();
                if array_oid == 0 {
                    return Err(HeadError::not_supported(NotSupportedFeature::ArrayOfType(
                        element_ty,
                    )));
                }
                let array_ty = TypeHandle::by_oid(i64::from(array_oid))?;
                Ok(Expr::Resolved(Box::new(Typed::ArrayAgg(
                    Box::new(resolved),
                    array_ty,
                    empty_result,
                ))))
            }
            Expr::Cast(inner, ty) => {
                let inner = self.map(*inner)?;
                presolve_identifier_cast(inner, ty, self.ctx, self.cx)
            }
            other @ Expr::Column(..)
            | other @ Expr::CurrentUser
            | other @ Expr::Literal(..)
            | other @ Expr::Param(..)
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
            | other @ Expr::ArrayLiteral(_)
            | other @ Expr::Call(..)
            | other @ Expr::Subscript(..)
            | other @ Expr::AnyEq(..)
            | other @ Expr::Resolved(_) => walk_expr_map(self, other),
        }
    }
}

impl Presolver<'_, '_, '_> {
    /// The one gate for `Feature::Subquery`, whichever of the four subquery
    /// shapes reached it: attaches the `SubLink`'s own location the same
    /// way `check::classify`/`check::type_check`'s own `Feature::of` gate
    /// does for a subquery `coerce_constant` never presolves (an `EXECUTE`
    /// parameter).
    fn permit_subquery(&self, location: Option<Location>) -> Result<(), HeadError> {
        self.cx
            .permit(Feature::Subquery)
            .map_err(|error| error.at(self.cx.locate(location)))
    }
}

fn presolve_identifier_cast(
    inner: Expr,
    ty: TypeHandle,
    ctx: &WalkCtx,
    cx: &Context,
) -> Result<Expr, HeadError> {
    match (ty.oid(), inner) {
        (REGCLASS_OID, Expr::Literal(Literal::Text(raw), location)) => {
            let oid = resolve_regclass_literal(&raw, ctx, cx, location)?;
            Ok(Expr::Resolved(Box::new(Typed::Value(
                Scalar::Integer(oid.as_i64()),
                ty,
            ))))
        }
        (REGPROC_OID, Expr::Literal(Literal::Text(_), _)) => Err(HeadError::not_supported(
            NotSupportedFeature::RegprocCastFromName,
        )),
        (_, inner) => Ok(Expr::Cast(Box::new(inner), ty)),
    }
}

fn resolve_regclass_literal(
    raw: &str,
    ctx: &WalkCtx,
    cx: &Context,
    location: Option<Location>,
) -> Result<Oid, HeadError> {
    let located = cx.locate(location);
    let relation_name = qualified_name::relation_name(raw).map_err(|error| error.at(located))?;
    table(&relation_name, ctx.lookup)
        .map(|found| found.oid)
        .map_err(|_| qualified_name::undefined_relation(&relation_name).at(located))
}

fn require_single_column(query: &ResolvedQuery) -> Result<(), HeadError> {
    if query.first.output.len() != 1 {
        return Err(HeadError::not_supported(
            NotSupportedFeature::SubqueryMultipleColumns,
        ));
    }
    Ok(())
}
