use crate::analyze::functions::{Evaluation, FunctionHandle};
use crate::analyze::types::{
    TypeHandle, BOOL_OID, INT2VECTOR_OID, INT4_OID, INT8_OID, NAME_OID, OIDVECTOR_OID, OID_OID,
    TEXT_OID,
};
use crate::catalog::Attnum;
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::{ColumnName, FunctionName};
use crate::parse::expr::{Expr, Literal, Truth};
use crate::parse::Location;

use super::coerce::{
    coerce_against, coerce_concat_operand, coerce_constant, coerce_pair, coerce_scalar,
    default_typed,
};
use super::context::{Context, Feature};
use super::scope::{column_lookup, tableoid_lookup};
use super::walk::{walk_typed, walk_typed_map, TypedMap, TypedVisitor};
use super::{numeric, RelationSlot, Scalar, Scope, Typed};

pub(super) enum Classified {
    Fixed(Typed, TypeHandle),
    /// The location is the literal's own (already filtered through
    /// `Context::locate` at the point `classify` produced this from an
    /// `Expr::Literal`), carried so a coercion failure (invalid input
    /// syntax, an out-of-range value) can point at it the way PostgreSQL 18
    /// does.
    Unknown(Scalar, Option<Location>),
}

fn must_be_boolean(what: &'static str, ty: TypeHandle) -> HeadError {
    HeadError::raise(PgError::ArgumentMustBeBoolean { what, actual: ty })
}

/// The operator-level boolean requirement (`NOT`, `AND`/`OR`, `CASE/WHEN`):
/// not a position rule, so it stays keyed by the operator's own name rather
/// than a `Position`.
pub(crate) fn require_boolean(what: &'static str, typed: &Typed) -> Result<(), HeadError> {
    let ty = typed.result_type()?;
    if ty.oid() != BOOL_OID {
        return Err(must_be_boolean(what, ty));
    }
    Ok(())
}

pub(super) fn classify(
    expr: Expr,
    scope: &Scope<'_, '_>,
    cx: &Context,
) -> Result<Classified, HeadError> {
    if let Some(feature) = Feature::of(&expr) {
        cx.permit(feature)
            .map_err(|error| error.at(cx.locate(Feature::location(&expr))))?;
    }
    match expr {
        Expr::Column(qualifier, name, location) => match column_lookup(&qualifier, &name, scope) {
            Ok((slot, attnum, ty)) => Ok(Classified::Fixed(
                Typed::Column(slot, attnum, ty, cx.locate(location)),
                ty,
            )),
            Err(error) if name == ColumnName::tableoid() => {
                match tableoid_lookup(&qualifier, scope) {
                    Ok(oid) => {
                        let ty = TypeHandle::by_oid(OID_OID)?;
                        Ok(Classified::Fixed(Typed::TableOid(oid), ty))
                    }
                    Err(_) => Err(error.at(cx.locate(location))),
                }
            }
            Err(error) => Err(error.at(cx.locate(location))),
        },
        Expr::CurrentUser => {
            let ty = TypeHandle::by_oid(NAME_OID)?;
            Ok(Classified::Fixed(Typed::CurrentUser, ty))
        }
        Expr::Literal(Literal::Integer(value), location) => Ok(Classified::Unknown(
            Scalar::Integer(value),
            cx.locate(location),
        )),
        Expr::Literal(Literal::Text(value), location) => Ok(Classified::Unknown(
            Scalar::Text(value),
            cx.locate(location),
        )),
        Expr::Literal(Literal::Null, _) => Ok(Classified::Unknown(Scalar::Null, None)),
        Expr::Literal(Literal::Boolean(value), _) => {
            let ty = TypeHandle::by_oid(BOOL_OID)?;
            Ok(Classified::Fixed(
                Typed::Value(Scalar::Integer(i64::from(value)), ty),
                ty,
            ))
        }
        other @ (Expr::Param(..)
        | Expr::Cast(..)
        | Expr::Not(_)
        | Expr::And(_)
        | Expr::Or(_)
        | Expr::Compare(..)
        | Expr::IsNull(..)
        | Expr::Is(..)
        | Expr::DistinctFrom(..)
        | Expr::In(..)
        | Expr::Concat(..)
        | Expr::Case { .. }
        | Expr::Subquery(..)
        | Expr::Exists(..)
        | Expr::InSelect(..)
        | Expr::ArrayFromQuery(..)
        | Expr::ArrayLiteral(_)
        | Expr::Call(..)
        | Expr::Subscript(..)
        | Expr::AnyEq(..)
        | Expr::Resolved(_)) => {
            let typed = type_check(other, scope, cx)?;
            let ty = typed.result_type()?;
            Ok(Classified::Fixed(typed, ty))
        }
    }
}

pub(crate) fn type_check(
    expr: Expr,
    scope: &Scope<'_, '_>,
    cx: &Context,
) -> Result<Typed, HeadError> {
    if let Some(feature) = Feature::of(&expr) {
        cx.permit(feature)
            .map_err(|error| error.at(cx.locate(Feature::location(&expr))))?;
    }
    match expr {
        Expr::Column(_, _, _) | Expr::CurrentUser | Expr::Literal(..) => {
            match classify(expr, scope, cx)? {
                Classified::Fixed(typed, _) => Ok(typed),
                Classified::Unknown(scalar, _) => default_typed(scalar),
            }
        }
        // `ARRAY[]` alone cannot determine its own type (42P18); PostgreSQL's
        // only accepted syntax for an empty array is an immediate cast
        // (`ARRAY[]::integer[]`), which supplies it directly. A literal
        // string cast to an array type (`'{1,2}'::int4[]`) is validated
        // here too, rather than at the parse edge: this `Context` is what
        // keeps a policy's `USING` positionless the way PostgreSQL 18 does,
        // the same as every other literal.
        Expr::Cast(inner, ty) if ty.category() == b'A' => match *inner {
            Expr::ArrayLiteral(elements) if elements.is_empty() => {
                Ok(Typed::Value(Scalar::Array(Vec::new()), ty))
            }
            Expr::Literal(Literal::Text(text), location) => {
                let element = TypeHandle::by_oid(i64::from(ty.element()))?;
                let elements = crate::parse::expr::parse_array_literal_text(&text, element)
                    .map_err(|error| error.at(cx.locate(location)))?;
                Ok(Typed::Value(Scalar::Array(elements), ty))
            }
            other @ Expr::Column(..)
            | other @ Expr::CurrentUser
            | other @ Expr::Literal(..)
            | other @ Expr::Param(..)
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
            | other @ Expr::Resolved(_) => {
                Ok(Typed::Cast(Box::new(type_check(other, scope, cx)?), ty))
            }
        },
        // A cast directly over a text or `NULL` literal is folded into the
        // one constant it names (`coerce_constant`, the same fold
        // `INSERT`'s and `EXECUTE`'s own literal-or-cast-of-one values
        // already go through), not a `Cast` wrapping a separately
        // defaulted `Value`: PostgreSQL 18's own analyzer keeps no residual
        // cast over one either (probed: `pg_get_expr` shows `NULL::integer`,
        // never `(NULL::text)::integer`, and folds `'5'::integer = 5` to
        // `5 = 5`). A text or `NULL` token is PostgreSQL's own polymorphic
        // "unknown" constant, coerced directly to whatever type context
        // asks for; every type folds here, including `"char"`
        // (`analyze/types.rs`'s `LiteralFold::Char`,
        // `analyze/typing/coerce.rs`'s `coerce_scalar`), never excluded by
        // an oid comparison.
        //
        // An integer or boolean literal is not PostgreSQL's "unknown" type:
        // the parser gives each its own concrete type immediately (`int4`,
        // widening to `int8` only once the value overflows `int4`; `bool`
        // for `TRUE`/`FALSE`), so a cast to anything else stays a genuine,
        // unfolded `Cast` over that concretely-typed value (probed:
        // `pg_get_expr` shows `(5)::bigint` and `(174358563)::text`, never
        // folding the way a text literal's cast does); only a cast whose
        // target already equals that concrete type is a no-op, folding the
        // same as PostgreSQL elides a redundant same-type cast entirely.
        Expr::Cast(inner, ty) => match inner.as_ref() {
            Expr::Literal(Literal::Text(_) | Literal::Null, _) => {
                Ok(Typed::Value(coerce_constant(*inner, ty, cx)?, ty))
            }
            Expr::Literal(Literal::Integer(_) | Literal::Boolean(_), _) => {
                cast_of_concrete_literal(*inner, ty)
            }
            Expr::Column(..)
            | Expr::CurrentUser
            | Expr::Param(..)
            | Expr::Cast(..)
            | Expr::Not(_)
            | Expr::And(_)
            | Expr::Or(_)
            | Expr::Compare(..)
            | Expr::IsNull(..)
            | Expr::Is(..)
            | Expr::DistinctFrom(..)
            | Expr::In(..)
            | Expr::Concat(..)
            | Expr::Case { .. }
            | Expr::Subquery(..)
            | Expr::Exists(..)
            | Expr::InSelect(..)
            | Expr::ArrayFromQuery(..)
            | Expr::ArrayLiteral(_)
            | Expr::Call(..)
            | Expr::Subscript(..)
            | Expr::AnyEq(..)
            | Expr::Resolved(_) => {
                // PostgreSQL 18 elides a cast whose target already equals
                // its argument's own type, for any argument shape, not
                // only a literal (probed: `pg_get_expr` shows `(x > x)`
                // for `x > (x)::text` when `x` is already `text`, but keeps
                // `(i)::bigint` when `i` is `integer`); `cast_of_concrete_literal`
                // above already applies the same rule to a literal.
                let inner = type_check(*inner, scope, cx)?;
                if inner.result_type()?.oid() == ty.oid() {
                    Ok(inner)
                } else {
                    Ok(Typed::Cast(Box::new(inner), ty))
                }
            }
        },
        Expr::Not(inner) => {
            let inner = type_check(*inner, scope, cx)?;
            require_boolean("NOT", &inner)?;
            Ok(Typed::Not(Box::new(inner)))
        }
        Expr::And(items) => Ok(Typed::And(boolean_operands("AND", items, scope, cx)?)),
        Expr::Or(items) => Ok(Typed::Or(boolean_operands("OR", items, scope, cx)?)),
        Expr::Compare(op, left, right, location) => {
            let (left, right) = coerce_pair(
                op.pg_symbol(),
                classify(*left, scope, cx)?,
                classify(*right, scope, cx)?,
                cx.locate(location),
            )?;
            Ok(Typed::Compare(op, Box::new(left), Box::new(right)))
        }
        Expr::IsNull(inner, negated) => Ok(Typed::IsNull(
            Box::new(type_check(*inner, scope, cx)?),
            negated,
        )),
        Expr::Is(inner, truth, negated) => {
            let inner = type_check(*inner, scope, cx)?;
            let ty = inner.result_type()?;
            if ty.oid() != BOOL_OID {
                let what = match (truth, negated) {
                    (Truth::True, false) => "IS TRUE",
                    (Truth::True, true) => "IS NOT TRUE",
                    (Truth::False, false) => "IS FALSE",
                    (Truth::False, true) => "IS NOT FALSE",
                    (Truth::Unknown, false) => "IS UNKNOWN",
                    (Truth::Unknown, true) => "IS NOT UNKNOWN",
                };
                return Err(must_be_boolean(what, ty));
            }
            Ok(Typed::Is(Box::new(inner), truth, negated))
        }
        Expr::DistinctFrom(left, right, negated, location) => {
            let (left, right) = coerce_pair(
                "=",
                classify(*left, scope, cx)?,
                classify(*right, scope, cx)?,
                cx.locate(location),
            )?;
            Ok(Typed::DistinctFrom(
                Box::new(left),
                Box::new(right),
                negated,
            ))
        }
        Expr::In(needle, list, negated, location) => {
            let needle = type_check(*needle, scope, cx)?;
            let needle_ty = needle.result_type()?;
            let location = cx.locate(location);
            let list = list
                .into_iter()
                .map(|item| coerce_against("=", needle_ty, classify(item, scope, cx)?, location))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Typed::In(Box::new(needle), list, negated))
        }
        Expr::Concat(left, right, location) => {
            let location = cx.locate(location);
            let left = coerce_concat_operand(classify(*left, scope, cx)?, location)?;
            let right = coerce_concat_operand(classify(*right, scope, cx)?, location)?;
            Ok(Typed::Concat(Box::new(left), Box::new(right)))
        }
        Expr::Case {
            base,
            arms,
            otherwise,
        } => case_expr(base, arms, otherwise, scope, cx),
        Expr::Resolved(typed) => Ok(*typed),
        Expr::Subquery(_, _)
        | Expr::Exists(_, _)
        | Expr::InSelect(..)
        | Expr::ArrayFromQuery(..) => Err(HeadError::internal(
            "a subquery expression reached typing without being pre-resolved",
        )),
        Expr::ArrayLiteral(elements) => array_literal_expr(elements, scope, cx),
        Expr::Call(name, args, location) => call_expr(name, args, location, scope, cx),
        Expr::Subscript(base, index) => subscript_expr(*base, *index, scope, cx),
        Expr::AnyEq(needle, array, location) => any_eq_expr(*needle, *array, location, scope, cx),
        Expr::Param(number, location) => {
            Err(HeadError::raise(PgError::UndefinedParameter(number)).at(cx.locate(location)))
        }
    }
}

/// The concrete type PostgreSQL 18's own parser gives a bare integer
/// token directly, before any cast: whichever of this crate's two
/// declarable integer types (`integer`/`bigint`) is the narrowest whose
/// own captured byte width (`TypeHandle::len`, PostgreSQL's own
/// `pg_type.typlen`) can hold `value` (this crate never admits a literal
/// wide enough to need `numeric`) -- derived from that captured fact, not
/// a hardcoded oid choice. Shared by `cast_of_concrete_literal` (an
/// explicit `::type` cast) and `case_expr`'s own arm unification: both
/// are PostgreSQL 18 coercing this same concretely-typed value to some
/// other type, never folding it the way a text or `NULL` literal's
/// coercion does.
fn natural_integer_type(value: i64) -> Result<TypeHandle, HeadError> {
    let int4 = TypeHandle::by_oid(INT4_OID)?;
    let fits_int4 = match int4.len() {
        4 => i32::try_from(value).is_ok(),
        other => {
            return Err(HeadError::internal(format!(
                "integer's own captured byte width is {other}, not 4"
            )))
        }
    };
    if fits_int4 {
        Ok(int4)
    } else {
        TypeHandle::by_oid(INT8_OID)
    }
}

/// A concretely-typed value (`natural`) coerced to `target`: PostgreSQL 18
/// elides the coercion entirely when `target` already is that value's own
/// type (probed: `pg_get_expr` shows `(i > i)` for `i > (i)::integer`,
/// never `(i > (i)::integer)`), otherwise keeps a genuine, unfolded `Cast`
/// (probed: `pg_get_expr` shows `(5)::bigint` and, inside a `CASE` whose
/// other arm is `bigint`, `('-61325'::integer)::bigint`).
fn concrete_value_coerced(scalar: Scalar, natural: TypeHandle, target: TypeHandle) -> Typed {
    if natural.oid() == target.oid() {
        Typed::Value(scalar, target)
    } else {
        Typed::Cast(Box::new(Typed::Value(scalar, natural)), target)
    }
}

/// A cast directly over an integer or boolean literal: PostgreSQL 18 never
/// folds this away unless the cast is a same-type no-op (see the doc
/// comment at this function's one call site, `type_check`'s `Expr::Cast`
/// arm). `expr` is always one of `Expr::Literal(Literal::Integer(_), _)`
/// or `Expr::Literal(Literal::Boolean(_), _)`; every other shape is a bug
/// in the caller, not a value this crate's admitted SQL can produce here.
fn cast_of_concrete_literal(expr: Expr, ty: TypeHandle) -> Result<Typed, HeadError> {
    let (scalar, natural) =
        match expr {
            Expr::Literal(Literal::Integer(value), _) => {
                (Scalar::Integer(value), natural_integer_type(value)?)
            }
            Expr::Literal(Literal::Boolean(value), _) => (
                Scalar::Integer(i64::from(value)),
                TypeHandle::by_oid(BOOL_OID)?,
            ),
            Expr::Literal(Literal::Text(_) | Literal::Null, _)
            | Expr::Column(..)
            | Expr::CurrentUser
            | Expr::Param(..)
            | Expr::Cast(..)
            | Expr::Not(_)
            | Expr::And(_)
            | Expr::Or(_)
            | Expr::Compare(..)
            | Expr::IsNull(..)
            | Expr::Is(..)
            | Expr::DistinctFrom(..)
            | Expr::In(..)
            | Expr::Concat(..)
            | Expr::Case { .. }
            | Expr::Subquery(..)
            | Expr::Exists(..)
            | Expr::InSelect(..)
            | Expr::ArrayFromQuery(..)
            | Expr::ArrayLiteral(_)
            | Expr::Call(..)
            | Expr::Subscript(..)
            | Expr::AnyEq(..)
            | Expr::Resolved(_) => return Err(HeadError::internal(
                "cast_of_concrete_literal called on a value that is not a concretely-typed literal",
            )),
        };
    Ok(concrete_value_coerced(scalar, natural, ty))
}

pub(crate) fn zero_based_origin_oid(expr: &Typed) -> Result<i64, HeadError> {
    match expr {
        Typed::Cast(inner, _) => zero_based_origin_oid(inner),
        other @ Typed::Column(..)
        | other @ Typed::TableOid(_)
        | other @ Typed::Value(..)
        | other @ Typed::CurrentUser
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
        | other @ Typed::ArrayAgg(..) => Ok(other.result_type()?.oid()),
    }
}

fn subscript_expr(
    base: Expr,
    index: Expr,
    scope: &Scope<'_, '_>,
    cx: &Context,
) -> Result<Typed, HeadError> {
    let base = type_check(base, scope, cx)?;
    let base_ty = base.result_type()?;
    let zero_based = matches!(base_ty.oid(), INT2VECTOR_OID | OIDVECTOR_OID | NAME_OID)
        || matches!(
            zero_based_origin_oid(&base)?,
            INT2VECTOR_OID | OIDVECTOR_OID
        );
    let element = base_ty.element();
    if element == 0 {
        return Err(HeadError::not_supported(
            NotSupportedFeature::SubscriptOnType(base_ty),
        ));
    }
    let element_ty = TypeHandle::by_oid(i64::from(element))?;
    let index_ty = TypeHandle::by_oid(INT4_OID)?;
    let index = coerce_against(
        "array subscript",
        index_ty,
        classify(index, scope, cx)?,
        None,
    )?;
    Ok(Typed::Subscript(
        Box::new(base),
        Box::new(index),
        element_ty,
        zero_based,
    ))
}

fn any_eq_expr(
    needle: Expr,
    array: Expr,
    location: Option<Location>,
    scope: &Scope<'_, '_>,
    cx: &Context,
) -> Result<Typed, HeadError> {
    let array = type_check(array, scope, cx)?;
    let array_ty = array.result_type()?;
    let element = array_ty.element();
    if element == 0 {
        return Err(HeadError::not_supported(NotSupportedFeature::AnyOverType(
            array_ty,
        )));
    }
    let element_ty = TypeHandle::by_oid(i64::from(element))?;
    let needle = coerce_against(
        "=",
        element_ty,
        classify(needle, scope, cx)?,
        cx.locate(location),
    )?;
    Ok(Typed::AnyEq(Box::new(needle), Box::new(array)))
}

/// `ARRAY[e1, e2, ...]`. Probed against PostgreSQL 18: every element
/// unifies to one element type the same way `select_common_type` does for
/// this head's own closed set of types (a numeric literal or cast widens
/// to the widest numeric type present, a non-numeric type must match
/// exactly, an empty array with no elements and no enclosing cast is
/// `42P18`), then every element is coerced against that type the same way
/// `IN`'s list already is.
fn array_literal_expr(
    elements: Vec<Expr>,
    scope: &Scope<'_, '_>,
    cx: &Context,
) -> Result<Typed, HeadError> {
    let classified = elements
        .into_iter()
        .map(|element| classify(element, scope, cx))
        .collect::<Result<Vec<_>, _>>()?;
    let element_ty = array_literal_element_type(&classified)?;
    let elements = classified
        .into_iter()
        .map(|classified| coerce_against("ARRAY", element_ty, classified, None))
        .collect::<Result<Vec<_>, _>>()?;
    let array_oid = element_ty.array_type();
    if array_oid == 0 {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ArrayOfElementType(element_ty),
        ));
    }
    let array_ty = TypeHandle::by_oid(i64::from(array_oid))?;
    Ok(Typed::ArrayLiteral(elements, array_ty))
}

fn array_literal_element_type(classified: &[Classified]) -> Result<TypeHandle, HeadError> {
    let int4 = TypeHandle::by_oid(INT4_OID)?;
    let mut fixed: Option<TypeHandle> = None;
    let mut saw_unknown_integer = false;
    for classified in classified {
        let Classified::Fixed(_, ty) = classified else {
            if matches!(classified, Classified::Unknown(Scalar::Integer(_), _)) {
                saw_unknown_integer = true;
            }
            continue;
        };
        fixed = Some(match fixed {
            None => *ty,
            Some(current) if current.oid() == ty.oid() => current,
            Some(current) if numeric(current) && numeric(*ty) => widen_numeric(current, *ty),
            Some(current) => {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::ArrayTypesCannotBeMatched(current, *ty),
                ))
            }
        });
    }
    match fixed {
        // The only fixed element type present is narrower than `integer`
        // (`int2`, the sole type this crate's captured `pg_cast` shows
        // implicitly casting up to it): an untyped integer literal's own
        // natural type is `integer` (`natural_integer_type`), so the two
        // together still need `integer[]`, not `int2[]` (probed:
        // `ARRAY[1, 2::smallint]` is `integer[]`).
        Some(ty) if numeric(ty) && saw_unknown_integer && ty.implicitly_casts_to(int4) => Ok(int4),
        Some(ty) => Ok(ty),
        None if saw_unknown_integer => Ok(int4),
        None if classified.is_empty() => Err(HeadError::raise(PgError::IndeterminateEmptyArray)),
        None => TypeHandle::by_oid(TEXT_OID),
    }
}

/// PostgreSQL 18's own within-category widening for two numeric-category
/// types (`select_common_type`), read from the captured `pg_cast`
/// implicit-cast facts (`TypeHandle::implicitly_casts_to`) rather than a
/// hand-ranked width: `next` wins only when `current` implicitly casts to
/// it and not the reverse (probed: `integer`/`bigint` always converge on
/// `bigint`, regardless of which is `current`); otherwise `current` keeps
/// its own type, whether the pair is mutually castable either way
/// (`oid`/`regclass`) or not castable either way at all (unreached by any
/// admitted array literal today, since only a numeric-category mismatch
/// with an actual implicit order has ever been generated). This is the
/// one place this merge favors the earlier of a tied pair: `CASE`'s own
/// merge (`case_anchor_merge`) favors the later one instead, probed
/// separately there.
fn widen_numeric(current: TypeHandle, next: TypeHandle) -> TypeHandle {
    if current.implicitly_casts_to(next) && !next.implicitly_casts_to(current) {
        next
    } else {
        current
    }
}

/// The feature a resolved call needs permission for. `Feature::of_call`
/// alone cannot see this: a session function is unrestricted (folded away
/// before lowering, wherever it appears) unless one of its arguments is not
/// constant, in which case it needs the same per-row output hook a
/// catalog-rendered function needs, and so becomes `Feature::RowComputed`.
pub(super) fn call_feature(handle: FunctionHandle, args: &[Typed]) -> Option<Feature> {
    let needs_row_computed = handle.evaluation() == Evaluation::Session
        && args.iter().any(|arg| !is_session_constant(arg));
    if needs_row_computed {
        Some(Feature::RowComputed(handle))
    } else {
        Feature::of_call(handle)
    }
}

fn is_literal_null(typed: &Typed) -> bool {
    matches!(typed, Typed::Value(Scalar::Null, _))
}

/// The fallback `call_expr` takes only once `FunctionHandle::lookup` has
/// already failed against every argument's own default type: PostgreSQL
/// defers a literal `NULL` argument's type until a candidate overload is
/// chosen, so a call like `format_type(seqtypid, NULL)` needs `NULL`
/// treated as a wildcard, not defaulted to this head's usual fallback
/// (`TEXT`), before a match can be found. Every non-`NULL` argument keeps
/// exactly the type the first attempt already gave it.
fn resolve_with_deferred_nulls(
    name: &FunctionName,
    typed_args: Vec<Typed>,
    arg_types: &[TypeHandle],
) -> Result<(FunctionHandle, Vec<Typed>), HeadError> {
    let probe: Vec<Option<TypeHandle>> = typed_args
        .iter()
        .zip(arg_types)
        .map(|(typed, ty)| {
            if is_literal_null(typed) {
                None
            } else {
                Some(*ty)
            }
        })
        .collect();
    let handle = FunctionHandle::lookup_deferring_nulls(name, &probe)
        .ok_or_else(|| undefined_function(name.as_str(), arg_types))?;
    let declared = handle.declared_arg_types()?;
    let typed_args = typed_args
        .into_iter()
        .zip(declared)
        .map(|(typed, declared_ty)| {
            if is_literal_null(&typed) {
                Typed::Value(Scalar::Null, declared_ty)
            } else {
                typed
            }
        })
        .collect();
    Ok((handle, typed_args))
}

fn call_expr(
    name: FunctionName,
    args: Vec<Expr>,
    location: Option<Location>,
    scope: &Scope<'_, '_>,
    cx: &Context,
) -> Result<Typed, HeadError> {
    let typed_args = args
        .into_iter()
        .map(|arg| type_check(arg, scope, cx))
        .collect::<Result<Vec<_>, _>>()?;
    let arg_types = typed_args
        .iter()
        .map(Typed::result_type)
        .collect::<Result<Vec<_>, _>>()?;
    let (handle, typed_args) = match FunctionHandle::lookup(&name, &arg_types) {
        Some(handle) => (handle, typed_args),
        None => resolve_with_deferred_nulls(&name, typed_args, &arg_types)
            .map_err(|error| error.at(cx.locate(location)))?,
    };
    if let Some(feature) = call_feature(handle, &typed_args) {
        cx.permit(feature)
            .map_err(|error| error.at(cx.locate(location)))?;
    }
    let typed_args = match handle.evaluation() {
        crate::analyze::functions::Evaluation::SetReturning => {
            if typed_args.iter().any(|arg| !is_constant(arg)) {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::NonConstantArgumentToSetReturningFunction(name),
                ));
            }
            typed_args
        }
        crate::analyze::functions::Evaluation::Engine
        | crate::analyze::functions::Evaluation::HeadEngine
        | crate::analyze::functions::Evaluation::Aggregate => typed_args,
        crate::analyze::functions::Evaluation::CatalogRendered => {
            if typed_args.iter().any(|arg| !is_column_or_constant(arg)) {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::ArgumentNotColumnOrConstant(name),
                ));
            }
            typed_args
        }
        crate::analyze::functions::Evaluation::Session => {
            let typed_args: Vec<Typed> = typed_args
                .into_iter()
                .map(fold_session_literal_cast)
                .collect();
            if typed_args.iter().any(|arg| !is_column_or_constant(arg)) {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::ArgumentNotColumnOrConstant(name),
                ));
            }
            typed_args
        }
    };
    Ok(Typed::Call(handle, typed_args))
}

fn is_column_or_constant(typed: &Typed) -> bool {
    matches!(typed, Typed::Column(..)) || is_constant(typed)
}

fn fold_session_literal_cast(typed: Typed) -> Typed {
    if let Typed::Cast(inner, ty) = &typed {
        if let Typed::Value(scalar, _) = inner.as_ref() {
            if let Ok(folded) = coerce_scalar(scalar.clone(), *ty) {
                return Typed::Value(folded, *ty);
            }
        }
    }
    typed
}

pub(crate) fn is_session_constant(typed: &Typed) -> bool {
    matches!(typed, Typed::Value(..))
}

/// Finds the first call anywhere in `typed` that needs `Feature::RowComputed`
/// permission, i.e. the same traversal `Context::permit` would refuse if it
/// were asked about every node instead of just the ones `type_check` visits.
/// Used by the two places that check an already-typed expression instead of
/// a fresh one: a `SELECT` target's own nested subexpressions (which
/// `Position::SelectTarget` allows at the top only) and an `ORDER BY`
/// reference to a select-list item or position (which never gets its own
/// `type_check` pass at all).
struct FindRowComputed;

impl TypedVisitor for FindRowComputed {
    type Stop = Feature;

    fn enter(&mut self, typed: &Typed) -> Result<(), Feature> {
        if let Typed::Call(handle, args) = typed {
            if let Some(feature @ Feature::RowComputed(_)) = call_feature(*handle, args) {
                return Err(feature);
            }
        }
        walk_typed(self, typed)
    }
}

fn find_row_computed_feature(typed: &Typed) -> Option<Feature> {
    FindRowComputed.enter(typed).err()
}

/// A `SELECT` target item is allowed to be a `Feature::RowComputed` call
/// only when the item's own root is that call; a call nested any deeper
/// still needs the per-row output hook that only a top-level item gets.
pub(crate) fn refuse_row_computed_in_select_item(item: &Typed) -> Result<(), HeadError> {
    if let Typed::Call(handle, args) = item {
        if matches!(call_feature(*handle, args), Some(Feature::RowComputed(_))) {
            return Ok(());
        }
    }
    match find_row_computed_feature(item) {
        Some(feature) => Err(feature.refusal(super::context::Position::SelectTarget)),
        None => Ok(()),
    }
}

/// `ORDER BY`'s ordinal- and alias-reference forms reuse an already-typed
/// `SELECT` target item without a fresh `type_check` pass, so nothing would
/// otherwise refuse a `Feature::RowComputed` call reached only through them.
pub(crate) fn refuse_row_computed_in_order_by_reference(item: &Typed) -> Result<(), HeadError> {
    match find_row_computed_feature(item) {
        Some(feature) => Err(feature.refusal(super::context::Position::OrderBy)),
        None => Ok(()),
    }
}

pub(crate) fn undefined_function(name: &str, arg_types: &[TypeHandle]) -> HeadError {
    HeadError::raise(PgError::UndefinedFunction {
        name: name.to_string(),
        arg_types: arg_types.to_vec(),
    })
}

struct IsConstant;

impl TypedVisitor for IsConstant {
    type Stop = ();

    fn enter(&mut self, typed: &Typed) -> Result<(), ()> {
        match typed {
            Typed::Column(..)
            | Typed::Subquery(_)
            | Typed::Exists(_)
            | Typed::InSelect(..)
            | Typed::ArrayAgg(..) => Err(()),
            Typed::Value(..) | Typed::TableOid(..) | Typed::CurrentUser | Typed::AlwaysFalse => {
                Ok(())
            }
            other @ Typed::Cast(..)
            | other @ Typed::Not(_)
            | other @ Typed::And(_)
            | other @ Typed::Or(_)
            | other @ Typed::Compare(..)
            | other @ Typed::IsNull(..)
            | other @ Typed::Is(..)
            | other @ Typed::DistinctFrom(..)
            | other @ Typed::In(..)
            | other @ Typed::Concat(..)
            | other @ Typed::Case { .. }
            | other @ Typed::Call(..)
            | other @ Typed::Subscript(..)
            | other @ Typed::AnyEq(..)
            | other @ Typed::ArrayLiteral(..) => walk_typed(self, other),
        }
    }
}

fn is_constant(typed: &Typed) -> bool {
    IsConstant.enter(typed).is_ok()
}

fn boolean_operands(
    what: &'static str,
    items: Vec<Expr>,
    scope: &Scope<'_, '_>,
    cx: &Context,
) -> Result<Vec<Typed>, HeadError> {
    items
        .into_iter()
        .map(|item| {
            let typed = type_check(item, scope, cx)?;
            require_boolean(what, &typed)?;
            Ok(typed)
        })
        .collect()
}

fn case_expr(
    base: Option<Box<Expr>>,
    arms: Vec<(Expr, Expr)>,
    otherwise: Option<Box<Expr>>,
    scope: &Scope<'_, '_>,
    cx: &Context,
) -> Result<Typed, HeadError> {
    let base = base.map(|base| type_check(*base, scope, cx)).transpose()?;
    let base_ty = base.as_ref().map(Typed::result_type).transpose()?;
    let mut typed_arms = Vec::with_capacity(arms.len());
    for (condition, result) in arms {
        let when = match base_ty {
            Some(base_ty) => coerce_against("=", base_ty, classify(condition, scope, cx)?, None)?,
            None => {
                let condition = type_check(condition, scope, cx)?;
                require_boolean("CASE/WHEN", &condition)?;
                condition
            }
        };
        let then = classify(result, scope, cx)?;
        typed_arms.push((when, then));
    }
    let otherwise = otherwise
        .map(|otherwise| classify(*otherwise, scope, cx))
        .transpose()?;
    // Folded left to right over every arm that contributes a concrete
    // type, not just the first one found: PostgreSQL 18 keeps updating
    // its own running candidate common type as it walks a `CASE`'s arms
    // in order (`select_common_type`), so a later arm can still change
    // the anchor (`case_anchor_merge`'s own doc comment). A bare integer
    // (or boolean, already `Classified::Fixed` from `classify`) literal
    // contributes its own concrete type here too, the same as a column:
    // only a text or `NULL` token is PostgreSQL's genuinely polymorphic
    // "unknown", skipped here and coerced later in `case_branch` (probed:
    // `CASE WHEN true THEN 5 ELSE text_col END` raises the same "CASE
    // types text and integer cannot be matched" a `Fixed`/`Fixed`
    // mismatch would, so a bare integer is not skipped as unknown).
    let mut anchor: Option<TypeHandle> = None;
    for classified in typed_arms
        .iter()
        .map(|(_, then)| then)
        .chain(otherwise.iter())
    {
        if let Some(candidate) = case_candidate_type(classified)? {
            anchor = Some(match anchor {
                None => candidate,
                Some(current) => case_anchor_merge(current, candidate)?,
            });
        }
    }
    // Every arm was a text or `NULL` token (or there were none): PostgreSQL
    // 18 defaults an all-unknown `CASE` to `text`, the same default every
    // other all-unknown context in this crate already falls back to
    // (`default_typed`).
    let anchor = match anchor {
        Some(ty) => ty,
        None => TypeHandle::by_oid(TEXT_OID)?,
    };
    let arms = typed_arms
        .into_iter()
        .map(|(when, then)| match case_branch(then, anchor) {
            Ok(then) => Ok((when, then)),
            Err(error) => Err(error),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let otherwise = otherwise
        .map(|then| case_branch(then, anchor))
        .transpose()?
        .map(Box::new);
    Ok(Typed::Case {
        base: base.map(Box::new),
        arms,
        otherwise,
        result: anchor,
    })
}

/// The concrete type one `CASE` arm contributes to `case_expr`'s own
/// left-to-right common-type fold, or `None` for PostgreSQL's genuinely
/// polymorphic "unknown" literal (a text or `NULL` token), which
/// `select_common_type` skips over the same way (probed: `CASE WHEN true
/// THEN 5 ELSE x END` against a `text` column `x` raises the same
/// mismatch a `text`/`integer` pair of columns would, so a bare integer
/// participates immediately; a bare `NULL` or string literal instead
/// defers to whatever the other arms settle on, `case_branch`'s own
/// `Classified::Unknown` arm). `Scalar::Array` never reaches this: an
/// array literal only ever classifies as `Classified::Fixed` (`classify`
/// runs it through `type_check`, not its own direct literal arm).
fn case_candidate_type(classified: &Classified) -> Result<Option<TypeHandle>, HeadError> {
    match classified {
        Classified::Fixed(_, ty) => Ok(Some(*ty)),
        Classified::Unknown(Scalar::Integer(value), _) => Ok(Some(natural_integer_type(*value)?)),
        Classified::Unknown(Scalar::Text(_) | Scalar::Null, _) => Ok(None),
        Classified::Unknown(Scalar::Array(_), _) => Err(HeadError::internal(
            "an array literal reached CASE arm type unification",
        )),
    }
}

/// PostgreSQL 18's own common-type rule for a `CASE`'s arms
/// (`select_common_type`), read from the captured `pg_cast` implicit-cast
/// facts (`TypeHandle::implicitly_casts_to`, `capture/out/casts.json`),
/// never a hardcoded pair of oids or a hand-ranked width: every arm must
/// share one `typcategory` (`TypeHandle::category`), else PostgreSQL
/// raises "CASE types {the arm that broke it} and {the running candidate
/// so far} cannot be matched" (42804), naming the two in that order
/// regardless of which position either is written in (probed, both
/// `THEN`/`ELSE` orders and a three-arm case). Within one category,
/// whichever of the pair the other one implicitly casts to wins (probed:
/// `integer`/`bigint` always converge on `bigint` regardless of which arm
/// holds which, since only `integer -> bigint` is a valid implicit cast;
/// `oid` always wins against either, since `integer`/`bigint` both cast
/// to `oid` implicitly but `oid` only assignment-casts back). A pair
/// mutually castable either way (`name` against `text`, through
/// `current_user`; `oid` against `regclass`) has no such order, so the
/// later arm keeps its own type and the earlier one grows the cast
/// instead (probed, every order). A pair sharing a category with no cast
/// either way at all (`regclass` against `regproc`, both cast only
/// through `oid`, never directly) is not the same refusal as a
/// cross-category mismatch: PostgreSQL 18 raises "CASE/WHEN could not
/// convert type {current} to {next}" (42846) there instead, naming the
/// pair in fold order, not the "cannot be matched" text above (probed).
fn case_anchor_merge(current: TypeHandle, next: TypeHandle) -> Result<TypeHandle, HeadError> {
    if current.oid() == next.oid() {
        return Ok(current);
    }
    if current.category() != next.category() {
        return Err(HeadError::raise(PgError::CaseTypesCannotBeMatched {
            left: next,
            right: current,
        }));
    }
    match (
        current.implicitly_casts_to(next),
        next.implicitly_casts_to(current),
    ) {
        (true, false) => Ok(next),
        (false, true) => Ok(current),
        (true, true) => Ok(next),
        (false, false) => Err(HeadError::raise(PgError::CaseCouldNotConvertType {
            from: current,
            to: next,
        })),
    }
}

fn case_branch(classified: Classified, anchor: TypeHandle) -> Result<Typed, HeadError> {
    match classified {
        // `case_candidate_type` already folded this arm's own type into
        // `anchor` (or raised `CaseTypesCannotBeMatched` doing so), so
        // reaching a mismatched category here would be this function's
        // own bug, not a normal refusal.
        Classified::Fixed(typed, ty) => {
            if ty.oid() == anchor.oid() {
                Ok(typed)
            } else {
                Ok(Typed::Cast(Box::new(typed), anchor))
            }
        }
        // A bare integer literal arm unified against another arm's
        // concrete type: PostgreSQL 18's own `CASE` unification is a real
        // coercion to one shared type, unlike a comparison operator
        // (which has direct cross-type overloads and never needs to
        // touch the constant at all), so this needs
        // `cast_of_concrete_literal`'s own rule, not `coerce_against`'s
        // (`concrete_value_coerced`'s own doc comment has the probe).
        Classified::Unknown(Scalar::Integer(value), _) => Ok(concrete_value_coerced(
            Scalar::Integer(value),
            natural_integer_type(value)?,
            anchor,
        )),
        Classified::Unknown(scalar, location) => {
            coerce_against("=", anchor, Classified::Unknown(scalar, location), None)
        }
    }
}

struct Retarget(RelationSlot);

impl TypedMap for Retarget {
    type Error = std::convert::Infallible;

    fn enter(&mut self, typed: Typed) -> Result<Typed, std::convert::Infallible> {
        Ok(match typed {
            Typed::Column(_, attnum, ty, location) => Typed::Column(self.0, attnum, ty, location),
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
            | other @ Typed::ArrayAgg(..) => walk_typed_map(self, other)?,
        })
    }
}

pub(crate) fn retarget(typed: &Typed, to: RelationSlot) -> Typed {
    match Retarget(to).enter(typed.clone()) {
        Ok(result) => result,
        Err(never) => match never {},
    }
}

struct ReferencedColumns<'a> {
    columns: &'a mut std::collections::BTreeSet<Attnum>,
}

impl TypedVisitor for ReferencedColumns<'_> {
    type Stop = std::convert::Infallible;

    fn enter(&mut self, typed: &Typed) -> Result<(), std::convert::Infallible> {
        match typed {
            Typed::Column(_, attnum, _, _) => {
                self.columns.insert(*attnum);
                Ok(())
            }
            Typed::InSelect(needle, _, _) => self.enter(needle),
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
            | other @ Typed::Call(..)
            | other @ Typed::Subscript(..)
            | other @ Typed::AnyEq(..)
            | other @ Typed::ArrayLiteral(..)
            | other @ Typed::ArrayAgg(..) => walk_typed(self, other),
        }
    }
}

pub(crate) fn referenced_columns(typed: &Typed, columns: &mut std::collections::BTreeSet<Attnum>) {
    match (ReferencedColumns { columns }).enter(typed) {
        Ok(()) => {}
        Err(never) => match never {},
    }
}
