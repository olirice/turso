use turso_core::Value;

use crate::analyze::functions::FunctionHandle;
use crate::analyze::types::{BOOL_OID, INT4_OID, NAME_OID};
use crate::analyze::typing::{Scalar, Typed};
use crate::catalog::{Catalog, Table};
use crate::error::{HeadError, NotSupportedFeature, PolicyExpressionForm};
use crate::ident::TableName;
use crate::parse::expr::Truth;

use super::ident::{self, RenderedIdent};

pub(super) fn catalog_render_arity(handle: FunctionHandle) -> usize {
    match handle {
        FunctionHandle::PgGetConstraintdef | FunctionHandle::PgGetIndexdef => 1,
        FunctionHandle::PgGetExpr | FunctionHandle::PgGetConstraintdefPretty => 2,
        FunctionHandle::PgGetIndexdefColumn => 3,
        FunctionHandle::CurrentDatabase
        | FunctionHandle::CurrentSchemas
        | FunctionHandle::CurrentSetting
        | FunctionHandle::SetConfig
        | FunctionHandle::CurrentSettingMissingOk
        | FunctionHandle::PgIsInRecovery
        | FunctionHandle::Unnest
        | FunctionHandle::PgOptionsToTable
        | FunctionHandle::GenerateSeries
        | FunctionHandle::HeapTableamHandler
        | FunctionHandle::Bthandler
        | FunctionHandle::Hashhandler
        | FunctionHandle::Gisthandler
        | FunctionHandle::Ginhandler
        | FunctionHandle::Brinhandler
        | FunctionHandle::Spghandler
        | FunctionHandle::ArrayUpper
        | FunctionHandle::ArrayRemove
        | FunctionHandle::ArrayToString
        | FunctionHandle::QuoteIdent
        | FunctionHandle::QuoteLiteral
        | FunctionHandle::FormatType
        | FunctionHandle::AclDefault
        | FunctionHandle::PgGetTriggerdef
        | FunctionHandle::ArrayAgg => 0,
    }
}

fn primary_key_by_constraint(
    catalog: &Catalog,
    target: i64,
) -> Option<(&TableName, &Table, &crate::catalog::PrimaryKey)> {
    catalog
        .tables_by_oid()
        .into_iter()
        .find_map(|(name, table)| {
            let key = table.primary_key.as_ref()?;
            (key.constraint_oid.as_i64() == target).then_some((name, table, key))
        })
}

fn primary_key_by_index(
    catalog: &Catalog,
    target: i64,
) -> Option<(&TableName, &Table, &crate::catalog::PrimaryKey)> {
    catalog
        .tables_by_oid()
        .into_iter()
        .find_map(|(name, table)| {
            let key = table.primary_key.as_ref()?;
            (key.index_oid.as_i64() == target).then_some((name, table, key))
        })
}

fn not_null_by_constraint(
    catalog: &Catalog,
    target: i64,
) -> Option<(&Table, &crate::catalog::NotNullConstraint)> {
    catalog.tables_by_oid().into_iter().find_map(|(_, table)| {
        table
            .not_null_constraints
            .iter()
            .find(|constraint| constraint.constraint_oid.as_i64() == target)
            .map(|constraint| (table, constraint))
    })
}

fn value_oid(value: &Value) -> Option<i64> {
    value.as_int()
}

fn index_def_text(
    table_name: &TableName,
    key: &crate::catalog::PrimaryKey,
    column: &RenderedIdent,
) -> String {
    format!(
        "CREATE UNIQUE INDEX {} ON {} USING btree ({})",
        key.name.render().as_sql(),
        ident::render_qualified("public", table_name.as_str()).as_sql(),
        column.as_sql()
    )
}

fn constraint_def_text(column: &RenderedIdent) -> String {
    format!("PRIMARY KEY ({})", column.as_sql())
}

/// PostgreSQL 18's own `pg_get_constraintdef` for a not-null constraint:
/// the bare column name, no parentheses, unlike a primary key's.
fn not_null_def_text(column: &RenderedIdent) -> String {
    format!("NOT NULL {}", column.as_sql())
}

pub(super) fn render_call(
    handle: FunctionHandle,
    raw: &[Value],
    catalog: &Catalog,
) -> Result<Value, HeadError> {
    match (handle, raw) {
        (FunctionHandle::PgGetConstraintdef, [oid_value])
        | (FunctionHandle::PgGetConstraintdefPretty, [oid_value, _]) => {
            let Some(target) = value_oid(oid_value) else {
                return Ok(Value::Null);
            };
            Ok(match primary_key_by_constraint(catalog, target) {
                Some((_, table, key)) => {
                    let column = table.column_at(key.attnum)?.name.render();
                    text(&constraint_def_text(&column))
                }
                None => match not_null_by_constraint(catalog, target) {
                    Some((table, not_null)) => {
                        let column = table.column_at(not_null.attnum)?.name.render();
                        text(&not_null_def_text(&column))
                    }
                    None => Value::Null,
                },
            })
        }
        (FunctionHandle::PgGetIndexdef, [oid_value]) => {
            let Some(target) = value_oid(oid_value) else {
                return Ok(Value::Null);
            };
            Ok(match primary_key_by_index(catalog, target) {
                Some((name, table, key)) => {
                    let column = table.column_at(key.attnum)?.name.render();
                    text(&index_def_text(name, key, &column))
                }
                None => Value::Null,
            })
        }
        (FunctionHandle::PgGetIndexdefColumn, [oid_value, colno_value, _pretty]) => {
            let Some(target) = value_oid(oid_value) else {
                return Ok(Value::Null);
            };
            let Some((name, table, key)) = primary_key_by_index(catalog, target) else {
                return Ok(Value::Null);
            };
            let column = table.column_at(key.attnum)?.name.render();
            Ok(match value_oid(colno_value) {
                None | Some(0) => text(&index_def_text(name, key, &column)),
                Some(1) => text(column.as_sql()),
                Some(_) => text(""),
            })
        }
        (FunctionHandle::PgGetExpr, [tree_value, relid_value]) => match tree_value {
            Value::Null => Ok(Value::Null),
            Value::Text(stored) => {
                if value_oid(relid_value).is_none() {
                    return Ok(Value::Null);
                }
                Ok(text(stored.as_str()))
            }
            Value::Numeric(_) | Value::Blob(_) => Err(HeadError::internal(
                "pg_get_expr's tree argument was not text",
            )),
        },
        _ => Err(HeadError::internal(
            "a catalog-rendered function reached rendering with the wrong argument count",
        )),
    }
}

fn text(value: &str) -> Value {
    Value::from_text(value.to_string())
}

pub(super) fn policy_expression(typed: &Typed, table: &Table) -> Result<String, HeadError> {
    render(typed, table, 0)
}

/// `depth` is how many `CASE` blocks enclose this node (its condition,
/// result or `ELSE`, at any nesting): PostgreSQL 18's own pretty-printer
/// indents each nested `CASE`'s whole block by one more level than its
/// parent (probed: a `CASE` nested in an enclosing `CASE`'s `WHEN`
/// condition prints its own `CASE`/`END` four spaces deeper, its own arms
/// eight), and only `Typed::Case` ever introduces a newline, so `depth`
/// only ever changes where this function recurses into one's own
/// `base`/`when`/`then`/`otherwise`.
fn render(typed: &Typed, table: &Table, depth: usize) -> Result<String, HeadError> {
    Ok(match typed {
        Typed::AlwaysFalse => "false".to_string(),
        Typed::Column(_, attnum, _, _) => {
            table.column_at(*attnum)?.name.render().as_sql().to_string()
        }
        // Each of these is a form `Context::permit` already refuses for
        // `Position::Policy` before a `Typed` tree can exist (a subquery,
        // an aggregate), so reaching one here is an invariant violation,
        // not a normal refusal.
        Typed::Subquery(_) | Typed::Exists(_) | Typed::InSelect(..) | Typed::ArrayAgg(..) => {
            return Err(HeadError::internal(
                "a policy expression contains a form CREATE POLICY should have refused",
            ))
        }
        // Each of these PostgreSQL itself permits in a policy's `USING`;
        // this renderer cannot yet reproduce `pg_get_expr`'s text for one,
        // so `canonicalize` (`security/enforcement.rs`) turns this refusal
        // into `CREATE POLICY`'s own `0A000`, the one list of what a
        // policy may store.
        Typed::TableOid(..) => return Err(not_built(PolicyExpressionForm::TableOid)),
        Typed::Call(..) => return Err(not_built(PolicyExpressionForm::FunctionCall)),
        Typed::Subscript(..) => return Err(not_built(PolicyExpressionForm::ArraySubscript)),
        Typed::AnyEq(..) => return Err(not_built(PolicyExpressionForm::Any)),
        Typed::In(..) => return Err(not_built(PolicyExpressionForm::InList)),
        Typed::CurrentUser => "CURRENT_USER".to_string(),
        // PostgreSQL 18's own `pg_get_expr` deparses `ARRAY[e1, e2]`
        // verbatim (probed), so this is the one array form the renderer
        // reproduces exactly; `render_scalar`'s `Scalar::Array` (a
        // curly-brace text cast) is not, and stays refused there.
        Typed::ArrayLiteral(elements, _) => format!(
            "ARRAY[{}]",
            elements
                .iter()
                .map(|element| render(element, table, depth))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        ),
        Typed::Value(scalar, ty) => render_scalar(scalar, *ty)?,
        Typed::Cast(inner, ty) => {
            format!("({})::{}", render(inner, table, depth)?, ty.display_name())
        }
        Typed::Not(inner) => format!("({})", spaced("NOT", &render(inner, table, depth)?)),
        Typed::And(parts) => rendered_group(parts, "AND", table, depth)?,
        Typed::Or(parts) => rendered_group(parts, "OR", table, depth)?,
        Typed::Compare(op, left, right) => format!(
            "({} {})",
            render(left, table, depth)?,
            spaced(op.pg_symbol(), &render(right, table, depth)?)
        ),
        Typed::IsNull(inner, negated) => format!(
            "({} IS {}NULL)",
            render(inner, table, depth)?,
            if *negated { "NOT " } else { "" }
        ),
        Typed::Is(inner, truth, negated) => {
            let truth = match truth {
                Truth::True => "TRUE",
                Truth::False => "FALSE",
                Truth::Unknown => "UNKNOWN",
            };
            format!(
                "({} IS {}{truth})",
                render(inner, table, depth)?,
                if *negated { "NOT " } else { "" }
            )
        }
        Typed::DistinctFrom(left, right, negated) => {
            let positive = format!(
                "({} {})",
                render(left, table, depth)?,
                spaced("IS DISTINCT FROM", &render(right, table, depth)?)
            );
            if *negated {
                format!("(NOT {positive})")
            } else {
                positive
            }
        }
        Typed::Concat(left, right) => format!(
            "({} {})",
            render(left, table, depth)?,
            spaced("||", &render(right, table, depth)?)
        ),
        Typed::Case {
            base,
            arms,
            otherwise,
            ..
        } => {
            let indent = " ".repeat(4 * depth);
            let arm_indent = " ".repeat(4 * depth + 4);
            let base = base
                .as_deref()
                .map(|base| render(base, table, depth + 1))
                .transpose()?
                .map(|base| {
                    if base.starts_with('\n') {
                        base
                    } else {
                        format!(" {base}")
                    }
                })
                .unwrap_or_default();
            let arms = arms
                .iter()
                .map(|(when, then)| {
                    let then = spaced("THEN", &render(then, table, depth + 1)?);
                    Ok(format!(
                        "{arm_indent}{}\n",
                        spaced(
                            "WHEN",
                            &format!("{} {then}", render(when, table, depth + 1)?)
                        )
                    ))
                })
                .collect::<Result<Vec<_>, HeadError>>()?
                .join("");
            let otherwise = otherwise
                .as_deref()
                .map(|otherwise| render(otherwise, table, depth + 1))
                .transpose()?
                .map(|otherwise| format!("{arm_indent}{}\n", spaced("ELSE", &otherwise)))
                .unwrap_or_default();
            format!("\n{indent}CASE{base}\n{arms}{otherwise}{indent}END")
        }
    })
}

/// PostgreSQL 18's own layout for a multi-line construct (currently only a
/// `CASE`, whose own rendered text always opens with `\n`): the keyword or
/// operand right before one abuts its leading newline directly, with no
/// intervening space, while every other join keeps the usual single space
/// (probed: `pg_get_expr` shows `i >=\nCASE ... END`, never `i >= \nCASE`).
/// `keyword` is empty at the one call site (`CASE`'s own optional `base`)
/// that has no keyword of its own, just the same trim.
fn spaced(keyword: &str, right: &str) -> String {
    if right.starts_with('\n') {
        format!("{keyword}{right}")
    } else if keyword.is_empty() {
        right.to_string()
    } else {
        format!("{keyword} {right}")
    }
}

fn rendered_group(
    parts: &[Typed],
    keyword: &str,
    table: &Table,
    depth: usize,
) -> Result<String, HeadError> {
    let mut joined = String::new();
    for (index, part) in parts.iter().enumerate() {
        let rendered = render(part, table, depth)?;
        if index == 0 {
            joined.push_str(&rendered);
        } else {
            joined.push(' ');
            joined.push_str(&spaced(keyword, &rendered));
        }
    }
    Ok(format!("({joined})"))
}

fn render_scalar(
    scalar: &Scalar,
    ty: crate::analyze::types::TypeHandle,
) -> Result<String, HeadError> {
    Ok(match scalar {
        Scalar::Integer(value) if ty.oid() == BOOL_OID => {
            if *value == 0 { "false" } else { "true" }.to_string()
        }
        Scalar::Integer(value) => annotate_integer_literal(*value),
        Scalar::Text(text) if ty.oid() == NAME_OID => {
            format!("{}::name", ident::render_literal(text).as_sql())
        }
        Scalar::Text(text) if ty.oid() == INT4_OID => coerced_integer(text, ty)?,
        Scalar::Text(text) if ty.category() == b'N' => coerced_integer(text, ty)?,
        Scalar::Text(text) => format!("{}::text", ident::render_literal(text).as_sql()),
        Scalar::Null => format!("NULL::{}", ty.display_name()),
        Scalar::Array(_) => return Err(not_built(PolicyExpressionForm::ArrayLiteral)),
    })
}

/// The one place a policy's `USING` gets its `0A000`: a form PostgreSQL 18
/// itself permits there but this renderer cannot yet reproduce as
/// `pg_get_expr` text.
fn not_built(form: PolicyExpressionForm) -> HeadError {
    HeadError::not_supported(NotSupportedFeature::InPolicyExpression(form))
}

fn annotate_integer_literal(value: i64) -> String {
    match i32::try_from(value) {
        Ok(small) if small < 0 => format!("'{small}'::integer"),
        Ok(small) => small.to_string(),
        Err(_) => format!("'{value}'::bigint"),
    }
}

fn coerced_integer(text: &str, ty: crate::analyze::types::TypeHandle) -> Result<String, HeadError> {
    let value = crate::analyze::typing::parse_integer_text(text, ty)?;
    if ty.oid() == INT4_OID {
        Ok(annotate_integer_literal(value))
    } else {
        Ok(format!("'{value}'::{}", ty.display_name()))
    }
}
