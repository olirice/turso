use turso_core::{Numeric, Value};

use crate::catalog::pg;
use crate::catalog::{Catalog, Oid};
use crate::error::HeadError;

use super::ident;

/// `reltuples` is the only `float4` column the head ever populates
/// (`class_storage_facts`); its value is always exact in `f32`, so the
/// engine's own `f64` storage (`turso_core::Numeric::Float`, which can
/// never hold `NaN`, only `Value::Numeric(Numeric::Float(NonNan))`) is
/// narrowed to `f32` here and formatted with PostgreSQL 18's own float4
/// text output (`format_float`, `render/float.rs`).
pub(super) fn render_float4(value: Value) -> Result<Value, HeadError> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let float = match value {
        Value::Numeric(Numeric::Float(float)) => f64::from(float),
        Value::Null | Value::Numeric(_) | Value::Text(_) | Value::Blob(_) => {
            return Err(HeadError::internal(
                "a float4 column held a non-float engine value",
            ))
        }
    };
    // Rust has no fallible or trait-based `f64` -> `f32` narrowing (it is
    // always lossy, never an error): `as` is the only way to take a
    // `float4` column's engine-widened storage back down to its own
    // 32-bit precision before formatting it.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "float4's own precision; no non-`as` f64->f32 narrowing exists"
    )]
    let narrowed = float as f32;
    Ok(Value::from_text(super::float::format_float4(narrowed)?))
}

pub(super) fn render_bool(value: Value) -> Result<Value, HeadError> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let flag = value
        .as_int()
        .ok_or_else(|| HeadError::internal("a bool column held a non-integer engine value"))?;
    Ok(Value::from_text(
        if flag == 0 { "f" } else { "t" }.to_string(),
    ))
}

pub(super) fn render_regclass(value: Value, catalog: &Catalog) -> Result<Value, HeadError> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let oid = integer(value)?;
    let name = catalog
        .relation_name_at(Oid::from_i64(oid)?)
        .ok_or_else(|| HeadError::internal(format!("no relation is registered for oid {oid}")))?;
    Ok(Value::from_text(
        ident::render_identifier(name.as_str()).as_sql().to_string(),
    ))
}

pub(super) fn render_regproc(value: Value) -> Result<Value, HeadError> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let oid = integer(value)?;
    let target = u32::try_from(oid)
        .map_err(|_| HeadError::internal(format!("regproc oid {oid} does not fit in u32")))?;
    let row = pg::proc_rows()
        .iter()
        .find(|row| row.oid(pg::pg_proc::OID) == Ok(target))
        .ok_or_else(|| HeadError::internal(format!("no pg_proc row is captured for oid {oid}")))?;
    let name = row.text(pg::pg_proc::PRONAME)?;
    Ok(Value::from_text(
        ident::render_identifier(name).as_sql().to_string(),
    ))
}

pub(super) fn render_acl_array(value: Value, catalog: &Catalog) -> Result<Value, HeadError> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let elements =
        turso_core::decode_array(&value).map_err(|error| HeadError::internal(error.to_string()))?;
    let entries = elements
        .iter()
        .map(crate::catalog::AclEntry::from_value)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::from_text(super::acl::render_acl(catalog, &entries)))
}

pub(super) fn render_vector(value: Value) -> Result<Value, HeadError> {
    let text = match value {
        Value::Null => return Ok(Value::Null),
        other @ Value::Numeric(_) | other @ Value::Text(_) | other @ Value::Blob(_) => other
            .to_text()
            .ok_or_else(|| {
                HeadError::internal("an oidvector/int2vector column held a non-text engine value")
            })?
            .to_string(),
    };
    let inner = text
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| {
            HeadError::internal("an oidvector/int2vector column was not rendered as an array")
        })?;
    Ok(Value::from_text(inner.replace(',', " ")))
}

fn integer(value: Value) -> Result<i64, HeadError> {
    value
        .as_int()
        .ok_or_else(|| HeadError::internal("an oid/owner column held a non-integer engine value"))
}
