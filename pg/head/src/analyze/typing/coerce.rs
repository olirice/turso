use crate::analyze::types::{LiteralFold, TypeHandle, INT2_OID, INT4_OID, NAME_OID, TEXT_OID};
use crate::error::{HeadError, NotSupportedFeature, OperandType, PgError};
use crate::parse::expr::Expr;
use crate::parse::Location;

use super::check::Classified;
use super::context::Context;
use super::scope::Scope;
use super::{numeric, type_check, Scalar, Typed};

fn operator_does_not_exist(
    op: &'static str,
    left: OperandType,
    right: OperandType,
    op_location: Option<Location>,
) -> HeadError {
    HeadError::raise(PgError::OperatorDoesNotExist { op, left, right }).at(op_location)
}

fn literal_target_type(fixed: TypeHandle) -> Result<TypeHandle, HeadError> {
    if numeric(fixed) {
        return Ok(fixed);
    }
    if fixed.oid() == NAME_OID {
        return TypeHandle::by_oid(NAME_OID);
    }
    TypeHandle::by_oid(TEXT_OID)
}

pub(super) fn coerce_against(
    op: &'static str,
    target: TypeHandle,
    classified: Classified,
    op_location: Option<Location>,
) -> Result<Typed, HeadError> {
    match classified {
        Classified::Fixed(typed, ty) => {
            if numeric(ty) != numeric(target) {
                return Err(operator_does_not_exist(
                    op,
                    OperandType::Type(target),
                    OperandType::Type(ty),
                    op_location,
                ));
            }
            Ok(typed)
        }
        Classified::Unknown(Scalar::Null, _) => {
            Ok(Typed::Value(Scalar::Null, literal_target_type(target)?))
        }
        Classified::Unknown(Scalar::Integer(value), _) => {
            if !numeric(target) {
                return Err(operator_does_not_exist(
                    op,
                    OperandType::Type(target),
                    OperandType::Literal("integer"),
                    op_location,
                ));
            }
            Ok(Typed::Value(Scalar::Integer(value), target))
        }
        Classified::Unknown(Scalar::Text(text), location) => {
            if numeric(target) {
                parse_integer_text(&text, target).map_err(|error| error.at(location))?;
                Ok(Typed::Value(Scalar::Text(text), target))
            } else if target.category() == b'A' {
                let element = TypeHandle::by_oid(i64::from(target.element()))?;
                let elements = crate::parse::expr::parse_array_literal_text(&text, element)
                    .map_err(|error| error.at(location))?;
                Ok(Typed::Value(Scalar::Array(elements), target))
            } else {
                Ok(Typed::Value(
                    Scalar::Text(text),
                    literal_target_type(target)?,
                ))
            }
        }
        Classified::Unknown(Scalar::Array(_), _) => Err(HeadError::internal(
            "an array literal reached untyped literal coercion",
        )),
    }
}

pub(super) fn coerce_pair(
    op: &'static str,
    left: Classified,
    right: Classified,
    op_location: Option<Location>,
) -> Result<(Typed, Typed), HeadError> {
    match (left, right) {
        (Classified::Fixed(left, left_ty), right) => {
            let right = coerce_against(op, left_ty, right, op_location)?;
            Ok((left, right))
        }
        (left, Classified::Fixed(right, right_ty)) => {
            let left = coerce_against(op, right_ty, left, op_location)?;
            Ok((left, right))
        }
        (
            Classified::Unknown(Scalar::Text(text), location),
            Classified::Unknown(Scalar::Integer(value), _),
        ) => {
            let int4 = TypeHandle::by_oid(INT4_OID)?;
            parse_integer_text(&text, int4).map_err(|error| error.at(location))?;
            Ok((
                Typed::Value(Scalar::Text(text), int4),
                Typed::Value(Scalar::Integer(value), int4),
            ))
        }
        (
            Classified::Unknown(Scalar::Integer(value), _),
            Classified::Unknown(Scalar::Text(text), location),
        ) => {
            let int4 = TypeHandle::by_oid(INT4_OID)?;
            parse_integer_text(&text, int4).map_err(|error| error.at(location))?;
            Ok((
                Typed::Value(Scalar::Integer(value), int4),
                Typed::Value(Scalar::Text(text), int4),
            ))
        }
        (Classified::Unknown(left, _), Classified::Unknown(right, _)) => {
            Ok((default_typed(left)?, default_typed(right)?))
        }
    }
}

pub(super) fn default_typed(scalar: Scalar) -> Result<Typed, HeadError> {
    let ty = match &scalar {
        Scalar::Integer(_) => TypeHandle::by_oid(INT4_OID)?,
        Scalar::Text(_) | Scalar::Null => TypeHandle::by_oid(TEXT_OID)?,
        Scalar::Array(_) => {
            return Err(HeadError::internal(
                "an array literal reached scalar coercion with no target type",
            ))
        }
    };
    Ok(Typed::Value(scalar, ty))
}

pub(super) fn coerce_concat_operand(
    classified: Classified,
    op_location: Option<Location>,
) -> Result<Typed, HeadError> {
    match classified {
        Classified::Fixed(typed, ty) => {
            if numeric(ty) {
                return Err(operator_does_not_exist(
                    "||",
                    OperandType::Type(ty),
                    OperandType::Literal("text"),
                    op_location,
                ));
            }
            Ok(typed)
        }
        Classified::Unknown(Scalar::Integer(_), _) => Err(operator_does_not_exist(
            "||",
            OperandType::Literal("integer"),
            OperandType::Literal("text"),
            op_location,
        )),
        Classified::Unknown(Scalar::Text(text), _) => Ok(Typed::Value(
            Scalar::Text(text),
            TypeHandle::by_oid(TEXT_OID)?,
        )),
        Classified::Unknown(Scalar::Null, _) => {
            Ok(Typed::Value(Scalar::Null, TypeHandle::by_oid(TEXT_OID)?))
        }
        Classified::Unknown(Scalar::Array(_), _) => Err(HeadError::internal(
            "an array literal reached concatenation coercion",
        )),
    }
}

fn fold_literal(typed: Typed) -> Result<Scalar, HeadError> {
    match typed {
        Typed::Value(scalar, _) => Ok(scalar),
        Typed::Cast(inner, ty) => match *inner {
            Typed::Value(scalar, _) => coerce_scalar(scalar, ty),
            Typed::Column(..)
            | Typed::TableOid(_)
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
            | Typed::ArrayAgg(..) => Err(HeadError::not_supported(
                NotSupportedFeature::ExpressionNotLiteralOrCast,
            )),
        },
        Typed::Column(..)
        | Typed::TableOid(_)
        | Typed::CurrentUser
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
        | Typed::ArrayAgg(..) => Err(HeadError::not_supported(
            NotSupportedFeature::ExpressionNotLiteralOrCast,
        )),
    }
}

/// A literal scalar coerced to a known target type: PostgreSQL 18 keeps a
/// numeric-targeted text literal as text (probed: `pg_get_expr` shows
/// `'22'::bigint`, not a bare `22`, for a `bigint` column; only an
/// `integer` target ever renders bare digits, `render_scalar`'s own
/// `coerced_integer`), so this never resolves one to an `Integer` early;
/// whichever engine value it becomes is resolved once, at the last
/// possible point (`to_engine_value`, `lower/expr.rs`'s `scalar_value`),
/// the same as a numeric-targeted literal `coerce_against` (this module's
/// own operator-operand coercion) already leaves untouched.
pub(super) fn coerce_scalar(scalar: Scalar, target: TypeHandle) -> Result<Scalar, HeadError> {
    match scalar {
        Scalar::Null => Ok(Scalar::Null),
        Scalar::Integer(value) if numeric(target) => Ok(Scalar::Integer(value)),
        Scalar::Integer(value) => Ok(Scalar::Text(value.to_string())),
        Scalar::Text(text) => fold_text_literal(text, target),
        Scalar::Array(_) => Err(HeadError::not_supported(
            NotSupportedFeature::ArrayValueInContext,
        )),
    }
}

/// The one type-directed step of `coerce_scalar`: every other `Scalar`
/// kind (`Null`, `Integer`, `Array`) coerces the same way regardless of
/// target type, but a text literal's fate depends on the target's own
/// `LiteralFold` fact (`analyze/types.rs`).
fn fold_text_literal(text: String, target: TypeHandle) -> Result<Scalar, HeadError> {
    match target.literal_fold() {
        LiteralFold::Numeric => {
            parse_integer_text(&text, target)?;
            Ok(Scalar::Text(text))
        }
        LiteralFold::Text => Ok(Scalar::Text(text)),
        LiteralFold::Char => char_truncate(&text).map(Scalar::Text),
        LiteralFold::NotFoldable => Err(HeadError::internal(format!(
            "a text literal cannot be folded to type {}, which this crate's admitted SQL never \
             casts a text literal to",
            target.name()
        ))),
    }
}

/// PostgreSQL 18's own `"char"` input truncates to the value's first byte,
/// `core/schema.rs`'s `CREATE TYPE "char" ... ENCODE` expression at the
/// engine level; probed against a live PostgreSQL 18: `''::"char"`,
/// `'r'::"char"` and `'abc'::"char"` all truncate to a single byte, and a
/// leading NUL byte (the empty-string case) prints as empty text since
/// `charout` returns a NUL-terminated C string. Every `"char"` literal in
/// this crate's admitted SQL (`pg_dump`'s own `'r'::"char"`, `'s'::"char"`,
/// `' '::"char"`, and similar catalog codes) is a single ASCII byte; a
/// multi-byte UTF-8 lead byte cannot be held in a Rust `String` on its own,
/// so that case is refused rather than guessed at.
fn char_truncate(text: &str) -> Result<String, HeadError> {
    match text.as_bytes().first().copied() {
        None | Some(0) => Ok(String::new()),
        Some(byte) if byte < 0x80 => {
            let ascii = [byte];
            std::str::from_utf8(&ascii)
                .map(str::to_string)
                .map_err(|_| HeadError::internal("a byte below 0x80 is always valid UTF-8"))
        }
        Some(_) => Err(HeadError::internal(
            "a \"char\" literal's first byte is not ASCII; this head only folds the single-byte \
             catalog codes its admitted SQL casts to \"char\"",
        )),
    }
}

fn to_engine_value(scalar: Scalar, target: TypeHandle) -> Result<turso_core::Value, HeadError> {
    match scalar {
        Scalar::Null => Ok(turso_core::Value::Null),
        Scalar::Integer(value) => {
            if target.oid() == INT4_OID && i32::try_from(value).is_err() {
                return Err(HeadError::raise(PgError::IntegerOutOfRange));
            }
            Ok(turso_core::Value::from_i64(value))
        }
        // A numeric-targeted literal reaching here is still text-backed
        // (`coerce_scalar` above never resolves one early); resolved here,
        // the same as `lower/expr.rs`'s `scalar_value` resolves one reached
        // through a general typed tree. `parse_integer_text` checks the
        // target's own width itself, so this needs no separate range check
        // the way the bare-`Integer` arm above does.
        Scalar::Text(text) if numeric(target) => Ok(turso_core::Value::from_i64(
            parse_integer_text(&text, target)?,
        )),
        Scalar::Text(text) => Ok(turso_core::Value::from_text(text)),
        Scalar::Array(_) => Err(HeadError::internal(
            "an array literal reached engine value conversion",
        )),
    }
}

pub(crate) fn assign(
    expr: Expr,
    target: TypeHandle,
    cx: &Context,
) -> Result<turso_core::Value, HeadError> {
    // `to_engine_value`'s own "integer out of range" (an in-range-for-i64,
    // out-of-range-for-i32 bare integer constant) carries no position on a
    // live PostgreSQL 18 (probed), unlike every other error `coerce_constant`
    // raises, so it is not wrapped with one here.
    to_engine_value(coerce_constant(expr, target, cx)?, target)
}

pub(crate) fn coerce_constant(
    expr: Expr,
    target: TypeHandle,
    cx: &Context,
) -> Result<Scalar, HeadError> {
    let location = cx.locate(literal_location(&expr));
    let typed = type_check(expr, &Scope::empty(), cx)?;
    fold_literal(typed)
        .and_then(|scalar| coerce_scalar(scalar, target))
        .map_err(|error| error.at(location))
}

/// The location a bare literal or a cast of one (the only shapes
/// `fold_literal` accepts) carries, read before `type_check` consumes
/// `expr`: an `INSERT` value or `EXECUTE` parameter is always one of these
/// two shapes, so this is never asked about anything else.
fn literal_location(expr: &Expr) -> Option<Location> {
    match expr {
        Expr::Literal(_, location) => *location,
        Expr::Cast(inner, _) => literal_location(inner),
        Expr::Column(..)
        | Expr::CurrentUser
        | Expr::Param(..)
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
        | Expr::Resolved(_) => None,
    }
}

pub(crate) fn parse_integer_text(text: &str, ty: TypeHandle) -> Result<i64, HeadError> {
    let invalid = || {
        HeadError::raise(PgError::InvalidInputSyntax {
            ty,
            text: text.to_string(),
        })
    };
    let trimmed =
        text.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c'));
    let (negative, unsigned) = match trimmed.as_bytes().first() {
        // The matched byte is a single-byte ASCII sign, always on a char
        // boundary, so the rest of the text is always present.
        Some(b'-') => (true, trimmed.get(1..).unwrap_or_default()),
        Some(b'+') => (false, trimmed.get(1..).unwrap_or_default()),
        _ => (false, trimmed),
    };
    let (radix, digits, underscore_may_lead) = match unsigned.get(..2) {
        // The matched prefix is two ASCII bytes, always on a char boundary,
        // so the rest of the text is always present.
        Some("0x" | "0X") => (16, unsigned.get(2..).unwrap_or_default(), true),
        Some("0o" | "0O") => (8, unsigned.get(2..).unwrap_or_default(), true),
        Some("0b" | "0B") => (2, unsigned.get(2..).unwrap_or_default(), true),
        _ => (10, unsigned, false),
    };
    let mut magnitude: i128 = 0;
    let mut seen_digit = false;
    let mut characters = digits.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '_' {
            let followed_by_digit = characters.peek().is_some_and(|next| next.is_digit(radix));
            if !followed_by_digit || (!seen_digit && !underscore_may_lead) {
                return Err(invalid());
            }
            continue;
        }
        let digit = character.to_digit(radix).ok_or_else(invalid)?;
        seen_digit = true;
        magnitude = (magnitude * i128::from(radix) + i128::from(digit)).min(i128::from(u64::MAX));
    }
    if !seen_digit {
        return Err(invalid());
    }
    let value = if negative { -magnitude } else { magnitude };
    let fits = if ty.oid() == INT4_OID {
        i32::try_from(value).is_ok()
    } else if ty.oid() == INT2_OID {
        i16::try_from(value).is_ok()
    } else {
        i64::try_from(value).is_ok()
    };
    if !fits {
        return Err(HeadError::raise(PgError::ValueOutOfRangeForType {
            text: text.to_string(),
            ty,
        }));
    }
    i64::try_from(value)
        .map_err(|_| HeadError::internal("a value that fits the column type fits in i64"))
}
