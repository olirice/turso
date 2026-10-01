use turso_core::Value;

use crate::analyze::typing::Typed;
use crate::analyze::OutputSpec;
use crate::catalog::{Catalog, Table};
use crate::error::HeadError;
use crate::session::{SessionEffect, Settings};

mod acl;
mod definition;
mod float;
pub(crate) mod ident;
mod value;

pub(crate) fn policy_expression(typed: &Typed, table: &Table) -> Result<String, HeadError> {
    definition::policy_expression(typed, table)
}

/// The one place a `Value` becomes PostgreSQL wire text: an integer as
/// plain digits, everything else as its stored text, `NULL` as absent
/// (the wire's own NULL marker, not the text "null"). Every already-typed
/// value reaching here (a `render::rows` output, an inserted row) came
/// through a renderer that already produced the right text form for its
/// column (`render_bool`'s "t"/"f", say), so this never re-derives a
/// type-specific representation itself.
pub fn wire_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        value @ Value::Numeric(_) | value @ Value::Text(_) | value @ Value::Blob(_) => value
            .as_int()
            .map(|integer| integer.to_string())
            .or_else(|| value.to_text().map(str::to_string)),
    }
}

pub(crate) fn rows(
    specs: &[OutputSpec],
    physical_rows: Vec<Vec<Value>>,
    catalog: &Catalog,
    settings: &mut Settings,
    effects: &mut Vec<SessionEffect>,
) -> Result<Vec<Vec<Value>>, HeadError> {
    physical_rows
        .into_iter()
        .map(|row| render_row(specs, row, catalog, settings, effects))
        .collect()
}

fn render_row(
    specs: &[OutputSpec],
    row: Vec<Value>,
    catalog: &Catalog,
    settings: &mut Settings,
    effects: &mut Vec<SessionEffect>,
) -> Result<Vec<Value>, HeadError> {
    let mut values = row.into_iter();
    specs
        .iter()
        .map(|spec| render_one(spec, &mut values, catalog, settings, effects))
        .collect()
}

fn render_one(
    spec: &OutputSpec,
    values: &mut std::vec::IntoIter<Value>,
    catalog: &Catalog,
    settings: &mut Settings,
    effects: &mut Vec<SessionEffect>,
) -> Result<Value, HeadError> {
    match spec {
        OutputSpec::Column => next(values),
        OutputSpec::Bool => value::render_bool(next(values)?),
        OutputSpec::Float4 => value::render_float4(next(values)?),
        OutputSpec::RegClass => value::render_regclass(next(values)?, catalog),
        OutputSpec::RegProc => value::render_regproc(next(values)?),
        OutputSpec::AclArray => value::render_acl_array(next(values)?, catalog),
        OutputSpec::Vector => value::render_vector(next(values)?),
        OutputSpec::CatalogRendered(handle) => render_catalog_call(*handle, values, catalog),
        OutputSpec::SessionCall(handle) => render_session_call(*handle, values, settings, effects),
        OutputSpec::Unsupported(_) => Err(HeadError::internal(
            "lowering must refuse an unsupported output column before rows are rendered",
        )),
    }
}

fn render_session_call(
    handle: crate::analyze::functions::FunctionHandle,
    values: &mut std::vec::IntoIter<Value>,
    settings: &mut Settings,
    effects: &mut Vec<SessionEffect>,
) -> Result<Value, HeadError> {
    let arity = handle.arg_count()?;
    let mut raw = Vec::with_capacity(arity);
    for _ in 0..arity {
        raw.push(next(values)?);
    }
    crate::session::session_functions::evaluate_over_row(handle, &raw, settings, effects)
}

fn render_catalog_call(
    handle: crate::analyze::functions::FunctionHandle,
    values: &mut std::vec::IntoIter<Value>,
    catalog: &Catalog,
) -> Result<Value, HeadError> {
    let arity = definition::catalog_render_arity(handle);
    let mut raw = Vec::with_capacity(arity);
    for _ in 0..arity {
        raw.push(next(values)?);
    }
    definition::render_call(handle, &raw, catalog)
}

fn next(values: &mut std::vec::IntoIter<Value>) -> Result<Value, HeadError> {
    values.next().ok_or_else(|| {
        HeadError::internal("a physical row is missing a column its output shape expects")
    })
}
