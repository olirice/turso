use std::collections::BTreeSet;
use std::fmt::Write;

use crate::numeric::Numeric;
use crate::types::{ImmutableRecord, Value, ValueIterator};
use crate::{LimboError, Result};

/// The wire tag an array-membership coercion refusal carries, so a schema
/// dialect can recognize the family (`= ANY`/`@>`/`<@`/`&&`/
/// `array_position`) without sniffing free text: the refusal message is
/// `{MARKER}{human text}` inside a `LimboError::Constraint`. Core has no
/// structured error code of its own to attach, the same constraint that
/// shaped compiled triggers' RAISE tagging.
pub const ARRAY_ANY_COERCION_MARKER: &str = "\u{1}array-any-coercion\u{1}";

fn coercion_refusal(message: impl std::fmt::Display) -> LimboError {
    LimboError::Constraint(format!("{ARRAY_ANY_COERCION_MARKER}{message}"))
}

/// Coerce every array element to the probe's value class before the
/// membership family compares anything, refusing loudly where PostgreSQL's
/// own input functions would: a text element against a numeric probe
/// becomes a number (`id = ANY('{"1","3"}')` used to silently match
/// nothing) or refuses on the WHOLE array -- PostgreSQL parses the literal
/// before matching, so `'{"1","x"}'` errors even though `"1"` alone would
/// have matched. A numeric element against a text probe compares as text,
/// which is how PostgreSQL types the same untyped literal. A blob on
/// exactly one side has no comparison at all and refuses loudly.
fn coerce_elements_for_probe(elements: Vec<Value>, probe: &Value) -> Result<Vec<Value>> {
    let mismatch = || {
        coercion_refusal(
            "array element type does not match the comparison value's type on this \
             engine build; cast the array to the column's array type",
        )
    };
    elements
        .into_iter()
        .map(|element| {
            Ok(match (&element, probe) {
                (Value::Null, _) | (_, Value::Null) => element,
                (Value::Numeric(_), Value::Numeric(_))
                | (Value::Text(_), Value::Text(_))
                | (Value::Blob(_), Value::Blob(_)) => element,
                (Value::Text(t), Value::Numeric(_)) => coerce_text_to_numeric(t.as_str())?,
                (Value::Numeric(n), Value::Text(_)) => Value::build_text(numeric_as_text(n)),
                (Value::Blob(_), _) | (_, Value::Blob(_)) => return Err(mismatch()),
            })
        })
        .collect()
}

fn coerce_text_to_numeric(text: &str) -> Result<Value> {
    if let Ok(i) = text.trim().parse::<i64>() {
        return Ok(Value::from_i64(i));
    }
    if let Ok(f) = text.trim().parse::<f64>() {
        if f.is_finite() {
            return Ok(Value::from_f64(f));
        }
    }
    Err(coercion_refusal(format!(
        "invalid input syntax for type numeric: \"{text}\""
    )))
}

fn numeric_as_text(n: &Numeric) -> String {
    match n {
        Numeric::Integer(i) => i.to_string(),
        Numeric::Float(f) => f64::from(*f).to_string(),
    }
}

/// Extract values from a record-format array blob.
/// Returns Err if the blob is not a valid record.
/// Uses zero-copy iteration over the blob bytes — no Vec<u8> allocation.
pub(crate) fn array_values_from_blob(blob: &[u8]) -> Result<Vec<Value>> {
    let iter = ValueIterator::new(blob)?;
    let mut values = Vec::with_capacity(iter.size_hint().0);
    for value in iter {
        values.push(value?.to_owned()?);
    }
    Ok(values)
}

/// Extract elements from any Value that represents an array.
/// Handles record blobs, JSON text input, and NULL (empty array).
/// Returns None if the value cannot be interpreted as an array.
///
/// `pub` (with `parse_text_array`, `serialize_array_from_blob` and
/// `values_to_record_blob`) so schema dialects can implement array
/// consumers the core does not ship -- set-returning `unnest`, json
/// rendering of an array value, and array-returning introspection
/// (`array_positions`) all need to iterate, parse, render, or construct
/// an array from outside this crate, and the record blob format is
/// deliberately private in every other respect.
pub fn array_values_from_any(arr: &Value) -> Result<Option<Vec<Value>>> {
    Ok(match arr {
        Value::Blob(blob) => array_values_from_blob(blob).ok(),
        Value::Text(text) => parse_text_array(text.as_str())?,
        Value::Null => Some(Vec::new()),
        _ => None,
    })
}

/// Parse a text array literal in PG format `{1, hello, NULL}` into a Vec<Value>.
/// Handles integers, floats, strings (quoted and unquoted), and NULL.
pub fn parse_text_array(text: &str) -> Result<Option<Vec<Value>>> {
    let text = text.trim();
    if text.starts_with('{') && text.ends_with('}') {
        return parse_pg_text_array(text);
    }
    Ok(None)
}

/// Parse a PG-style text array like `{1, hello, NULL, 3.14}` into a Vec<Value>.
/// Unquoted `NULL` (case-insensitive) → Value::Null.
/// Quoted strings use `"..."` with `\"` and `\\` escapes.
/// Unquoted tokens are parsed as integer, then float, then text.
fn parse_pg_text_array(text: &str) -> Result<Option<Vec<Value>>> {
    let inner = text[1..text.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let bytes = inner.as_bytes();
    let mut pos = 0;
    let mut elements = Vec::new();

    loop {
        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        if bytes[pos] == b'{' {
            // A nested brace is a multidimensional array literal, which the
            // flat array model cannot represent. Anything but a refusal here
            // is a silent wrong answer: the old tokenizer read `{1` as a
            // TEXT element, so `2 = ANY('{{1,2},{3,4}}')` matched nothing
            // and an INSERT stored garbage elements. The one exception is
            // an all-empty nesting (`{ {} }`, `{{},{}}`): PostgreSQL says
            // the empty array has no dimensions at all and answers `{}`
            // for every such spelling (probed on 17.7).
            if inner
                .chars()
                .all(|c| c == '{' || c == '}' || c == ',' || c.is_whitespace())
            {
                return Ok(Some(Vec::new()));
            }
            return Err(LimboError::Constraint(
                "multidimensional array literals are not supported on this engine build"
                    .to_string(),
            ));
        }
        if bytes[pos] == b'"' {
            // Quoted string
            pos += 1;
            let mut s = String::new();
            loop {
                if pos >= bytes.len() {
                    return Ok(None);
                }
                match bytes[pos] {
                    b'\\' => {
                        pos += 1;
                        if pos >= bytes.len() {
                            return Ok(None);
                        }
                        match bytes[pos] {
                            b'n' => s.push('\n'),
                            b't' => s.push('\t'),
                            b'r' => s.push('\r'),
                            other => s.push(other as char),
                        }
                    }
                    b'"' => {
                        pos += 1;
                        break;
                    }
                    _ => {
                        let remaining = &inner[pos..];
                        let ch = remaining.chars().next().unwrap_or('\u{FFFD}');
                        s.push(ch);
                        pos += ch.len_utf8();
                        continue;
                    }
                }
                pos += 1;
            }
            elements.push(Value::build_text(s));
        } else {
            // Unquoted token: read until comma, whitespace, or end
            let start = pos;
            while pos < bytes.len() && bytes[pos] != b',' && !bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            let token = &inner[start..pos];
            if token.eq_ignore_ascii_case("null") {
                elements.push(Value::Null);
            } else if let Ok(i) = token.parse::<i64>() {
                elements.push(Value::from_i64(i));
            } else if let Ok(f) = token.parse::<f64>() {
                if !f.is_finite() {
                    return Ok(None); // reject Infinity and NaN
                }
                elements.push(Value::from_f64(f));
            } else {
                elements.push(Value::build_text(token.to_string()));
            }
        }

        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        if bytes[pos] == b',' {
            pos += 1;
            // Reject trailing commas: after consuming ',' there must be another element
            let mut peek = pos;
            while peek < bytes.len() && bytes[peek].is_ascii_whitespace() {
                peek += 1;
            }
            if peek >= bytes.len() {
                return Ok(None); // trailing comma
            }
        } else if pos < bytes.len() {
            return Ok(None);
        }
    }

    Ok(Some(elements))
}

/// Pack values into a record-format array blob.
pub fn values_to_record_blob(values: &[Value]) -> Result<Value> {
    Ok(Value::Blob(
        ImmutableRecord::from_values(values, values.len())?.into_payload(),
    ))
}

/// Serialize a record-format array blob to PostgreSQL text representation.
/// Uses `{...}` delimiters and PG quoting rules:
/// - NULL elements → uppercase `NULL` (unquoted)
/// - Text elements → double-quoted if they contain special chars, unquoted otherwise
/// - Numeric elements → unquoted
pub fn serialize_array_from_blob(blob: &[u8]) -> Result<String> {
    let iter = ValueIterator::new(blob)?;
    let mut result = String::from("{");
    let mut first = true;
    for vref in iter {
        let vref = vref?;
        if !first {
            result.push(',');
        }
        first = false;
        write_value_ref_pg(&mut result, &vref);
    }
    result.push('}');
    Ok(result)
}

fn write_value_ref_pg(result: &mut String, val: &crate::ValueRef<'_>) {
    match val {
        crate::ValueRef::Null => result.push_str("NULL"),
        crate::ValueRef::Numeric(Numeric::Integer(n)) => {
            let _ = write!(result, "{n}");
        }
        crate::ValueRef::Numeric(Numeric::Float(f)) => {
            let fval: f64 = (*f).into();
            // Normalize -0.0 to 0.0 for display
            let fval = if fval == 0.0 { 0.0 } else { fval };
            if fval.fract() == 0.0 && fval.is_finite() {
                let _ = write!(result, "{fval:.1}");
            } else {
                let _ = write!(result, "{fval}");
            }
        }
        crate::ValueRef::Text(t) => {
            write_pg_text_element(result, t.as_str());
        }
        crate::ValueRef::Blob(b) => {
            result.push_str("\"X'");
            for byte in *b {
                let _ = write!(result, "{byte:02X}");
            }
            result.push_str("'\"");
        }
    }
}

/// Write a text element in PG array format.
/// Simple values are unquoted; values with special chars are double-quoted.
fn write_pg_text_element(result: &mut String, s: &str) {
    let needs_quoting = s.is_empty()
        || s.eq_ignore_ascii_case("null")
        || s.contains(|c: char| {
            c == ','
                || c == '{'
                || c == '}'
                || c == '"'
                || c == '\\'
                || c.is_whitespace()
                || c.is_control()
        });
    if needs_quoting {
        result.push('"');
        for ch in s.chars() {
            match ch {
                '"' => result.push_str("\\\""),
                '\\' => result.push_str("\\\\"),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                c if c.is_control() => {
                    let _ = write!(result, "\\u{:04x}", c as u32);
                }
                c => result.push(c),
            }
        }
        result.push('"');
    } else {
        result.push_str(s);
    }
}

/// Compute the number of elements in an array value. Shared by
/// op_array_length (instruction) and ScalarFunc::ArrayLength (function).
/// Returns None for NULL or non-blob input (maps to SQL NULL).
pub(crate) fn compute_array_length(val: &Value) -> Result<Option<i64>> {
    Ok(match val {
        Value::Null => None,
        Value::Blob(b) => match ValueIterator::new(b) {
            Ok(iter) => Some(iter.count() as i64),
            Err(_) => None,
        },
        Value::Text(t) => parse_text_array(t.as_str())?.map(|v| v.len() as i64),
        _ => None,
    })
}

/// Compute the element count at array dimension `dim` (1-based). For `dim == 1`,
/// returns the same value as [`compute_array_length`]. For `dim > 1`, the array
/// is assumed to be uniform — each outer element is itself an array — and the
/// walker recurses into element zero `dim - 1` times.
///
/// Returns `None` for NULL or non-array input, for `dim < 1`, for an empty
/// array when `dim > 1` (no element zero to peek into), and for `dim` deeper
/// than the array's actual nesting. Matches PostgreSQL's
/// `array_length(arr, dim)` contract — except Turso doesn't track per-
/// dimension lower bounds, so `array_upper(arr, dim)` equals this function
/// for all valid `dim`.
pub(crate) fn compute_array_length_at_dim(val: &Value, dim: i64) -> Result<Option<i64>> {
    if dim < 1 {
        return Ok(None);
    }
    if dim == 1 {
        return compute_array_length(val);
    }
    // dim > 1: peek into element zero and recurse. Uniform-shape arrays let
    // us answer "length at depth N" by looking at any element at depth 1;
    // element zero is the cheapest to extract.
    let Some(first) = array_values_from_any(val)?.and_then(|v| v.into_iter().next()) else {
        return Ok(None);
    };
    compute_array_length_at_dim(&first, dim - 1)
}

pub(crate) fn exec_array_append(arr: &Value, elem: &Value) -> Result<Value> {
    let Some(mut elements) = array_values_from_any(arr)? else {
        return Ok(Value::Null);
    };
    elements.push(elem.clone());
    values_to_record_blob(&elements)
}

pub(crate) fn exec_array_prepend(arr: &Value, elem: &Value) -> Result<Value> {
    let Some(elements) = array_values_from_any(arr)? else {
        return Ok(Value::Null);
    };
    // Build new vec with elem first — avoids O(n) shift from Vec::insert(0, ...)
    let mut result = Vec::with_capacity(elements.len() + 1);
    result.push(elem.clone());
    result.extend(elements);
    values_to_record_blob(&result)
}

pub(crate) fn exec_array_cat(a: &Value, b: &Value) -> Result<Value> {
    if matches!(a, Value::Null) || matches!(b, Value::Null) {
        return Ok(Value::Null);
    }
    let Some(mut elems_a) = array_values_from_any(a)? else {
        return Ok(Value::Null);
    };
    let Some(elems_b) = array_values_from_any(b)? else {
        return Ok(Value::Null);
    };
    elems_a.extend(elems_b);
    values_to_record_blob(&elems_a)
}

pub(crate) fn exec_array_remove(arr: &Value, target: &Value) -> Result<Value> {
    if matches!(arr, Value::Null) {
        return Ok(Value::Null);
    }
    let Some(elements) = array_values_from_any(arr)? else {
        return Ok(Value::Null);
    };
    let result: Vec<Value> = elements.into_iter().filter(|e| e != target).collect();
    values_to_record_blob(&result)
}

pub(crate) fn exec_array_contains(arr: &Value, target: &Value) -> Result<Value> {
    if matches!(arr, Value::Null) {
        return Ok(Value::Null);
    }
    if let Value::Blob(blob) = arr {
        return Ok(array_find_streaming(blob, |vref| vref == *target)
            .map(|_| Value::from_i64(1))
            .unwrap_or_else(|| Value::from_i64(0)));
    }
    let Some(elements) = array_values_from_any(arr)? else {
        return Ok(Value::Null);
    };
    let elements = coerce_elements_for_probe(elements, target)?;
    let found = elements.iter().any(|e| e == target);
    Ok(Value::from_i64(found as i64))
}

pub(crate) fn exec_array_position(arr: &Value, target: &Value) -> Result<Value> {
    if matches!(arr, Value::Null) {
        return Ok(Value::Null);
    }
    if let Value::Blob(blob) = arr {
        return Ok(array_find_streaming(blob, |vref| vref == *target)
            .map(|i| Value::from_i64(i as i64 + 1)) // 1-based (PG convention)
            .unwrap_or(Value::Null));
    }
    let Some(elements) = array_values_from_any(arr)? else {
        return Ok(Value::Null);
    };
    let elements = coerce_elements_for_probe(elements, target)?;
    // `array_position` compares with IS NOT DISTINCT FROM (probed on 17.7:
    // a NULL probe finds a NULL element), which plain Value equality
    // already answers -- unlike the containment operators below, whose
    // element-equality semantics can never find a NULL.
    for (i, elem) in elements.iter().enumerate() {
        if elem == target {
            return Ok(Value::from_i64(i as i64 + 1)); // 1-based (PG convention)
        }
    }
    Ok(Value::Null)
}

/// Stream through a record-format blob, calling `predicate` on each element.
/// Returns Some(index) for the first element where the predicate returns true,
/// or None if no match or on error.
fn array_find_streaming(
    blob: &[u8],
    predicate: impl Fn(crate::ValueRef<'_>) -> bool,
) -> Option<usize> {
    let iter = ValueIterator::new(blob).ok()?;
    for (i, vref) in iter.enumerate() {
        let vref = vref.ok()?;
        if predicate(vref) {
            return Some(i);
        }
    }
    None
}

pub(crate) fn exec_array_slice(arr: &Value, start: &Value, end: &Value) -> Result<Value> {
    if matches!(arr, Value::Null) {
        return Ok(Value::Null);
    }
    let Some(elements) = array_values_from_any(arr)? else {
        return Ok(Value::Null);
    };
    // PG convention: 1-based inclusive bounds
    let start_idx = match start {
        Value::Numeric(Numeric::Integer(i)) if *i >= 1 => (*i - 1) as usize,
        _ => 0,
    };
    let end_idx = match end {
        Value::Numeric(Numeric::Integer(i)) if *i >= 1 => *i as usize, // inclusive → exclusive
        _ => 0,
    };
    let end = end_idx.min(elements.len());
    let start = start_idx.min(end);
    values_to_record_blob(&elements[start..end])
}

/// Split a string into an array using a delimiter.
/// string_to_array(text, delimiter [, null_string])
/// If text is NULL, returns NULL.
/// If delimiter is NULL, splits into individual characters (PostgreSQL behavior).
/// If null_string is provided, any element matching it becomes NULL.
pub(crate) fn exec_string_to_array(
    text: &Value,
    delimiter: &Value,
    null_str: Option<&Value>,
) -> Result<Value> {
    let text_str = match text {
        Value::Text(t) => t.as_str().to_string(),
        Value::Null => return Ok(Value::Null),
        other => other.to_string(),
    };

    let null_match: Option<String> = match null_str {
        Some(Value::Text(t)) => Some(t.as_str().to_string()),
        Some(Value::Null) | None => None,
        Some(other) => Some(other.to_string()),
    };

    // NULL delimiter: split into individual characters (PostgreSQL behavior)
    if matches!(delimiter, Value::Null) {
        let values: Vec<Value> = text_str
            .chars()
            .map(|c| {
                let s = c.to_string();
                if let Some(ref nm) = null_match {
                    if s == *nm {
                        return Value::Null;
                    }
                }
                Value::build_text(s)
            })
            .collect();
        return values_to_record_blob(&values);
    }

    let delim_str = match delimiter {
        Value::Text(d) => d.as_str().to_string(),
        other => other.to_string(),
    };

    let parts: Vec<&str> = if delim_str.is_empty() {
        // Empty delimiter: return single-element array with the whole string
        vec![&text_str]
    } else {
        text_str.split(&delim_str).collect()
    };

    let values: Vec<Value> = parts
        .into_iter()
        .map(|p| {
            if let Some(ref nm) = null_match {
                if p == nm.as_str() {
                    return Value::Null;
                }
            }
            Value::build_text(p.to_string())
        })
        .collect();

    values_to_record_blob(&values)
}

/// Join array elements into a string with a delimiter.
/// array_to_string(array, delimiter [, null_string])
/// NULL elements are omitted unless null_string is provided.
pub(crate) fn exec_array_to_string(
    arr: &Value,
    delimiter: &Value,
    null_str: Option<&Value>,
) -> Result<Value> {
    if matches!(arr, Value::Null) {
        return Ok(Value::Null);
    }

    let delim = match delimiter {
        Value::Text(t) => t.as_str().to_string(),
        Value::Null => return Ok(Value::Null),
        other => other.to_string(),
    };

    let null_replacement: Option<String> = match null_str {
        Some(Value::Text(t)) => Some(t.as_str().to_string()),
        Some(Value::Null) | None => None,
        Some(other) => Some(other.to_string()),
    };

    // Fast path: stream from blob without materializing Vec<Value>
    if let Value::Blob(blob) = arr {
        if let Ok(iter) = ValueIterator::new(blob) {
            let mut result = String::new();
            let mut first = true;
            for vref in iter {
                let Ok(vref) = vref else {
                    return Ok(Value::Null);
                };
                let part = match &vref {
                    crate::ValueRef::Null => {
                        if let Some(ref replacement) = null_replacement {
                            replacement.clone()
                        } else {
                            continue;
                        }
                    }
                    crate::ValueRef::Text(t) => t.as_str().to_string(),
                    other => format!("{other}"),
                };
                if !first {
                    result.push_str(&delim);
                }
                result.push_str(&part);
                first = false;
            }
            return Ok(Value::build_text(result));
        }
    }

    let Some(elements) = array_values_from_any(arr)? else {
        return Ok(Value::Null);
    };

    let mut result = String::new();
    let mut first = true;
    for elem in &elements {
        let part = match elem {
            Value::Null => {
                if let Some(ref replacement) = null_replacement {
                    replacement.clone()
                } else {
                    continue;
                }
            }
            Value::Text(t) => t.as_str().to_string(),
            other => other.to_string(),
        };
        if !first {
            result.push_str(&delim);
        }
        result.push_str(&part);
        first = false;
    }

    Ok(Value::build_text(result))
}

/// Check if two arrays have any elements in common.
/// Returns 1 if they share at least one element, 0 otherwise.
/// NULL if either input is not a valid array.
pub(crate) fn exec_array_overlap(a: &Value, b: &Value, nulls_never_match: bool) -> Result<Value> {
    if matches!(a, Value::Null) || matches!(b, Value::Null) {
        return Ok(Value::Null);
    }
    let Some(elems_a) = array_values_from_any(a)? else {
        return Ok(Value::Null);
    };
    let Some(elems_b) = array_values_from_any(b)? else {
        return Ok(Value::Null);
    };
    let (elems_a, elems_b) = coerce_sides(elems_a, elems_b)?;
    // O(n log n + m log n) via BTreeSet instead of O(n*m). Under
    // `nulls_never_match` (Connection::set_array_nulls_never_match,
    // PostgreSQL's element-equality semantics, probed on 17.7) a NULL
    // element matches nothing; the default keeps the engine's own pinned
    // NULL-matching behavior (turso-sqltests/array-edge-cases.sqltest).
    let set: BTreeSet<&Value> = elems_a.iter().collect();
    let found = elems_b
        .iter()
        .any(|eb| (!nulls_never_match || !matches!(eb, Value::Null)) && set.contains(eb));
    Ok(Value::from_i64(found as i64))
}

/// Check if array `a` contains all elements of array `b` (@> operator).
/// Returns 1 if every element in `b` appears in `a`, 0 otherwise.
/// NULL if either input is not a valid array.
pub(crate) fn exec_array_contains_all(
    a: &Value,
    b: &Value,
    nulls_never_match: bool,
) -> Result<Value> {
    if matches!(a, Value::Null) || matches!(b, Value::Null) {
        return Ok(Value::Null);
    }
    let Some(elems_a) = array_values_from_any(a)? else {
        return Ok(Value::Null);
    };
    let Some(elems_b) = array_values_from_any(b)? else {
        return Ok(Value::Null);
    };
    let (elems_a, elems_b) = coerce_sides(elems_a, elems_b)?;
    // O(n log n + m log n) via BTreeSet instead of O(n*m). Under
    // `nulls_never_match` (PostgreSQL: `@>` is built on element equality
    // and NULL = NULL is unknown, probed on 17.7) a NULL element in `b`
    // makes containment FALSE, never vacuously true; the default keeps the
    // engine's own pinned NULL-matching behavior.
    let set: BTreeSet<&Value> = elems_a.iter().collect();
    let all_found = elems_b
        .iter()
        .all(|eb| !(nulls_never_match && matches!(eb, Value::Null)) && set.contains(eb));
    Ok(Value::from_i64(all_found as i64))
}

/// The two-array analogue of [`coerce_elements_for_probe`]: when one side
/// is numeric-bearing and the other text-bearing, every text element on
/// both sides coerces to a number (refusing loudly on the whole statement
/// where PostgreSQL's input functions would); a blob on one side against
/// text or numeric on the other refuses outright. Same-class inputs pass
/// through untouched.
fn coerce_sides(a: Vec<Value>, b: Vec<Value>) -> Result<(Vec<Value>, Vec<Value>)> {
    fn classes(values: &[Value]) -> (bool, bool, bool) {
        let mut c = (false, false, false);
        for v in values {
            match v {
                Value::Numeric(_) => c.0 = true,
                Value::Text(_) => c.1 = true,
                Value::Blob(_) => c.2 = true,
                _ => {}
            }
        }
        c
    }
    let ca = classes(&a);
    let cb = classes(&b);
    let blob_mix = (ca.2 && (cb.0 || cb.1)) || (cb.2 && (ca.0 || ca.1));
    if blob_mix {
        return Err(coercion_refusal(
            "array element type does not match the other array's element type on this \
             engine build; cast both arrays to the same array type",
        ));
    }
    let numeric_text_mix = (ca.0 && cb.1) || (ca.1 && cb.0);
    if !numeric_text_mix {
        return Ok((a, b));
    }
    let coerce = |values: Vec<Value>| -> Result<Vec<Value>> {
        values
            .into_iter()
            .map(|v| match v {
                Value::Text(t) => coerce_text_to_numeric(t.as_str()),
                other => Ok(other),
            })
            .collect()
    };
    Ok((coerce(a)?, coerce(b)?))
}

/// Collect values from contiguous registers into a record-format array blob.
pub(crate) fn make_array_from_registers(
    registers: &[super::Register],
    start_reg: usize,
    count: usize,
) -> Result<Value> {
    let record = ImmutableRecord::from_registers(&registers[start_reg..start_reg + count], count)?;
    Ok(Value::Blob(record.into_payload()))
}

/// Element-wise comparison of two record-format array blobs.
/// Compares corresponding elements using ValueRef ordering.
/// If all common elements are equal, the shorter array is less.
/// Returns Err if either blob is not a valid record.
pub(crate) fn compare_arrays(a: &[u8], b: &[u8]) -> Result<std::cmp::Ordering> {
    let iter_a = ValueIterator::new(a)?;
    let iter_b = ValueIterator::new(b)?;
    let mut count_a = 0usize;
    let mut count_b = 0usize;
    for (va, vb) in iter_a.zip(iter_b) {
        count_a += 1;
        count_b += 1;
        let (va, vb) = (va?, vb?);
        let ord = va.cmp(&vb);
        if !ord.is_eq() {
            return Ok(ord);
        }
    }
    // Count remaining elements in the longer array
    //TODO don't start another iterator, because it will deserialize the whole record again.
    let len_a = count_a + ValueIterator::new(a)?.skip(count_a).count();
    let len_b = count_b + ValueIterator::new(b)?.skip(count_b).count();
    Ok(len_a.cmp(&len_b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_array_multibyte_utf8() {
        let input = r#"{"café","naïve","über"}"#;
        let result = parse_text_array(input).unwrap().unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], Value::build_text("café"));
        assert_eq!(result[1], Value::build_text("naïve"));
        assert_eq!(result[2], Value::build_text("über"));
    }

    #[test]
    fn test_parse_text_array_emoji() {
        let input = r#"{"hello 🌍","test 🚀"}"#;
        let result = parse_text_array(input).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], Value::build_text("hello 🌍"));
        assert_eq!(result[1], Value::build_text("test 🚀"));
    }

    #[test]
    fn test_parse_text_array_cjk() {
        let input = r#"{"你好","世界"}"#;
        let result = parse_text_array(input).unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], Value::build_text("你好"));
        assert_eq!(result[1], Value::build_text("世界"));
    }

    #[test]
    fn test_compute_array_length_null_returns_none() {
        assert_eq!(compute_array_length(&Value::Null).unwrap(), None);
    }

    #[test]
    fn test_compute_array_length_valid_array() {
        let blob = values_to_record_blob(&[Value::from_i64(1), Value::from_i64(2)]).unwrap();
        assert_eq!(compute_array_length(&blob).unwrap(), Some(2));
    }

    #[test]
    fn test_compute_array_length_non_blob_returns_none() {
        assert_eq!(compute_array_length(&Value::from_i64(42)).unwrap(), None,);
    }

    #[test]
    fn test_array_remove_all_occurrences() {
        let arr = values_to_record_blob(&[
            Value::from_i64(1),
            Value::from_i64(2),
            Value::from_i64(3),
            Value::from_i64(2),
            Value::from_i64(1),
        ])
        .unwrap();
        let result = exec_array_remove(&arr, &Value::from_i64(2)).unwrap();
        let Value::Blob(blob) = &result else {
            panic!("Expected Blob");
        };
        let elements = array_values_from_blob(blob).unwrap();
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0], Value::from_i64(1));
        assert_eq!(elements[1], Value::from_i64(3));
        assert_eq!(elements[2], Value::from_i64(1));
    }

    #[test]
    fn test_array_contains_null_array_returns_null() {
        assert_eq!(
            exec_array_contains(&Value::Null, &Value::from_i64(1)).unwrap(),
            Value::Null,
        );
    }

    #[test]
    fn test_array_position_null_array_returns_null() {
        assert_eq!(
            exec_array_position(&Value::Null, &Value::from_i64(1)).unwrap(),
            Value::Null,
        );
    }

    #[test]
    fn test_compute_array_length_invalid_blob_returns_none() {
        // A random blob that is not a valid record should return None
        let invalid = Value::from_slice(&[0xFF, 0xFE, 0xFD]).expect(crate::alloc::ALLOC_ERR_MSG);
        assert_eq!(compute_array_length(&invalid).unwrap(), None);
    }

    #[test]
    fn test_parse_text_array_rejects_json_format() {
        // JSON [1,2,3] format is no longer accepted — only PG {1,2,3}
        assert!(parse_text_array("[1,2,3]").unwrap().is_none());
        assert!(parse_text_array(r#"["hello"]"#).unwrap().is_none());
    }

    #[test]
    fn test_parse_text_array_rejects_trailing_comma() {
        assert!(parse_text_array("{1,2,}").unwrap().is_none());
        assert!(parse_text_array("{1, 2, }").unwrap().is_none());
    }

    #[test]
    fn test_parse_text_array_rejects_infinity() {
        assert!(parse_text_array("{1e309}").unwrap().is_none());
        assert!(parse_text_array("{-1e309}").unwrap().is_none());
    }

    #[test]
    fn test_string_to_array_null_delimiter_splits_chars() {
        let result = exec_string_to_array(&Value::build_text("hello"), &Value::Null, None).unwrap();
        let Value::Blob(blob) = &result else {
            panic!("Expected Blob, got {result:?}");
        };
        let elements = array_values_from_blob(blob).unwrap();
        assert_eq!(elements.len(), 5);
        assert_eq!(elements[0], Value::build_text("h"));
        assert_eq!(elements[1], Value::build_text("e"));
        assert_eq!(elements[4], Value::build_text("o"));
    }

    #[test]
    fn test_exec_array_contains_streaming() {
        let arr = values_to_record_blob(&[
            Value::from_i64(10),
            Value::from_i64(20),
            Value::from_i64(30),
        ])
        .unwrap();
        assert_eq!(
            exec_array_contains(&arr, &Value::from_i64(20)).unwrap(),
            Value::from_i64(1)
        );
        assert_eq!(
            exec_array_contains(&arr, &Value::from_i64(99)).unwrap(),
            Value::from_i64(0)
        );
    }

    #[test]
    fn test_exec_array_position_streaming() {
        let arr = values_to_record_blob(&[
            Value::from_i64(10),
            Value::from_i64(20),
            Value::from_i64(30),
        ])
        .unwrap();
        // 1-based: element 20 is at position 2
        assert_eq!(
            exec_array_position(&arr, &Value::from_i64(20)).unwrap(),
            Value::from_i64(2)
        );
        assert_eq!(
            exec_array_position(&arr, &Value::from_i64(99)).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn test_dc1_negative_index_preserves_array() {
        let arr = values_to_record_blob(&[
            Value::from_i64(10),
            Value::from_i64(20),
            Value::from_i64(30),
        ])
        .unwrap();
        // array_find_streaming with impossible predicate should return None
        let Value::Blob(blob) = &arr else {
            panic!("Expected Blob");
        };
        assert!(array_find_streaming(blob, |_| false).is_none());
    }

    #[test]
    fn test_dc4_array_remove_null_returns_null() {
        assert_eq!(
            exec_array_remove(&Value::Null, &Value::from_i64(1)).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn test_dc4_array_slice_null_returns_null() {
        assert_eq!(
            exec_array_slice(&Value::Null, &Value::from_i64(0), &Value::from_i64(2)).unwrap(),
            Value::Null,
        );
    }

    #[test]
    fn test_dc4_array_cat_null_returns_null() {
        assert_eq!(
            exec_array_cat(&Value::Null, &Value::Null).unwrap(),
            Value::Null
        );
        assert_eq!(
            exec_array_cat(&Value::Null, &Value::from_i64(1)).unwrap(),
            Value::Null,
        );
    }

    #[test]
    fn test_serialize_array_from_blob() {
        let arr =
            values_to_record_blob(&[Value::from_i64(1), Value::build_text("hello"), Value::Null])
                .unwrap();
        let Value::Blob(blob) = &arr else {
            panic!("Expected Blob");
        };
        let text = serialize_array_from_blob(blob).unwrap();
        assert_eq!(text, "{1,hello,NULL}");
    }

    #[test]
    fn test_make_array_from_registers() {
        use super::super::Register;
        let registers = vec![
            Register::Value(Value::from_i64(1)),
            Register::Value(Value::build_text("two")),
            Register::Value(Value::from_i64(3)),
        ];
        let result = make_array_from_registers(&registers, 0, 3).unwrap();
        let Value::Blob(blob) = &result else {
            panic!("Expected Blob");
        };
        let elements = array_values_from_blob(blob).unwrap();
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0], Value::from_i64(1));
        assert_eq!(elements[1], Value::build_text("two"));
        assert_eq!(elements[2], Value::from_i64(3));
    }

    #[test]
    fn a_multidimensional_text_literal_refuses_loudly() {
        // The flat model cannot represent it; the old tokenizer read `{1`
        // as a text element and every membership check silently missed.
        let err = parse_text_array("{{1,2},{3,4}}").unwrap_err();
        assert!(
            err.to_string().contains("multidimensional"),
            "expected the multidimensional refusal, got: {err}"
        );
        let err = exec_array_contains(&Value::build_text("{{1,2},{3,4}}"), &Value::from_i64(2))
            .unwrap_err();
        assert!(err.to_string().contains("multidimensional"));
    }

    #[test]
    fn quoted_text_elements_coerce_to_a_numeric_probe() {
        // `id = ANY('{"1","3"}')` used to silently match nothing: the text
        // element never equaled the integer probe.
        let arr = Value::build_text(r#"{"1","3"}"#);
        assert_eq!(
            exec_array_contains(&arr, &Value::from_i64(3)).unwrap(),
            Value::from_i64(1)
        );
        assert_eq!(
            exec_array_contains(&arr, &Value::from_i64(2)).unwrap(),
            Value::from_i64(0)
        );
        assert_eq!(
            exec_array_position(&arr, &Value::from_i64(3)).unwrap(),
            Value::from_i64(2)
        );
    }

    #[test]
    fn an_unparseable_text_element_refuses_the_whole_array() {
        // PostgreSQL parses the literal before matching, so `{"1","x"}`
        // errors even though "1" alone would have matched.
        let arr = Value::build_text(r#"{"1","x"}"#);
        let err = exec_array_contains(&arr, &Value::from_i64(1)).unwrap_err();
        assert!(
            err.to_string().contains(ARRAY_ANY_COERCION_MARKER)
                && err.to_string().contains("invalid input syntax"),
            "expected the marked coercion refusal, got: {err}"
        );
    }

    #[test]
    fn a_blob_probe_against_text_elements_refuses_loudly() {
        let arr = Value::build_text("{a,b}");
        let probe = Value::from_slice(&[1, 2, 3]).expect(crate::alloc::ALLOC_ERR_MSG);
        let err = exec_array_contains(&arr, &probe).unwrap_err();
        assert!(err.to_string().contains(ARRAY_ANY_COERCION_MARKER));
    }

    #[test]
    fn mixed_class_overlap_coerces_or_refuses() {
        let nums = values_to_record_blob(&[Value::from_i64(1), Value::from_i64(2)]).unwrap();
        let texts = Value::build_text(r#"{"2","5"}"#);
        assert_eq!(
            exec_array_overlap(&nums, &texts, true).unwrap(),
            Value::from_i64(1)
        );
        let bad = Value::build_text(r#"{"2","x"}"#);
        assert!(exec_array_overlap(&nums, &bad, true).is_err());
    }

    #[test]
    fn an_all_empty_nesting_is_the_empty_array() {
        // PostgreSQL: the empty array has no dimensions at all, so no
        // spelling of it is refused as multidimensional (probed on 17.7:
        // `{ {} }`::int[] answers {}).
        assert_eq!(
            parse_text_array("{ {} }").unwrap().unwrap(),
            Vec::<Value>::new()
        );
        assert_eq!(
            parse_text_array("{{},{}}").unwrap().unwrap(),
            Vec::<Value>::new()
        );
    }

    #[test]
    fn null_semantics_match_postgres() {
        // array_position compares IS NOT DISTINCT FROM: a NULL probe finds
        // a NULL element (probed on 17.7).
        let arr = Value::build_text("{a,NULL,c}");
        assert_eq!(
            exec_array_position(&arr, &Value::Null).unwrap(),
            Value::from_i64(2)
        );
        // @> is built on element equality and NULL = NULL is unknown, so a
        // NULL element in the contained side is NEVER found.
        let a = values_to_record_blob(&[Value::from_i64(1), Value::from_i64(2)]).unwrap();
        let b = values_to_record_blob(&[Value::Null]).unwrap();
        assert_eq!(
            exec_array_contains_all(&a, &b, true).unwrap(),
            Value::from_i64(0)
        );
        // ...not even when the containing side holds a NULL of its own.
        let a_with_null = values_to_record_blob(&[Value::from_i64(1), Value::Null]).unwrap();
        assert_eq!(
            exec_array_contains_all(&a_with_null, &b, true).unwrap(),
            Value::from_i64(0)
        );
        // && never matches through NULLs either.
        assert_eq!(
            exec_array_overlap(&a_with_null, &b, true).unwrap(),
            Value::from_i64(0)
        );
        // The engine's own DEFAULT keeps NULL-matching containment
        // (turso-sqltests/array-edge-cases.sqltest pins it), including
        // the shape those pins do not cover: a NULL in `b` with no NULL
        // in `a` is NOT vacuously contained.
        assert_eq!(
            exec_array_contains_all(&a_with_null, &b, false).unwrap(),
            Value::from_i64(1)
        );
        assert_eq!(
            exec_array_overlap(&a_with_null, &b, false).unwrap(),
            Value::from_i64(1)
        );
        assert_eq!(
            exec_array_contains_all(&a, &b, false).unwrap(),
            Value::from_i64(0)
        );
    }
}
