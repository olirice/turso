use pg_query::protobuf::{
    a_const::Val, node::Node as PgNode, AArrayExpr, AConst, AExpr, AExprKind, AIndices,
    AIndirection, BoolExpr, BoolExprType, BoolTestType, BooleanTest, CaseExpr, CaseWhen, FuncCall,
    Node, NullTest, NullTestType, ParamRef, SqlValueFunctionOp, SubLink, SubLinkType, TypeCast,
    TypeName,
};

use super::{node, unsupported_node, FromParser};
use crate::analyze::types::TypeHandle;
use crate::analyze::typing::Scalar;
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::{ColumnName, FunctionName, TableName};
use crate::parse::statement::{Query, SelectItem};
use crate::parse::Location;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmptyArrayResult {
    Null,
    EmptyArray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    pub(crate) fn pg_symbol(self) -> &'static str {
        match self {
            CompareOp::Eq => "=",
            CompareOp::Ne => "<>",
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Gt => ">",
            CompareOp::Ge => ">=",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Truth {
    True,
    False,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Literal {
    Integer(i64),
    Text(String),
    Boolean(bool),
    Null,
}

#[derive(Debug, Clone)]
pub(crate) enum Expr {
    /// The trailing location is the `ColumnRef`'s own (or `None` for a
    /// column this crate synthesizes itself), carried so an undefined- or
    /// ambiguous-column error can point at it the way PostgreSQL 18 does.
    Column(Option<TableName>, ColumnName, Option<Location>),
    CurrentUser,
    /// The trailing location is the `A_Const`'s own (or `None` for a
    /// literal this crate synthesizes itself), carried so an invalid-input,
    /// out-of-range or literal-coercion error can point at it the way
    /// PostgreSQL 18 does.
    Literal(Literal, Option<Location>),
    /// The trailing location is the `ParamRef`'s own (or `None` for a
    /// parameter this crate synthesizes itself), carried so an undefined-
    /// parameter error can point at it the way PostgreSQL 18 does.
    Param(u32, Option<Location>),
    Cast(Box<Expr>, TypeHandle),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    /// The trailing location is the `A_Expr`'s own (or `None` for a
    /// comparison this crate synthesizes itself), carried so an
    /// operator-does-not-exist error can point at it the way PostgreSQL 18
    /// does.
    Compare(CompareOp, Box<Expr>, Box<Expr>, Option<Location>),
    IsNull(Box<Expr>, bool),
    Is(Box<Expr>, Truth, bool),
    DistinctFrom(Box<Expr>, Box<Expr>, bool, Option<Location>),
    In(Box<Expr>, Vec<Expr>, bool, Option<Location>),
    Concat(Box<Expr>, Box<Expr>, Option<Location>),
    Case {
        base: Option<Box<Expr>>,
        arms: Vec<(Expr, Expr)>,
        otherwise: Option<Box<Expr>>,
    },
    /// The trailing location is the `SubLink`'s own (or `None` for a query
    /// this crate synthesizes itself), carried so a "cannot use subquery in
    /// ..." error can point at it the way PostgreSQL 18 does.
    Subquery(Box<Query>, Option<Location>),
    Exists(Box<Query>, Option<Location>),
    InSelect(Box<Expr>, Box<Query>, bool, Option<Location>),
    ArrayFromQuery(Box<Query>, EmptyArrayResult, Option<Location>),
    /// `ARRAY[e1, e2, ...]`: a general expression, unlike `ArrayFromQuery`'s
    /// `ARRAY(SELECT ...)`, so it carries other expressions, never a query.
    ArrayLiteral(Vec<Expr>),
    /// The trailing location is the `FuncCall`'s own (or `None` for a call
    /// this crate synthesizes itself), carried so an undefined-function
    /// error can point at it the way PostgreSQL 18 does.
    Call(FunctionName, Vec<Expr>, Option<Location>),
    Subscript(Box<Expr>, Box<Expr>),
    /// The trailing location is the `A_Expr`'s own (or `None` for an ANY
    /// comparison this crate synthesizes itself), carried so an
    /// operator-does-not-exist error can point at it the way PostgreSQL 18
    /// does.
    AnyEq(Box<Expr>, Box<Expr>, Option<Location>),
    Resolved(Box<crate::analyze::typing::Typed>),
}

pub(crate) fn select_item_display_name(expr: &Expr) -> Result<Option<ColumnName>, HeadError> {
    match expr {
        // PostgreSQL 18 keeps a cast's inner name when it has one (a column
        // reference, say); otherwise it falls back to the cast's own target
        // type name, e.g. `5::integer` displays as "int4". A chain of casts
        // over a nameless base (`'x'::regclass::oid`) falls back to the
        // OUTERMOST cast's type ("oid"), never an inner cast's own
        // fallback, so the inner name looked up here is the strict,
        // never-falls-back-itself kind (`nested_display_name`).
        Expr::Cast(inner, ty) => match nested_display_name(inner)? {
            Some(name) => Ok(Some(name)),
            // PostgreSQL 18 names an array-literal cast (`'{1,2}'::int2[]`)
            // after its element type, not the array type itself;
            // `cast_type_display_name` is already that fallback.
            None => Ok(Some(cast_type_display_name(*ty)?)),
        },
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
        | other @ Expr::Subquery(..)
        | other @ Expr::Exists(..)
        | other @ Expr::InSelect(..)
        | other @ Expr::ArrayFromQuery(..)
        | other @ Expr::ArrayLiteral(_)
        | other @ Expr::Call(..)
        | other @ Expr::Subscript(..)
        | other @ Expr::AnyEq(..)
        | other @ Expr::Resolved(_) => nested_display_name(other),
    }
}

/// The name an expression contributes when it is not itself the select
/// item being named, only nested inside one (a cast's argument, a
/// subscript's base): PostgreSQL's cast-target-type fallback only ever
/// applies at the outermost cast of a chain, so this never invokes it,
/// unlike `select_item_display_name`.
fn nested_display_name(expr: &Expr) -> Result<Option<ColumnName>, HeadError> {
    match expr {
        Expr::Column(_, name, _) => Ok(Some(name.clone())),
        Expr::Call(name, _, _) => Ok(Some(ColumnName::literal(name.as_str().to_string()))),
        Expr::Cast(inner, _) => nested_display_name(inner),
        // PostgreSQL 18 names an unaliased CASE "case", literally.
        Expr::Case { .. } => Ok(Some(ColumnName::literal("case"))),
        // PostgreSQL 18 names an unaliased ARRAY(subquery) "array",
        // literally, never propagating the inner query's own column name.
        Expr::ArrayFromQuery(..) => Ok(Some(ColumnName::literal("array"))),
        // PostgreSQL 18 names an unaliased scalar subquery after its own
        // sole output column, the same rule applied recursively.
        Expr::Subquery(query, _) => scalar_subquery_display_name(query),
        // A subscript keeps its base expression's name, the same as a cast.
        Expr::Subscript(base, _) => nested_display_name(base),
        Expr::CurrentUser
        | Expr::Literal(..)
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
        | Expr::Exists(..)
        | Expr::InSelect(..)
        | Expr::ArrayLiteral(_)
        | Expr::AnyEq(..)
        | Expr::Resolved(_) => Ok(None),
    }
}

/// The name PostgreSQL 18 falls back to for an unaliased cast whose
/// argument has no name of its own: the target type's own `pg_type.typname`
/// (an array type's element type, since the head only ever casts to an
/// array type through an array-literal's element type). `ty.element()`
/// names an oid straight out of the same compiled-in `pg_type` capture
/// `TypeHandle::by_oid` searches, so a lookup miss here is a genuine
/// catalog-capture inconsistency, not a normal failure to hedge around;
/// it is reported like every other internal-consistency failure, not
/// papered over with the array type's own name.
fn cast_type_display_name(ty: TypeHandle) -> Result<ColumnName, HeadError> {
    let element = ty.element();
    let name = if element == 0 {
        ty.name()
    } else {
        TypeHandle::by_oid(i64::from(element))?.name()
    };
    Ok(ColumnName::literal(name.to_string()))
}

fn scalar_subquery_display_name(query: &Query) -> Result<Option<ColumnName>, HeadError> {
    if !query.combined.is_empty() {
        return Ok(None);
    }
    let [SelectItem::Expr(expr, alias)] = query.first.items.as_slice() else {
        return Ok(None);
    };
    match alias {
        Some(alias) => Ok(Some(alias.clone())),
        None => select_item_display_name(expr),
    }
}

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "pg_query's Node oneof enumerates every node kind PostgreSQL's parser can produce; this refuses every kind not explicitly admitted here, never silently accepting one"
)]
pub(super) fn admit(proof: FromParser, raw: &PgNode) -> Result<Expr, HeadError> {
    match raw {
        PgNode::ColumnRef(reference) => column_ref(
            proof,
            &reference.fields,
            Location::from_raw(reference.location),
        ),
        PgNode::AConst(constant) => literal(constant)
            .map(|value| Expr::Literal(value, Location::from_raw(constant.location))),
        PgNode::SqlvalueFunction(function)
            if matches!(
                SqlValueFunctionOp::try_from(function.op),
                Ok(SqlValueFunctionOp::SvfopCurrentUser)
            ) =>
        {
            Ok(Expr::CurrentUser)
        }
        PgNode::TypeCast(cast) => type_cast(proof, cast),
        PgNode::NullTest(test) => null_test(proof, test),
        PgNode::BooleanTest(test) => boolean_test(proof, test),
        PgNode::BoolExpr(expression) => bool_expr(proof, expression),
        PgNode::CaseExpr(case) => case_expr(proof, case),
        PgNode::AExpr(expression) => a_expr(proof, expression),
        PgNode::SubLink(sub_link) => sub_link_expr(proof, sub_link),
        PgNode::FuncCall(call) => {
            let (name, args) = func_call(proof, call)?;
            Ok(Expr::Call(name, args, Location::from_raw(call.location)))
        }
        PgNode::AIndirection(indirection) => subscript_expr(proof, indirection),
        PgNode::ParamRef(param) => param_ref(param),
        PgNode::AArrayExpr(array) => array_literal(proof, array),
        other => Err(unsupported_node(other, "this expression")),
    }
}

fn array_literal(proof: FromParser, array: &AArrayExpr) -> Result<Expr, HeadError> {
    let AArrayExpr {
        elements,
        // No PostgreSQL error is reported at an `ARRAY[...]` literal's own
        // position; a type-mismatch error among its elements instead
        // points at the element itself, and every element already carries
        // its own location through `admit`.
        location: _,
    } = array;
    let elements = elements
        .iter()
        .map(|element| admit(proof, node(Some(element))?))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Expr::ArrayLiteral(elements))
}

fn param_ref(param: &ParamRef) -> Result<Expr, HeadError> {
    let ParamRef { number, location } = param;
    let location = Location::from_raw(*location);
    let number = u32::try_from(*number)
        .map_err(|_| HeadError::not_supported(NotSupportedFeature::ParameterNumberOutOfRange))?;
    if number == 0 {
        return Err(HeadError::not_supported(
            NotSupportedFeature::PositionalParameterZero,
        ));
    }
    Ok(Expr::Param(number, location))
}

pub(super) fn func_call(
    proof: FromParser,
    call: &FuncCall,
) -> Result<(FunctionName, Vec<Expr>), HeadError> {
    let FuncCall {
        funcname,
        args,
        agg_order,
        agg_filter,
        over,
        agg_within_group,
        agg_star,
        agg_distinct,
        func_variadic,
        // Always the analyzer's own default coercion-form tag at parse
        // time, never set by the raw grammar for any call shape.
        funcformat: _,
        // Read directly off the same `call` by `admit`, the only caller
        // that builds an `Expr::Call` from this function's result.
        location: _,
    } = call;
    let name = function_name(proof, funcname)?;
    let function_call_clause = |clause: &'static str| {
        HeadError::not_supported(NotSupportedFeature::FunctionCallClause(clause))
    };
    if !agg_order.is_empty() {
        return Err(function_call_clause("aggregate ORDER BY"));
    }
    if agg_filter.is_some() {
        return Err(function_call_clause("FILTER"));
    }
    if over.is_some() {
        return Err(function_call_clause("OVER"));
    }
    if *agg_within_group {
        return Err(function_call_clause("WITHIN GROUP"));
    }
    if *agg_star {
        return Err(function_call_clause("an aggregate's *"));
    }
    if *agg_distinct {
        return Err(function_call_clause("DISTINCT in a function call"));
    }
    if *func_variadic {
        return Err(function_call_clause("VARIADIC"));
    }
    let args = args
        .iter()
        .map(|arg| {
            let arg = node(Some(arg))?;
            if matches!(arg, PgNode::NamedArgExpr(_)) {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::NamedFunctionArguments,
                ));
            }
            admit(proof, arg)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((name, args))
}

pub(super) fn function_name(proof: FromParser, parts: &[Node]) -> Result<FunctionName, HeadError> {
    let names = parts
        .iter()
        .map(|part| {
            let PgNode::String(part) = node(Some(part))? else {
                return Err(HeadError::internal(
                    "a function name part that is not a string",
                ));
            };
            Ok(part.sval.as_str())
        })
        .collect::<Result<Vec<_>, _>>()?;
    match names.as_slice() {
        ["pg_catalog", name] | [name] => {
            FunctionName::from_parse_tree(proof, name.to_ascii_lowercase())
        }
        _ => Err(HeadError::not_supported(
            NotSupportedFeature::SchemaQualifiedFunctionCall,
        )),
    }
}

fn column_ref(
    proof: FromParser,
    fields: &[Node],
    location: Option<Location>,
) -> Result<Expr, HeadError> {
    match fields {
        [field] => {
            let PgNode::String(name) = node(Some(field))? else {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::StarMixedWithOtherSelectItems,
                ));
            };
            Ok(Expr::Column(
                None,
                ColumnName::from_parse_tree(proof, name.sval.clone())?,
                location,
            ))
        }
        [qualifier, field] => {
            let (PgNode::String(qualifier), PgNode::String(name)) =
                (node(Some(qualifier))?, node(Some(field))?)
            else {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::QualifiedColumnReferences,
                ));
            };
            Ok(Expr::Column(
                Some(TableName::from_parse_tree(proof, qualifier.sval.clone())?),
                ColumnName::from_parse_tree(proof, name.sval.clone())?,
                location,
            ))
        }
        _ => Err(HeadError::not_supported(
            NotSupportedFeature::QualifiedColumnReferences,
        )),
    }
}

fn literal(constant: &AConst) -> Result<Literal, HeadError> {
    let AConst {
        isnull,
        // Read directly off the same `constant` by `admit`, the only
        // caller that builds an `Expr::Literal` from this function's
        // result.
        location: _,
        val,
    } = constant;
    if *isnull {
        return Ok(Literal::Null);
    }
    match val {
        Some(Val::Ival(integer)) => Ok(Literal::Integer(i64::from(integer.ival))),
        Some(Val::Fval(float)) => float
            .fval
            .parse::<i64>()
            .map(Literal::Integer)
            .map_err(|_| HeadError::not_supported(NotSupportedFeature::NonIntegerNumericLiteral)),
        Some(Val::Sval(text)) => Ok(Literal::Text(text.sval.clone())),
        Some(Val::Boolval(value)) => Ok(Literal::Boolean(value.boolval)),
        Some(Val::Bsval(_)) => Err(HeadError::not_supported(
            NotSupportedFeature::BitStringLiterals,
        )),
        None => Err(HeadError::internal(
            "a non-null A_Const with no value variant",
        )),
    }
}

pub(super) fn cast_target_type(
    proof: FromParser,
    type_name: &TypeName,
) -> Result<TypeHandle, HeadError> {
    let (handle, joined) = named_type(proof, type_name)?;
    if !handle.is_declarable_column() {
        return Err(HeadError::not_supported(NotSupportedFeature::ColumnType(
            joined,
        )));
    }
    Ok(handle)
}

pub(super) fn parameter_type(
    proof: FromParser,
    type_name: &TypeName,
) -> Result<TypeHandle, HeadError> {
    named_type(proof, type_name).map(|(handle, _)| handle)
}

fn value_cast_target_type(
    proof: FromParser,
    type_name: &TypeName,
) -> Result<TypeHandle, HeadError> {
    let (handle, joined) = named_type(proof, type_name)?;
    if !handle.is_value_castable() {
        return Err(HeadError::not_supported(NotSupportedFeature::ColumnType(
            joined,
        )));
    }
    Ok(handle)
}

/// A `TypeName`'s `names`, each part a `String` node: shared by `named_type`
/// (a column or cast's target type) and `array_element_and_array_types` (an
/// array literal's target type), the two readers of a `TypeName`'s own
/// qualified name.
fn type_name_parts(names: &[Node]) -> Result<Vec<&str>, HeadError> {
    names
        .iter()
        .map(|name| {
            let PgNode::String(name) = node(Some(name))? else {
                return Err(HeadError::internal("a type name part that is not a string"));
            };
            Ok(name.sval.as_str())
        })
        .collect()
}

fn named_type(proof: FromParser, type_name: &TypeName) -> Result<(TypeHandle, String), HeadError> {
    let TypeName {
        names,
        // Always `0` from the raw parser: only the analyzer resolving a
        // type name fills in its own oid, and this function is that
        // resolution (`TypeHandle::by_name` below), not a reader of one
        // already resolved.
        type_oid: _,
        setof,
        pct_type,
        typmods,
        // Always `-1` from the raw parser; a real typmod parses into
        // `typmods` instead, checked below.
        typemod: _,
        array_bounds,
        location,
    } = type_name;
    if !typmods.is_empty() {
        return Err(HeadError::not_supported(NotSupportedFeature::TypeModifiers));
    }
    if !array_bounds.is_empty() || *setof || *pct_type {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ArraySetofPctType,
        ));
    }
    let names = type_name_parts(names)?;
    let joined = names.join(".");
    let unqualified = match names.as_slice() {
        ["pg_catalog", name] | [name] => *name,
        _ => {
            return Err(HeadError::not_supported(NotSupportedFeature::ColumnType(
                joined,
            )))
        }
    };
    let unqualified = crate::ident::TypeName::from_parse_tree(proof, unqualified)?;
    let handle = TypeHandle::by_name(&unqualified).ok_or_else(|| {
        HeadError::raise(PgError::UndefinedType(joined.clone())).at(Location::from_raw(*location))
    })?;
    Ok((handle, joined))
}

fn type_cast(proof: FromParser, cast: &TypeCast) -> Result<Expr, HeadError> {
    let TypeCast {
        arg,
        type_name,
        // No PostgreSQL error is reported at a cast's own position
        // (checked live): "invalid input syntax" points at the argument,
        // "cannot cast type X to Y" at the target type name, both already
        // carried by `admit`'s own recursion and `named_type`'s
        // `type_name.location` respectively.
        location: _,
    } = cast;
    let arg = arg
        .as_deref()
        .ok_or_else(|| HeadError::internal("a cast without an argument"))?;
    let type_name = type_name
        .as_ref()
        .ok_or_else(|| HeadError::internal("a cast without a target type"))?;
    if !type_name.array_bounds.is_empty() {
        return array_cast(proof, type_name, node(Some(arg))?);
    }
    let ty = value_cast_target_type(proof, type_name)?;
    Ok(Expr::Cast(Box::new(admit(proof, node(Some(arg))?)?), ty))
}

fn unsupported_array_literal() -> HeadError {
    HeadError::not_supported(NotSupportedFeature::ArraySetofPctType)
}

fn array_element_and_array_types(
    proof: FromParser,
    type_name: &TypeName,
) -> Result<(TypeHandle, TypeHandle), HeadError> {
    let TypeName {
        names,
        type_oid: _,
        setof,
        pct_type,
        typmods,
        typemod: _,
        // Checked by `type_cast`, which only calls `array_cast` (this
        // function's one caller) once this is already known non-empty;
        // re-reading it here would only duplicate that check.
        array_bounds: _,
        location,
    } = type_name;
    if *setof || *pct_type || !typmods.is_empty() {
        return Err(unsupported_array_literal());
    }
    let names = type_name_parts(names)?;
    let unqualified = match names.as_slice() {
        ["pg_catalog", name] | [name] => *name,
        _ => return Err(unsupported_array_literal()),
    };
    let unqualified_display = unqualified;
    let unqualified = crate::ident::TypeName::from_parse_tree(proof, unqualified)?;
    let element = TypeHandle::by_name(&unqualified).ok_or_else(|| {
        HeadError::raise(PgError::UndefinedType(unqualified_display.to_string()))
            .at(Location::from_raw(*location))
    })?;
    let array_oid = element.array_type();
    if array_oid == 0 {
        return Err(unsupported_array_literal());
    }
    let array_type = TypeHandle::by_oid(i64::from(array_oid))?;
    Ok((element, array_type))
}

/// A literal string constant is validated for its target array type only
/// once typing has a `Context` to gate its position through
/// (`analyze/typing/check.rs`'s `Expr::Cast` arm), the same as every other
/// literal; here it is only classified as one, never parsed. Every other
/// argument form keeps its own gate here, at the admit stage, since typing
/// never sees an array target type for anything else.
fn array_cast(proof: FromParser, type_name: &TypeName, arg: &PgNode) -> Result<Expr, HeadError> {
    let (_, array_type) = array_element_and_array_types(proof, type_name)?;
    let is_literal_text =
        matches!(arg, PgNode::AConst(constant) if matches!(constant.val, Some(Val::Sval(_))));
    if !is_literal_text && !array_type.is_value_castable() {
        return Err(unsupported_array_literal());
    }
    Ok(Expr::Cast(Box::new(admit(proof, arg)?), array_type))
}

pub(crate) fn parse_array_literal_text(
    text: &str,
    element: TypeHandle,
) -> Result<Vec<Scalar>, HeadError> {
    let inner = text
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| {
            HeadError::not_supported(NotSupportedFeature::ArrayLiteralNotBraceDelimited)
        })?;
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .map(|item| {
            let item = item.trim();
            if item.contains(['"', '\\', '{', '}']) {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::ArrayLiteralElementQuotedOrNested,
                ));
            }
            if item.eq_ignore_ascii_case("null") {
                Ok(Scalar::Null)
            } else if element.category() == b'N' {
                Ok(Scalar::Integer(crate::analyze::typing::parse_integer_text(
                    item, element,
                )?))
            } else {
                Ok(Scalar::Text(item.to_string()))
            }
        })
        .collect()
}

fn null_test(proof: FromParser, test: &NullTest) -> Result<Expr, HeadError> {
    let NullTest {
        // The generic executable-node header every `Expr`-like pg_query
        // struct on this page carries: `xpr` is populated only by the
        // executor, never by `pg_query`'s raw parser.
        xpr: _,
        arg,
        nulltesttype,
        // Distinguishes `(a, b) IS NULL` (a row test, checking each
        // field) from a scalar `IS NULL`; always `false` from the raw
        // parser regardless of the argument's own shape (checked live),
        // since PostgreSQL only classifies the argument as a row during
        // analysis, not parsing.
        argisrow: _,
        // No PostgreSQL error is reported at an `IS [NOT] NULL` test's own
        // position.
        location: _,
    } = test;
    let arg = arg
        .as_deref()
        .ok_or_else(|| HeadError::internal("NullTest without an argument"))?;
    let inner = Box::new(admit(proof, node(Some(arg))?)?);
    match NullTestType::try_from(*nulltesttype) {
        Ok(NullTestType::IsNull) => Ok(Expr::IsNull(inner, false)),
        Ok(NullTestType::IsNotNull) => Ok(Expr::IsNull(inner, true)),
        _ => Err(HeadError::internal("NullTest with no test type")),
    }
}

fn boolean_test(proof: FromParser, test: &BooleanTest) -> Result<Expr, HeadError> {
    let BooleanTest {
        xpr: _,
        arg,
        booltesttype,
        location: _,
    } = test;
    let arg = arg
        .as_deref()
        .ok_or_else(|| HeadError::internal("BooleanTest without an argument"))?;
    let inner = Box::new(admit(proof, node(Some(arg))?)?);
    match BoolTestType::try_from(*booltesttype) {
        Ok(BoolTestType::IsTrue) => Ok(Expr::Is(inner, Truth::True, false)),
        Ok(BoolTestType::IsNotTrue) => Ok(Expr::Is(inner, Truth::True, true)),
        Ok(BoolTestType::IsFalse) => Ok(Expr::Is(inner, Truth::False, false)),
        Ok(BoolTestType::IsNotFalse) => Ok(Expr::Is(inner, Truth::False, true)),
        Ok(BoolTestType::IsUnknown) => Ok(Expr::Is(inner, Truth::Unknown, false)),
        Ok(BoolTestType::IsNotUnknown) => Ok(Expr::Is(inner, Truth::Unknown, true)),
        _ => Err(HeadError::internal("BooleanTest with no test type")),
    }
}

fn bool_expr(proof: FromParser, expression: &BoolExpr) -> Result<Expr, HeadError> {
    let BoolExpr {
        xpr: _,
        boolop,
        args,
        // "argument of AND/OR/NOT must be type boolean" points at the
        // offending argument's own position (checked live), never at the
        // `BoolExpr` itself.
        location: _,
    } = expression;
    let args = args
        .iter()
        .map(|argument| admit(proof, node(Some(argument))?))
        .collect::<Result<Vec<_>, _>>()?;
    match BoolExprType::try_from(*boolop) {
        Ok(BoolExprType::AndExpr) => Ok(Expr::And(args)),
        Ok(BoolExprType::OrExpr) => Ok(Expr::Or(args)),
        Ok(BoolExprType::NotExpr) => {
            let mut args = args;
            let Some(only) = (args.len() == 1).then(|| args.remove(0)) else {
                return Err(HeadError::internal("NOT with other than 1 argument"));
            };
            Ok(Expr::Not(Box::new(only)))
        }
        _ => Err(HeadError::internal("BoolExpr with no operator")),
    }
}

fn case_expr(proof: FromParser, case: &CaseExpr) -> Result<Expr, HeadError> {
    let CaseExpr {
        xpr: _,
        // The `CASE`'s own result type and its collation: always `0`
        // from the raw parser, filled in only once typing resolves the
        // arms' common type (`analyze/typing`), which this function never
        // sees.
        casetype: _,
        casecollid: _,
        arg,
        args,
        defresult,
        location: _,
    } = case;
    let base = arg
        .as_deref()
        .map(|arg| admit(proof, node(Some(arg))?).map(Box::new))
        .transpose()?;
    let arms = args
        .iter()
        .map(|arg| {
            let PgNode::CaseWhen(when) = node(Some(arg))? else {
                return Err(HeadError::internal("a CASE arm that is not a WHEN"));
            };
            case_when(proof, when)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let otherwise = defresult
        .as_deref()
        .map(|result| admit(proof, node(Some(result))?).map(Box::new))
        .transpose()?;
    Ok(Expr::Case {
        base,
        arms,
        otherwise,
    })
}

fn case_when(proof: FromParser, when: &CaseWhen) -> Result<(Expr, Expr), HeadError> {
    let CaseWhen {
        xpr: _,
        expr: condition,
        result,
        location: _,
    } = when;
    let condition = condition
        .as_deref()
        .ok_or_else(|| HeadError::internal("CASE WHEN without a condition"))?;
    let result = result
        .as_deref()
        .ok_or_else(|| HeadError::internal("CASE WHEN without a result"))?;
    Ok((
        admit(proof, node(Some(condition))?)?,
        admit(proof, node(Some(result))?)?,
    ))
}

fn a_expr(proof: FromParser, expression: &AExpr) -> Result<Expr, HeadError> {
    let AExpr {
        kind,
        name,
        lexpr,
        rexpr,
        // "operator does not exist" points at the operator token itself
        // (checked live), which is this same location; threaded into
        // whichever of `Expr::Compare`/`Expr::Concat`/`Expr::In`/
        // `Expr::DistinctFrom`/`Expr::AnyEq` this expression resolves to.
        location,
    } = expression;
    let location = Location::from_raw(*location);
    match AExprKind::try_from(*kind) {
        Ok(AExprKind::AexprOp) => compare_or_concat(proof, name, lexpr, rexpr, location),
        Ok(AExprKind::AexprIn) => in_expr(proof, name, lexpr, rexpr, location),
        Ok(AExprKind::AexprDistinct) => distinct_expr(proof, lexpr, rexpr, false, location),
        Ok(AExprKind::AexprNotDistinct) => distinct_expr(proof, lexpr, rexpr, true, location),
        Ok(AExprKind::AexprOpAny) => any_expr(proof, name, lexpr, rexpr, location),
        _ => Err(unsupported_expr(name)),
    }
}

fn any_expr(
    proof: FromParser,
    name: &[Node],
    lexpr: &Option<Box<Node>>,
    rexpr: &Option<Box<Node>>,
    location: Option<Location>,
) -> Result<Expr, HeadError> {
    let operator = operator_name(name)?;
    if operator != "=" {
        return Err(HeadError::not_supported(
            NotSupportedFeature::AnyWithOperator(operator.to_string()),
        ));
    }
    let left = lexpr
        .as_deref()
        .ok_or_else(|| HeadError::internal("ANY without a left operand"))?;
    let right = rexpr
        .as_deref()
        .ok_or_else(|| HeadError::internal("ANY without an array operand"))?;
    Ok(Expr::AnyEq(
        Box::new(admit(proof, node(Some(left))?)?),
        Box::new(admit(proof, node(Some(right))?)?),
        location,
    ))
}

fn subscript_expr(proof: FromParser, indirection: &AIndirection) -> Result<Expr, HeadError> {
    let AIndirection { arg, indirection } = indirection;
    let arg = arg
        .as_deref()
        .ok_or_else(|| HeadError::internal("a subscript without a base"))?;
    let [item] = indirection.as_slice() else {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ChainedSubscript,
        ));
    };
    let PgNode::AIndices(indices) = node(Some(item))? else {
        return Err(HeadError::not_supported(
            NotSupportedFeature::FieldAccessOnValue,
        ));
    };
    let AIndices {
        is_slice,
        lidx,
        uidx,
    } = &**indices;
    if *is_slice || lidx.is_some() {
        return Err(HeadError::not_supported(NotSupportedFeature::ArraySlice));
    }
    let index = uidx
        .as_deref()
        .ok_or_else(|| HeadError::internal("a subscript without an index"))?;
    Ok(Expr::Subscript(
        Box::new(admit(proof, node(Some(arg))?)?),
        Box::new(admit(proof, node(Some(index))?)?),
    ))
}

fn unsupported_expr(name: &[Node]) -> HeadError {
    match name {
        [operator] => match operator.node.as_ref() {
            Some(PgNode::String(operator)) => {
                HeadError::not_supported(NotSupportedFeature::Operator(operator.sval.clone()))
            }
            _ => HeadError::not_supported(NotSupportedFeature::ThisExpression),
        },
        _ => HeadError::not_supported(NotSupportedFeature::ThisExpression),
    }
}

fn operator_name(name: &[Node]) -> Result<&str, HeadError> {
    let [operator] = name else {
        return Err(HeadError::not_supported(
            NotSupportedFeature::SchemaQualifiedOperators,
        ));
    };
    let PgNode::String(operator) = node(Some(operator))? else {
        return Err(HeadError::internal("an operator name that is not a string"));
    };
    Ok(operator.sval.as_str())
}

fn compare_or_concat(
    proof: FromParser,
    name: &[Node],
    lexpr: &Option<Box<Node>>,
    rexpr: &Option<Box<Node>>,
    location: Option<Location>,
) -> Result<Expr, HeadError> {
    let operator = operator_name(name)?;
    let left = lexpr
        .as_deref()
        .ok_or_else(|| HeadError::internal("a binary operator without a left operand"))?;
    let right = rexpr
        .as_deref()
        .ok_or_else(|| HeadError::internal("a binary operator without a right operand"))?;
    let left = Box::new(admit(proof, node(Some(left))?)?);
    let right = Box::new(admit(proof, node(Some(right))?)?);
    match operator {
        "=" => Ok(Expr::Compare(CompareOp::Eq, left, right, location)),
        "<>" | "!=" => Ok(Expr::Compare(CompareOp::Ne, left, right, location)),
        "<" => Ok(Expr::Compare(CompareOp::Lt, left, right, location)),
        "<=" => Ok(Expr::Compare(CompareOp::Le, left, right, location)),
        ">" => Ok(Expr::Compare(CompareOp::Gt, left, right, location)),
        ">=" => Ok(Expr::Compare(CompareOp::Ge, left, right, location)),
        "||" => Ok(Expr::Concat(left, right, location)),
        other => Err(HeadError::not_supported(NotSupportedFeature::Operator(
            other.to_string(),
        ))),
    }
}

fn in_expr(
    proof: FromParser,
    name: &[Node],
    lexpr: &Option<Box<Node>>,
    rexpr: &Option<Box<Node>>,
    location: Option<Location>,
) -> Result<Expr, HeadError> {
    let left = lexpr
        .as_deref()
        .ok_or_else(|| HeadError::internal("IN without a left operand"))?;
    let left = Box::new(admit(proof, node(Some(left))?)?);
    let right = rexpr
        .as_deref()
        .ok_or_else(|| HeadError::internal("IN without a list"))?;
    let PgNode::List(list) = node(Some(right))? else {
        return Err(HeadError::internal(
            "IN with a right side that is not a list",
        ));
    };
    let list = list
        .items
        .iter()
        .map(|item| admit(proof, node(Some(item))?))
        .collect::<Result<Vec<_>, _>>()?;
    let not = operator_name(name)? == "<>";
    Ok(Expr::In(left, list, not, location))
}

fn sub_link_expr(proof: FromParser, sub_link: &SubLink) -> Result<Expr, HeadError> {
    let SubLink {
        xpr: _,
        sub_link_type,
        // The analyzer's own correlated-subquery numbering: always `0`
        // from the raw parser, which never assigns one.
        sub_link_id: _,
        testexpr,
        oper_name,
        subselect,
        location,
    } = sub_link;
    let subselect = subselect
        .as_deref()
        .ok_or_else(|| HeadError::internal("a subquery expression without a subselect"))?;
    let PgNode::SelectStmt(select) = node(Some(subselect))? else {
        return Err(HeadError::internal(
            "a subquery expression whose subselect is not a SELECT",
        ));
    };
    let query = super::query::query(select)?;
    let location = Location::from_raw(*location);
    match SubLinkType::try_from(*sub_link_type) {
        Ok(SubLinkType::ExistsSublink) => Ok(Expr::Exists(Box::new(query), location)),
        Ok(SubLinkType::ExprSublink) => Ok(Expr::Subquery(Box::new(query), location)),
        Ok(SubLinkType::ArraySublink) => Ok(Expr::ArrayFromQuery(
            Box::new(query),
            EmptyArrayResult::EmptyArray,
            location,
        )),
        Ok(SubLinkType::AnySublink) => in_subquery(proof, oper_name, testexpr, query, location),
        _ => Err(HeadError::not_supported(
            NotSupportedFeature::UnsupportedSubqueryForm,
        )),
    }
}

fn in_subquery(
    proof: FromParser,
    oper_name: &[Node],
    testexpr: &Option<Box<Node>>,
    query: Query,
    location: Option<Location>,
) -> Result<Expr, HeadError> {
    let parts: Vec<&str> = oper_name
        .iter()
        .filter_map(|part| match node(Some(part)) {
            Ok(PgNode::String(part)) => Some(part.sval.as_str()),
            _ => None,
        })
        .collect();
    if !matches!(parts.as_slice(), [] | ["="] | ["pg_catalog", "="]) {
        return Err(HeadError::not_supported(
            NotSupportedFeature::SubqueryOperatorNotIn,
        ));
    }
    let test = testexpr
        .as_deref()
        .ok_or_else(|| HeadError::internal("IN (SELECT ...) without a left operand"))?;
    let lhs = Box::new(admit(proof, node(Some(test))?)?);
    Ok(Expr::InSelect(lhs, Box::new(query), false, location))
}

fn distinct_expr(
    proof: FromParser,
    lexpr: &Option<Box<Node>>,
    rexpr: &Option<Box<Node>>,
    not: bool,
    location: Option<Location>,
) -> Result<Expr, HeadError> {
    let left = lexpr
        .as_deref()
        .ok_or_else(|| HeadError::internal("DISTINCT FROM without a left operand"))?;
    let right = rexpr
        .as_deref()
        .ok_or_else(|| HeadError::internal("DISTINCT FROM without a right operand"))?;
    let left_node = node(Some(left))?;
    let right_node = node(Some(right))?;
    // PostgreSQL 18's own parser rewrites `x IS [NOT] DISTINCT FROM NULL`
    // into a plain `IS [NOT] NULL` whenever exactly one side is a bare,
    // uncast `NULL` token (probed: `y IS DISTINCT FROM NULL::text` keeps
    // its DISTINCT FROM form, an explicit cast is not bare; `NULL IS
    // DISTINCT FROM NULL` also keeps its form, since neither side is left
    // to build a null test from), never at typing time, so this admits
    // the same rewrite here, at the parse edge, rather than at
    // `Expr::DistinctFrom`.
    match (
        is_bare_null_const(left_node),
        is_bare_null_const(right_node),
    ) {
        (true, false) => Ok(Expr::IsNull(Box::new(admit(proof, right_node)?), !not)),
        (false, true) => Ok(Expr::IsNull(Box::new(admit(proof, left_node)?), !not)),
        _ => Ok(Expr::DistinctFrom(
            Box::new(admit(proof, left_node)?),
            Box::new(admit(proof, right_node)?),
            not,
            location,
        )),
    }
}

/// A raw parse-tree node that is exactly PostgreSQL's own bare `NULL`
/// token, not `NULL` cast to some type (a `TypeCast` wrapping one, a
/// different node entirely): `distinct_expr`'s one caller needs this
/// distinction the way `transformAExprDistinct` does, before `admit` has
/// turned either side into this crate's own `Expr`.
fn is_bare_null_const(node: &PgNode) -> bool {
    matches!(node, PgNode::AConst(constant) if constant.isnull)
}
