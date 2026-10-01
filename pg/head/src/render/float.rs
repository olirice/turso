use crate::error::HeadError;

/// `float4`'s own precision (`FLT_DIG`): the decimal exponent at or above
/// which PostgreSQL 18 switches a `float4` to scientific notation.
const FLOAT4_PRECISION: i32 = 6;

/// Which of PostgreSQL's non-ordinary float shapes (or none) a value is,
/// so `format_shortest` below takes one `bool` (the sign), not four.
enum FloatClass {
    Zero,
    Infinite,
    Nan,
    Finite,
}

fn classify(is_nan: bool, is_infinite: bool, is_zero: bool) -> FloatClass {
    if is_nan {
        FloatClass::Nan
    } else if is_infinite {
        FloatClass::Infinite
    } else if is_zero {
        FloatClass::Zero
    } else {
        FloatClass::Finite
    }
}

/// PostgreSQL 18's float4 text output: the shortest decimal digit string
/// that round-trips to the same value (PostgreSQL's algorithm since 12,
/// with `extra_float_digits` defaulting to 1), in scientific notation once
/// the decimal exponent is below -4 or at least `FLOAT4_PRECISION`, and
/// fixed-point otherwise. Rust's `{:e}` already yields the shortest
/// round-trip digits; this only lays them out as PostgreSQL does.
pub(super) fn format_float4(value: f32) -> Result<String, HeadError> {
    format_shortest(
        classify(value.is_nan(), value.is_infinite(), value == 0.0),
        value.is_sign_negative(),
        &format!("{:e}", value.abs()),
        FLOAT4_PRECISION,
    )
}

fn format_shortest(
    class: FloatClass,
    is_negative: bool,
    mantissa_exp: &str,
    precision: i32,
) -> Result<String, HeadError> {
    match class {
        FloatClass::Nan => return Ok("NaN".to_string()),
        FloatClass::Infinite => {
            return Ok(if is_negative { "-Infinity" } else { "Infinity" }.to_string())
        }
        FloatClass::Zero => return Ok(if is_negative { "-0" } else { "0" }.to_string()),
        FloatClass::Finite => {}
    }
    let sign = if is_negative { "-" } else { "" };
    let (mantissa, exponent_text) = mantissa_exp.split_once('e').ok_or_else(|| {
        HeadError::internal("Rust's `{:e}` float formatting always contains an 'e'")
    })?;
    let exponent: i32 = exponent_text.parse().map_err(|_| {
        HeadError::internal("Rust's `{:e}` float formatting always has an integer exponent")
    })?;
    let digits: String = mantissa.chars().filter(|digit| *digit != '.').collect();
    if exponent < -4 || exponent >= precision {
        return Ok(format!(
            "{sign}{}e{}{:02}",
            scientific_mantissa(&digits),
            if exponent < 0 { '-' } else { '+' },
            exponent.abs()
        ));
    }
    if exponent >= 0 {
        let int_len = usize::try_from(exponent + 1)
            .map_err(|_| HeadError::internal("a non-negative exponent plus one is non-negative"))?;
        if digits.len() <= int_len {
            let mut whole = digits;
            whole.push_str(&"0".repeat(int_len - whole.len()));
            return Ok(format!("{sign}{whole}"));
        }
        let whole = digits.get(..int_len).unwrap_or("");
        let fraction = digits.get(int_len..).unwrap_or("");
        return Ok(format!("{sign}{whole}.{fraction}"));
    }
    let leading_zeros = usize::try_from(-exponent - 1).map_err(|_| {
        HeadError::internal("an exponent below -4 makes `-exponent - 1` non-negative")
    })?;
    Ok(format!("{sign}0.{}{digits}", "0".repeat(leading_zeros)))
}

/// `D` alone if there is only one significant digit, else `D.DDD...`.
fn scientific_mantissa(digits: &str) -> String {
    let mut chars = digits.chars();
    let Some(first) = chars.next() else {
        return digits.to_string();
    };
    let rest = chars.as_str();
    if rest.is_empty() {
        first.to_string()
    } else {
        format!("{first}.{rest}")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// `select x::float4::text` run against a live PostgreSQL 18 for each
    /// literal below (`postgres/conformance/head/pg_catalog.sql`'s own
    /// `reltuples` case corpus-tests the two values (`-1`, `0`) this head
    /// ever actually produces through admitted SQL; `float4` is not a
    /// value-castable type here, so every other spread below, can
    /// only be reached by calling the renderer directly, the same as
    /// `render_identifier_matches_postgres_18_quote_ident` in
    /// `render/ident.rs` probes its own renderer inline rather than
    /// through a `.sql` transcript).
    #[test]
    fn format_float4_matches_postgresql_18() {
        let cases: &[(f32, &str)] = &[
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (100.0, "100"),
            (0.1, "0.1"),
            (123.456, "123.456"),
            (1e10, "1e+10"),
            (1e-10, "1e-10"),
            (std::f32::consts::PI, "3.1415927"),
            (1.5e38, "1.5e+38"),
            (1.5e-38, "1.5e-38"),
            (f32::INFINITY, "Infinity"),
            (f32::NEG_INFINITY, "-Infinity"),
            (f32::NAN, "NaN"),
            (16777217.0, "1.6777216e+07"),
            (9999999.0, "9.999999e+06"),
            (0.00001234, "1.234e-05"),
            (1234567.0, "1.234567e+06"),
            (100000.0, "100000"),
            (1000000.0, "1e+06"),
            (10000000.0, "1e+07"),
            (123456789.0, "1.2345679e+08"),
            (999999.0, "999999"),
            (999999.9, "999999.9"),
            (100000.5, "100000.5"),
            (0.001, "0.001"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (0.000001, "1e-06"),
            (-100000.5, "-100000.5"),
            (-0.00001234, "-1.234e-05"),
            (3.4028235e38, "3.4028235e+38"),
            (1.1754944e-38, "1.1754944e-38"),
            (2147483647.0, "2.1474836e+09"),
            (123_456.79, "123456.79"),
            (999999.99, "1e+06"),
            (0.00009999, "9.999e-05"),
        ];
        for (value, expected) in cases {
            assert_eq!(
                format_float4(*value).expect("a finite or special f32 always formats"),
                *expected,
                "float4 {value}"
            );
        }
    }
}
