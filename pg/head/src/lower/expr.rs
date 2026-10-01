use turso_core::Value;
use turso_parser::ast;

use crate::analyze::typing::{Scalar, Typed};
use crate::catalog::{Attnum, Catalog};
use crate::error::HeadError;
use crate::ident::RoleName;
use crate::lower::query::{engine_alias, lower_query};
use crate::lower::sql;
use crate::parse::expr::{CompareOp, EmptyArrayResult, Truth};

pub(crate) fn lower(
    typed: &Typed,
    params: &mut Vec<Value>,
    current_user: &RoleName,
    catalog: &Catalog,
) -> Result<Box<ast::Expr>, HeadError> {
    Ok(match typed {
        Typed::AlwaysFalse => Box::new(ast::Expr::Literal(ast::Literal::Numeric("0".to_string()))),
        Typed::Column(slot, attnum, _, _) => {
            sql::qualified_column_ref(&engine_alias(*slot), *attnum)
        }
        Typed::TableOid(oid) => sql::bind(params, Value::from_i64(oid.as_i64()))?,
        Typed::Value(scalar, ty) => {
            let bound = sql::bind(params, scalar_value(scalar, *ty)?)?;
            match ty.engine() {
                Ok(engine) if engine.array => Box::new(ast::Expr::Cast {
                    expr: bound,
                    type_name: Some(ast::Type {
                        name: sql::engine_type_keyword(engine.element).to_string(),
                        size: None,
                        array_dimensions: 1,
                    }),
                }),
                _ => bound,
            }
        }
        Typed::CurrentUser => {
            sql::bind(params, Value::from_text(current_user.as_str().to_string()))?
        }
        Typed::Cast(inner, ty) => {
            let engine = ty.engine()?;
            let cast = Box::new(ast::Expr::Cast {
                expr: lower(inner, params, current_user, catalog)?,
                type_name: Some(ast::Type {
                    name: sql::engine_type_keyword(engine.element).to_string(),
                    size: None,
                    array_dimensions: u32::from(engine.array),
                }),
            });
            match (
                engine.array,
                sql::engine_type_decode_function(engine.element),
            ) {
                (false, Some(decode)) => sql::function_call(decode, vec![*cast]),
                _ => cast,
            }
        }
        Typed::Not(inner) => Box::new(ast::Expr::Unary(
            ast::UnaryOperator::Not,
            lower(inner, params, current_user, catalog)?,
        )),
        Typed::And(items) => nary(items, ast::Operator::And, params, current_user, catalog)?,
        Typed::Or(items) => nary(items, ast::Operator::Or, params, current_user, catalog)?,
        Typed::Compare(op, left, right) => Box::new(ast::Expr::Binary(
            lower(left, params, current_user, catalog)?,
            compare_operator(*op),
            lower(right, params, current_user, catalog)?,
        )),
        Typed::IsNull(inner, negated) => {
            let inner = lower(inner, params, current_user, catalog)?;
            if *negated {
                Box::new(ast::Expr::NotNull(inner))
            } else {
                Box::new(ast::Expr::IsNull(inner))
            }
        }
        Typed::Is(inner, truth, negated) => {
            let inner = lower(inner, params, current_user, catalog)?;
            let literal = Box::new(ast::Expr::Literal(ast::Literal::Numeric(
                match truth {
                    Truth::True => "1",
                    Truth::False => "0",
                    Truth::Unknown => {
                        return Ok(if *negated {
                            Box::new(ast::Expr::NotNull(inner))
                        } else {
                            Box::new(ast::Expr::IsNull(inner))
                        })
                    }
                }
                .to_string(),
            )));
            let operator = if *negated {
                ast::Operator::IsNot
            } else {
                ast::Operator::Is
            };
            Box::new(ast::Expr::Binary(inner, operator, literal))
        }
        Typed::DistinctFrom(left, right, negated) => {
            let operator = if *negated {
                ast::Operator::Is
            } else {
                ast::Operator::IsNot
            };
            Box::new(ast::Expr::Binary(
                lower(left, params, current_user, catalog)?,
                operator,
                lower(right, params, current_user, catalog)?,
            ))
        }
        Typed::In(needle, list, negated) => Box::new(ast::Expr::InList {
            lhs: lower(needle, params, current_user, catalog)?,
            not: *negated,
            rhs: list
                .iter()
                .map(|item| lower(item, params, current_user, catalog))
                .collect::<Result<Vec<_>, _>>()?,
        }),
        Typed::Concat(left, right) => Box::new(ast::Expr::Binary(
            lower(left, params, current_user, catalog)?,
            ast::Operator::Concat,
            lower(right, params, current_user, catalog)?,
        )),
        Typed::Case {
            base,
            arms,
            otherwise,
            ..
        } => Box::new(ast::Expr::Case {
            base: base
                .as_deref()
                .map(|base| lower(base, params, current_user, catalog))
                .transpose()?,
            when_then_pairs: arms
                .iter()
                .map(|(when, then)| {
                    Ok((
                        lower(when, params, current_user, catalog)?,
                        lower(then, params, current_user, catalog)?,
                    ))
                })
                .collect::<Result<Vec<_>, HeadError>>()?,
            else_expr: otherwise
                .as_deref()
                .map(|otherwise| lower(otherwise, params, current_user, catalog))
                .transpose()?,
        }),
        Typed::Subquery(query) => Box::new(ast::Expr::Subquery(lower_query(
            query,
            None,
            catalog,
            params,
            current_user,
        )?)),
        Typed::Exists(query) => Box::new(ast::Expr::Exists(lower_query(
            query,
            None,
            catalog,
            params,
            current_user,
        )?)),
        Typed::InSelect(needle, query, negated) => Box::new(ast::Expr::InSelect {
            lhs: lower(needle, params, current_user, catalog)?,
            not: *negated,
            rhs: lower_query(query, None, catalog, params, current_user)?,
        }),
        // The engine's own `array` builtin (`ScalarFunc::Array`, desugared
        // from `ARRAY[...]` syntax at translation time), not a second array
        // representation: the same builtin `sql::array` already binds
        // constant array elements onto.
        Typed::ArrayLiteral(elements, _) => {
            let lowered = elements
                .iter()
                .map(|element| lower(element, params, current_user, catalog).map(|expr| *expr))
                .collect::<Result<Vec<_>, _>>()?;
            sql::function_call("array", lowered)
        }
        Typed::Call(handle, args) => match handle.evaluation() {
            crate::analyze::functions::Evaluation::Engine
            | crate::analyze::functions::Evaluation::HeadEngine => {
                let name = handle.engine_name()?;
                let lowered_args = args
                    .iter()
                    .map(|arg| lower(arg, params, current_user, catalog).map(|expr| *expr))
                    .collect::<Result<Vec<_>, _>>()?;
                let call = sql::function_call(name, lowered_args);
                if matches!(handle, crate::analyze::functions::FunctionHandle::ArrayUpper) {
                    let zero_based = args.first().is_some_and(|arg| {
                        matches!(
                            crate::analyze::typing::zero_based_origin_oid(arg),
                            Ok(crate::analyze::types::INT2VECTOR_OID | crate::analyze::types::OIDVECTOR_OID)
                        )
                    });
                    postgres_array_upper(call, zero_based)
                } else {
                    call
                }
            }
            crate::analyze::functions::Evaluation::SetReturning => {
                return Err(HeadError::internal(
                    "a set-returning function call reached engine lowering as a plain expression",
                ))
            }
            crate::analyze::functions::Evaluation::CatalogRendered => return Err(HeadError::internal(
                "a catalog-rendered function call reached engine lowering as a plain expression",
            )),
            crate::analyze::functions::Evaluation::Session => {
                return Err(HeadError::internal(
                    "a session function call reached engine lowering as a plain expression",
                ))
            }
            crate::analyze::functions::Evaluation::Aggregate => {
                let name = handle.engine_name()?;
                let lowered_args = args
                    .iter()
                    .map(|arg| lower(arg, params, current_user, catalog).map(|expr| *expr))
                    .collect::<Result<Vec<_>, _>>()?;
                sql::function_call(name, lowered_args)
            }
        },
        Typed::Subscript(base, index, _, zero_based) => {
            let base_ty = base.result_type()?;
            let is_name = base_ty.oid() == crate::analyze::types::NAME_OID;
            let base = lower(base, params, current_user, catalog)?;
            let index = lower(index, params, current_user, catalog)?;
            let index = if *zero_based {
                Box::new(ast::Expr::Binary(
                    index,
                    ast::Operator::Add,
                    sql::integer_literal(1),
                ))
            } else {
                index
            };
            if is_name {
                name_byte_at(base, index, base_ty.len())
            } else {
                sql::array_index_at(base, index)
            }
        }
        Typed::AnyEq(needle, array) => {
            let needle = lower(needle, params, current_user, catalog)?;
            let array = lower(array, params, current_user, catalog)?;
            let length = sql::coalesce(
                sql::array_length_at(array.clone(), sql::integer_literal(1)),
                sql::integer_literal(0),
            );
            let is_empty = Box::new(ast::Expr::Binary(
                length,
                ast::Operator::Equals,
                sql::integer_literal(0),
            ));
            let needle_is_null = Box::new(ast::Expr::IsNull(needle.clone()));
            let contains_needle = Box::new(ast::Expr::Binary(
                sql::array_contains_value(array.clone(), needle),
                ast::Operator::Equals,
                sql::integer_literal(1),
            ));
            let contains_null = Box::new(ast::Expr::Binary(
                sql::array_contains_value(array, Box::new(ast::Expr::Literal(ast::Literal::Null))),
                ast::Operator::Equals,
                sql::integer_literal(1),
            ));
            Box::new(ast::Expr::Case {
                base: None,
                when_then_pairs: vec![
                    (is_empty, sql::integer_literal(0)),
                    (
                        needle_is_null,
                        Box::new(ast::Expr::Literal(ast::Literal::Null)),
                    ),
                    (contains_needle, sql::integer_literal(1)),
                    (
                        contains_null,
                        Box::new(ast::Expr::Literal(ast::Literal::Null)),
                    ),
                ],
                else_expr: Some(sql::integer_literal(0)),
            })
        }
        Typed::ArrayAgg(query, _, empty_result) => {
            let mut inner = lower_query(query, None, catalog, params, current_user)?;
            sql::name_the_sole_result_column(&mut inner, Attnum::new(1));
            let alias = "array_source";
            let derived = ast::SelectTable::Select(inner, Some(sql::as_alias(alias)));
            let agg = sql::array_agg_of(sql::qualified_column_ref(alias, Attnum::new(1)));
            let result = match empty_result {
                EmptyArrayResult::Null => agg,
                EmptyArrayResult::EmptyArray => sql::coalesce(agg, sql::array(params, Vec::new())?),
            };
            let outer = ast::Select {
                with: None,
                body: ast::SelectBody {
                    select: ast::OneSelect::Select {
                        distinctness: None,
                        columns: vec![ast::ResultColumn::Expr(result, None)],
                        from: Some(ast::FromClause {
                            select: Box::new(derived),
                            joins: Vec::new(),
                        }),
                        where_clause: None,
                        group_by: None,
                        window_clause: Vec::new(),
                    },
                    compounds: Vec::new(),
                },
                order_by: Vec::new(),
                limit: None,
            };
            Box::new(ast::Expr::Subquery(outer))
        }
    })
}

fn postgres_array_upper(call: Box<ast::Expr>, zero_based: bool) -> Box<ast::Expr> {
    if zero_based {
        let shifted = Box::new(ast::Expr::Binary(
            call,
            ast::Operator::Subtract,
            sql::integer_literal(1),
        ));
        sql::nullif(shifted, sql::integer_literal(-1))
    } else {
        sql::nullif(call, sql::integer_literal(0))
    }
}

fn name_byte_at(base: Box<ast::Expr>, index: Box<ast::Expr>, stored_len: i64) -> Box<ast::Expr> {
    let bytes = Box::new(ast::Expr::Cast {
        expr: base,
        type_name: Some(ast::Type {
            name: sql::engine_type_keyword(crate::catalog::pg::EngineStorageType::Blob).to_string(),
            size: None,
            array_dimensions: 0,
        }),
    });
    let in_range = Box::new(ast::Expr::Between {
        lhs: index.clone(),
        not: false,
        start: sql::integer_literal(1),
        end: sql::integer_literal(stored_len),
    });
    let raw_byte = sql::function_call("substr", vec![*bytes, *index, *sql::integer_literal(1)]);
    let empty_blob = Box::new(ast::Expr::Literal(ast::Literal::Blob("x''".to_string())));
    let zero_byte = Box::new(ast::Expr::Literal(ast::Literal::Blob("x'00'".to_string())));
    let padded_byte = sql::coalesce(sql::nullif(raw_byte, empty_blob), zero_byte);
    let decoded = sql::function_call("char_out", vec![*padded_byte]);
    Box::new(ast::Expr::Case {
        base: None,
        when_then_pairs: vec![(in_range, decoded)],
        else_expr: None,
    })
}

fn compare_operator(op: CompareOp) -> ast::Operator {
    match op {
        CompareOp::Eq => ast::Operator::Equals,
        CompareOp::Ne => ast::Operator::NotEquals,
        CompareOp::Lt => ast::Operator::Less,
        CompareOp::Le => ast::Operator::LessEquals,
        CompareOp::Gt => ast::Operator::Greater,
        CompareOp::Ge => ast::Operator::GreaterEquals,
    }
}

pub(crate) fn scalar_value(
    scalar: &Scalar,
    ty: crate::analyze::types::TypeHandle,
) -> Result<Value, HeadError> {
    Ok(match scalar {
        Scalar::Integer(value) => Value::from_i64(*value),
        Scalar::Text(text) if ty.category() == b'N' => {
            Value::from_i64(crate::analyze::typing::parse_integer_text(text, ty)?)
        }
        Scalar::Text(text) => Value::from_text(text.clone()),
        Scalar::Null => Value::Null,
        Scalar::Array(items) => {
            let element = crate::analyze::types::TypeHandle::by_oid(i64::from(ty.element()))?;
            let values = items
                .iter()
                .map(|item| scalar_value(item, element))
                .collect::<Result<Vec<_>, HeadError>>()?;
            turso_core::encode_array(&values)
                .map_err(|error| HeadError::internal(error.to_string()))?
        }
    })
}

fn nary(
    items: &[Typed],
    operator: ast::Operator,
    params: &mut Vec<Value>,
    current_user: &RoleName,
    catalog: &Catalog,
) -> Result<Box<ast::Expr>, HeadError> {
    let mut result: Option<Box<ast::Expr>> = None;
    for item in items {
        let expr = lower(item, params, current_user, catalog)?;
        result = Some(match result {
            Some(left) => Box::new(ast::Expr::Binary(left, operator, expr)),
            None => expr,
        });
    }
    result.ok_or_else(|| HeadError::internal("AND/OR always holds at least one operand"))
}
