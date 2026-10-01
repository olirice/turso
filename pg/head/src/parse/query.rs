use pg_query::protobuf::{
    node::Node as PgNode, Alias, FuncCall, InsertStmt, JoinExpr, JoinType as PgJoinType,
    LimitOption, Node, OverridingKind, RangeFunction, RangeSubselect, RangeVar, ResTarget,
    SelectStmt, SetOperation, SortBy, SortByDir, SortByNulls,
};

use super::{bare_relation_name, node, table_name, PROOF};
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::{ColumnName, FunctionName, TableName};
use crate::parse::expr::{self, EmptyArrayResult, Expr};
use crate::parse::statement::{
    FromClause, FromItem, Join, JoinKind, OrderItem, Query, SelectItem, SimpleSelect, Statement,
};
use crate::parse::Location;

pub(super) fn insert_rows(insert: &InsertStmt) -> Result<Statement, HeadError> {
    let InsertStmt {
        relation,
        cols,
        select_stmt,
        on_conflict_clause,
        returning_list,
        with_clause,
        r#override,
    } = insert;
    let insert_clause =
        |clause: &'static str| HeadError::not_supported(NotSupportedFeature::InsertClause(clause));
    if with_clause.is_some() {
        return Err(insert_clause("WITH"));
    }
    if on_conflict_clause.is_some() {
        return Err(insert_clause("ON CONFLICT"));
    }
    if !returning_list.is_empty() {
        return Err(insert_clause("RETURNING"));
    }
    if !matches!(
        OverridingKind::try_from(*r#override),
        Ok(OverridingKind::OverridingNotSet)
    ) {
        return Err(insert_clause("OVERRIDING"));
    }
    let relation = relation
        .as_ref()
        .ok_or_else(|| HeadError::internal("INSERT without a relation"))?;
    let columns = if cols.is_empty() {
        None
    } else {
        Some(
            cols.iter()
                .map(|target| {
                    let PgNode::ResTarget(target) = node(Some(target))? else {
                        return Err(HeadError::not_supported(
                            NotSupportedFeature::InsertIntoColumnSubscriptOrField,
                        ));
                    };
                    let ResTarget {
                        name,
                        indirection,
                        // `INSERT`'s own column list never carries a value,
                        // only a name: the value comes from the separate
                        // `VALUES` row instead.
                        val: _,
                        location,
                    } = &**target;
                    if !indirection.is_empty() {
                        return Err(HeadError::not_supported(
                            NotSupportedFeature::InsertIntoColumnSubscriptOrField,
                        ));
                    }
                    ColumnName::from_parse_tree(PROOF, name.clone())
                        .map(|name| (name, Location::from_raw(*location)))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    let values = match select_stmt.as_deref().map(|source| node(Some(source))) {
        Some(Ok(PgNode::SelectStmt(select))) if is_only_values(select) => &select.values_lists,
        _ => {
            return Err(HeadError::not_supported(
                NotSupportedFeature::InsertFromNonValues,
            ))
        }
    };
    let rows = values
        .iter()
        .map(|row| {
            let PgNode::List(list) = node(Some(row))? else {
                return Err(HeadError::internal("a VALUES row that is not a list"));
            };
            list.items
                .iter()
                .map(|item| expr::admit(PROOF, node(Some(item))?))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Statement::Insert {
        table: table_name(relation)?,
        columns,
        rows,
    })
}

fn is_only_values(select: &SelectStmt) -> bool {
    let SelectStmt {
        // A bare `VALUES (...)` leaf never sets any clause a `SELECT`
        // alone could (`target_list` is checked empty below instead); a
        // combining node (`UNION`/`INTERSECT`/`EXCEPT`) carries its arms
        // in `larg`/`rarg` and leaves `values_lists` itself empty, so
        // confirming `values_lists` is non-empty below already rules out
        // every one of these.
        distinct_clause: _,
        into_clause: _,
        target_list,
        from_clause,
        where_clause,
        group_clause: _,
        group_distinct: _,
        having_clause: _,
        window_clause: _,
        values_lists,
        sort_clause,
        limit_offset,
        limit_count,
        limit_option: _,
        locking_clause: _,
        with_clause,
        op: _,
        all: _,
        larg: _,
        rarg: _,
    } = select;
    !values_lists.is_empty()
        && target_list.is_empty()
        && from_clause.is_empty()
        && where_clause.is_none()
        && with_clause.is_none()
        && sort_clause.is_empty()
        && limit_count.is_none()
        && limit_offset.is_none()
}

/// `SelectStmt`'s fields split three ways by the query shape they belong
/// to, each its own function below: [`query`] owns the clauses that apply
/// once to the whole statement (`WITH`, locking, `LIMIT`/`OFFSET`, the
/// final `ORDER BY`); [`set_operation_arms`] owns the union operator
/// itself; [`simple_select`] owns one arm's own body. Every field is
/// destructured and bound in whichever of the three reads it, `_`-named
/// in the other two (a set-operation arm is itself a full `SelectStmt`, so
/// each function sees every field on every call, not only the ones it
/// cares about).
pub(super) fn query(select: &SelectStmt) -> Result<Query, HeadError> {
    let SelectStmt {
        distinct_clause: _,
        into_clause: _,
        target_list: _,
        from_clause: _,
        where_clause: _,
        group_clause: _,
        group_distinct: _,
        having_clause: _,
        window_clause: _,
        values_lists: _,
        sort_clause,
        limit_offset,
        limit_count,
        limit_option,
        locking_clause,
        with_clause,
        op: _,
        all: _,
        larg: _,
        rarg: _,
    } = select;
    if matches!(
        LimitOption::try_from(*limit_option),
        Ok(LimitOption::WithTies)
    ) {
        return Err(HeadError::not_supported(NotSupportedFeature::FetchWithTies));
    }
    if with_clause.is_some() {
        return Err(HeadError::not_supported(NotSupportedFeature::SelectClause(
            "WITH",
        )));
    }
    if !locking_clause.is_empty() {
        return Err(HeadError::not_supported(NotSupportedFeature::SelectClause(
            "FOR UPDATE or FOR SHARE",
        )));
    }
    if limit_count.is_some() || limit_offset.is_some() {
        return Err(HeadError::not_supported(NotSupportedFeature::LimitOrOffset));
    }
    let (first, combined) = set_operation_arms(select)?;
    Ok(Query {
        first,
        combined,
        order_by: order_by(sort_clause)?,
    })
}

fn set_operation_arms(select: &SelectStmt) -> Result<(SimpleSelect, Vec<SimpleSelect>), HeadError> {
    let SelectStmt {
        distinct_clause: _,
        into_clause: _,
        target_list: _,
        from_clause: _,
        where_clause: _,
        group_clause: _,
        group_distinct: _,
        having_clause: _,
        window_clause: _,
        values_lists: _,
        sort_clause: _,
        limit_offset: _,
        limit_count: _,
        limit_option: _,
        locking_clause: _,
        with_clause: _,
        op,
        all,
        larg,
        rarg,
    } = select;
    if matches!(SetOperation::try_from(*op), Ok(SetOperation::SetopNone)) {
        return Ok((simple_select(select)?, Vec::new()));
    }
    match (SetOperation::try_from(*op), all) {
        (Ok(SetOperation::SetopUnion), true) => {}
        (Ok(SetOperation::SetopUnion), false) => {
            return Err(HeadError::not_supported(NotSupportedFeature::SetOperation(
                "UNION",
            )))
        }
        (Ok(SetOperation::SetopIntersect), _) => {
            return Err(HeadError::not_supported(NotSupportedFeature::SetOperation(
                "INTERSECT",
            )))
        }
        (Ok(SetOperation::SetopExcept), _) => {
            return Err(HeadError::not_supported(NotSupportedFeature::SetOperation(
                "EXCEPT",
            )))
        }
        _ => return Err(HeadError::internal("a set operation with no operator")),
    };
    let left = larg
        .as_deref()
        .ok_or_else(|| HeadError::internal("a set operation without a left arm"))?;
    let right = rarg
        .as_deref()
        .ok_or_else(|| HeadError::internal("a set operation without a right arm"))?;
    let (first, mut combined) = set_operation_arms(left)?;
    combined.push(simple_select(right)?);
    Ok((first, combined))
}

fn simple_select(select: &SelectStmt) -> Result<SimpleSelect, HeadError> {
    let SelectStmt {
        distinct_clause,
        into_clause,
        target_list,
        from_clause,
        where_clause,
        group_clause,
        group_distinct,
        having_clause,
        window_clause,
        values_lists,
        // Read only by the `array_agg(... ORDER BY ...)` rewrite below,
        // which refuses it when the surrounding query also has its own
        // `ORDER BY`; the top-level statement's own `ORDER BY` is
        // `query`'s concern (this function runs once per set-operation
        // arm too, each with its own `sort_clause`).
        sort_clause,
        limit_offset: _,
        limit_count: _,
        limit_option: _,
        locking_clause,
        with_clause,
        op: _,
        all: _,
        larg: _,
        rarg: _,
    } = select;
    let select_clause =
        |clause: &'static str| HeadError::not_supported(NotSupportedFeature::SelectClause(clause));
    if into_clause.is_some() {
        return Err(select_clause("INTO"));
    }
    // `GROUP BY DISTINCT`'s own flag is only ever set alongside a
    // non-empty `group_clause`, refused unconditionally next to it.
    if !group_clause.is_empty() || *group_distinct {
        return Err(select_clause("GROUP BY"));
    }
    if having_clause.is_some() {
        return Err(select_clause("HAVING"));
    }
    if !window_clause.is_empty() {
        return Err(select_clause("WINDOW"));
    }
    if !values_lists.is_empty() {
        return Err(select_clause("VALUES"));
    }
    if with_clause.is_some() {
        return Err(select_clause("WITH"));
    }
    if !locking_clause.is_empty() {
        return Err(select_clause("FOR UPDATE or FOR SHARE"));
    }
    if target_list.is_empty() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::SelectWithNoColumns,
        ));
    }
    if let Some(rewritten) = array_agg_order_by_select(
        target_list,
        distinct_clause,
        from_clause,
        where_clause,
        sort_clause,
    )? {
        return Ok(rewritten);
    }
    let distinct = distinctness(distinct_clause)?;
    let from = match from_clause.as_slice() {
        [] => None,
        [first, rest @ ..] => Some(comma_joined_from_clause(first, rest)?),
    };
    Ok(SimpleSelect {
        distinct,
        items: select_items(target_list)?,
        from,
        filter: where_clause
            .as_deref()
            .map(|condition| expr::admit(PROOF, node(Some(condition))?))
            .transpose()?,
    })
}

fn array_agg_order_by_select(
    target_list: &[Node],
    distinct_clause: &[Node],
    from_clause: &[Node],
    where_clause: &Option<Box<Node>>,
    sort_clause: &[Node],
) -> Result<Option<SimpleSelect>, HeadError> {
    let [target] = target_list else {
        return Ok(None);
    };
    let PgNode::ResTarget(target) = node(Some(target))? else {
        return Ok(None);
    };
    let ResTarget {
        name: target_name,
        indirection,
        val,
        // See `select_items`: unused for the same reason.
        location: _,
    } = &**target;
    if !indirection.is_empty() {
        return Ok(None);
    }
    let PgNode::FuncCall(call) = node(val.as_deref())? else {
        return Ok(None);
    };
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
        // The special `ARRAY(SELECT ...)` this rewrite builds carries no
        // location of its own (`Expr::ArrayFromQuery`'s trailing `None`
        // below), the same as every other synthesized node this crate
        // builds rather than parses.
        location: _,
    } = &**call;
    if agg_order.is_empty() {
        return Ok(None);
    }
    if agg_filter.is_some()
        || over.is_some()
        || *agg_within_group
        || *agg_star
        || *agg_distinct
        || *func_variadic
        || !distinct_clause.is_empty()
    {
        return Err(HeadError::not_supported(
            NotSupportedFeature::AggregateOrderByWithAnotherModifier,
        ));
    }
    if !sort_clause.is_empty() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::AggregateOrderByWithQueryOrderBy,
        ));
    }
    let name = expr::function_name(PROOF, funcname)?;
    if name != FunctionName::array_agg() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::AggregateOrderByNotArrayAgg,
        ));
    }
    let [arg] = args.as_slice() else {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ArrayAggOrderByMultipleArgs,
        ));
    };
    let alias = if target_name.is_empty() {
        // An unaliased array_agg(... ORDER BY ...) still displays under its
        // own function name, the same as any other unaliased aggregate
        // call; PostgreSQL 18 does this regardless of the ORDER BY rewrite
        // this function applies below.
        Some(ColumnName::array_agg())
    } else {
        Some(ColumnName::from_parse_tree(PROOF, target_name.clone())?)
    };
    let inner_from = match from_clause {
        [] => None,
        [first, rest @ ..] => Some(comma_joined_from_clause(first, rest)?),
    };
    let inner = Query {
        first: SimpleSelect {
            distinct: false,
            items: vec![SelectItem::Expr(
                expr::admit(PROOF, node(Some(arg))?)?,
                None,
            )],
            from: inner_from,
            filter: where_clause
                .as_deref()
                .map(|condition| expr::admit(PROOF, node(Some(condition))?))
                .transpose()?,
        },
        combined: Vec::new(),
        order_by: order_by(agg_order)?,
    };
    Ok(Some(SimpleSelect {
        distinct: false,
        items: vec![SelectItem::Expr(
            Expr::ArrayFromQuery(Box::new(inner), EmptyArrayResult::Null, None),
            alias,
        )],
        from: None,
        filter: None,
    }))
}

fn distinctness(distinct_clause: &[Node]) -> Result<bool, HeadError> {
    if distinct_clause.is_empty() {
        return Ok(false);
    }
    if distinct_clause.iter().any(|item| item.node.is_some()) {
        return Err(HeadError::not_supported(NotSupportedFeature::DistinctOn));
    }
    Ok(true)
}

fn select_items(targets: &[Node]) -> Result<Vec<SelectItem>, HeadError> {
    targets
        .iter()
        .map(|target| {
            let PgNode::ResTarget(target) = node(Some(target))? else {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::SelectListSubscriptOrField,
                ));
            };
            let ResTarget {
                name,
                indirection,
                val,
                // No PostgreSQL error is reported at a select item's own
                // position; every position this head reports instead comes
                // from the expression inside it.
                location: _,
            } = &**target;
            if !indirection.is_empty() {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::SelectListSubscriptOrField,
                ));
            }
            let alias = if name.is_empty() {
                None
            } else {
                Some(ColumnName::from_parse_tree(PROOF, name.clone())?)
            };
            select_item(node(val.as_deref())?, alias)
        })
        .collect()
}

fn select_item(value: &PgNode, alias: Option<ColumnName>) -> Result<SelectItem, HeadError> {
    if alias.is_none() {
        if let PgNode::ColumnRef(reference) = value {
            match reference.fields.as_slice() {
                [star] => {
                    if let PgNode::AStar(_) = node(Some(star))? {
                        return Ok(SelectItem::AllColumns);
                    }
                }
                [qualifier, star] => {
                    if let (PgNode::String(name), PgNode::AStar(_)) =
                        (node(Some(qualifier))?, node(Some(star))?)
                    {
                        return Ok(SelectItem::AllColumnsOf(TableName::from_parse_tree(
                            PROOF,
                            name.sval.clone(),
                        )?));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(SelectItem::Expr(expr::admit(PROOF, value)?, alias))
}

fn comma_joined_from_clause(first: &Node, rest: &[Node]) -> Result<FromClause, HeadError> {
    let mut clause = from_clause(first)?;
    for node_ref in rest {
        let joined = from_clause(node_ref)?;
        clause.joins.push(Join {
            kind: JoinKind::Cross,
            item: joined.first,
            on: None,
        });
        clause.joins.extend(joined.joins);
    }
    Ok(clause)
}

fn from_clause(node_ref: &Node) -> Result<FromClause, HeadError> {
    if let PgNode::JoinExpr(join_expr) = node(Some(node_ref))? {
        let mut joins = Vec::new();
        let first = flatten_join_tree(join_expr, &mut joins)?;
        return Ok(FromClause { first, joins });
    }
    Ok(FromClause {
        first: from_operand(node_ref)?,
        joins: Vec::new(),
    })
}

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "pg_query's Node oneof enumerates every node kind PostgreSQL's parser can produce; this refuses every kind not explicitly admitted here, never silently accepting one"
)]
fn from_operand(node_ref: &Node) -> Result<FromItem, HeadError> {
    match node(Some(node_ref))? {
        PgNode::RangeVar(relation) => from_item_table(relation),
        PgNode::RangeSubselect(sub) => from_item_derived(sub),
        PgNode::RangeFunction(function) => from_item_function(function),
        PgNode::JoinExpr(_) => Err(HeadError::not_supported(
            NotSupportedFeature::ParenthesizedJoinRhs,
        )),
        _ => Err(HeadError::not_supported(
            NotSupportedFeature::JoinOperandNotTable,
        )),
    }
}

fn flatten_join_tree(join_expr: &JoinExpr, joins: &mut Vec<Join>) -> Result<FromItem, HeadError> {
    let JoinExpr {
        jointype,
        is_natural,
        larg,
        rarg,
        using_clause,
        // Only ever set alongside a non-empty `using_clause` (a `JOIN ...
        // USING (...) AS alias`), which `using_clause` below already
        // refuses unconditionally regardless of this field's value.
        join_using_alias: _,
        quals,
        alias,
        // The parsed join's range-table index: always `0` until the
        // analyzer assigns one, never set by the raw parser.
        rtindex: _,
    } = join_expr;
    // A join this head admits has no way to scope column resolution to an
    // alias covering the whole joined pair (`FromItem`/the relation walk
    // model a join as its flattened list of tables, not a single named
    // relation), and PostgreSQL itself then hides the inner tables' own
    // names once one is given, so this head cannot reproduce that
    // behavior; refuse rather than silently keep both names visible.
    if alias.is_some() {
        return Err(HeadError::not_supported(NotSupportedFeature::AliasedJoin));
    }
    let left = larg
        .as_deref()
        .ok_or_else(|| HeadError::internal("a JOIN without a left side"))?;
    let first = if let PgNode::JoinExpr(nested) = node(Some(left))? {
        flatten_join_tree(nested, joins)?
    } else {
        from_operand(left)?
    };
    let right = rarg
        .as_deref()
        .ok_or_else(|| HeadError::internal("a JOIN without a right side"))?;
    let item = from_operand(right)?;
    if *is_natural {
        return Err(HeadError::not_supported(NotSupportedFeature::NaturalJoin));
    }
    if !using_clause.is_empty() {
        return Err(HeadError::not_supported(NotSupportedFeature::JoinUsing));
    }
    let pg_kind = PgJoinType::try_from(*jointype).unwrap_or(PgJoinType::Undefined);
    let (kind, needs_on) = match pg_kind {
        PgJoinType::JoinInner if quals.is_none() => (JoinKind::Cross, false),
        PgJoinType::JoinInner => (JoinKind::Inner, true),
        PgJoinType::JoinLeft => (JoinKind::Left, true),
        PgJoinType::JoinRight => {
            return Err(HeadError::not_supported(NotSupportedFeature::RightJoin))
        }
        PgJoinType::JoinFull => {
            return Err(HeadError::not_supported(NotSupportedFeature::FullJoin))
        }
        PgJoinType::Undefined
        | PgJoinType::JoinSemi
        | PgJoinType::JoinAnti
        | PgJoinType::JoinRightAnti
        | PgJoinType::JoinUniqueOuter
        | PgJoinType::JoinUniqueInner => {
            return Err(HeadError::not_supported(NotSupportedFeature::ThisJoinType))
        }
    };
    let on = quals
        .as_deref()
        .map(|quals| expr::admit(PROOF, node(Some(quals))?))
        .transpose()?;
    if needs_on && on.is_none() {
        return Err(HeadError::not_supported(NotSupportedFeature::JoinWithoutOn));
    }
    joins.push(Join { kind, item, on });
    Ok(first)
}

fn order_by(sort_clause: &[Node]) -> Result<Vec<OrderItem>, HeadError> {
    sort_clause
        .iter()
        .map(|item| {
            let PgNode::SortBy(sort_by) = node(Some(item))? else {
                return Err(HeadError::internal("an ORDER BY item that is not a SortBy"));
            };
            let SortBy {
                node: expr_node,
                sortby_dir,
                sortby_nulls,
                // Only ever populated for `SortbyUsing`, refused
                // unconditionally below regardless of its content.
                use_op: _,
                // No PostgreSQL error is reported at an `ORDER BY` item's
                // own position; every position this head reports instead
                // comes from the sort expression itself.
                location: _,
            } = &**sort_by;
            let expr_node = expr_node
                .as_deref()
                .ok_or_else(|| HeadError::internal("ORDER BY without an expression"))?;
            let expr = expr::admit(PROOF, node(Some(expr_node))?)?;
            let desc = match SortByDir::try_from(*sortby_dir) {
                Ok(SortByDir::SortbyDesc) => true,
                Ok(SortByDir::SortbyAsc | SortByDir::SortbyDefault) => false,
                Ok(SortByDir::SortbyUsing) => {
                    return Err(HeadError::not_supported(NotSupportedFeature::OrderByUsing))
                }
                _ => return Err(HeadError::internal("ORDER BY with no direction")),
            };
            let nulls_first = match SortByNulls::try_from(*sortby_nulls) {
                Ok(SortByNulls::SortbyNullsFirst) => true,
                Ok(SortByNulls::SortbyNullsLast) => false,
                Ok(SortByNulls::SortbyNullsDefault) => desc,
                _ => return Err(HeadError::internal("ORDER BY with no NULLS placement")),
            };
            Ok(OrderItem {
                expr,
                desc,
                nulls_first,
            })
        })
        .collect()
}

fn from_item_table(relation: &RangeVar) -> Result<FromItem, HeadError> {
    let name = bare_relation_name(relation)?;
    let alias = match &relation.alias {
        None => None,
        Some(alias) => {
            if !alias.colnames.is_empty() {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::ColumnAliasesOnTableReference,
                ));
            }
            Some(TableName::from_parse_tree(PROOF, alias.aliasname.clone())?)
        }
    };
    Ok(FromItem::Table {
        relation: name,
        alias,
    })
}

fn alias_columns(alias: &Alias) -> Result<Option<Vec<ColumnName>>, HeadError> {
    let Alias {
        // Read separately by every caller of this function, straight off
        // the same `Alias`, for the table alias itself.
        aliasname: _,
        colnames,
    } = alias;
    if colnames.is_empty() {
        return Ok(None);
    }
    colnames
        .iter()
        .map(|name| {
            let PgNode::String(name) = node(Some(name))? else {
                return Err(HeadError::internal("a column alias that is not a name"));
            };
            ColumnName::from_parse_tree(PROOF, name.sval.clone())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn from_item_derived(sub: &RangeSubselect) -> Result<FromItem, HeadError> {
    let RangeSubselect {
        lateral,
        subquery,
        alias,
    } = sub;
    if *lateral {
        return Err(HeadError::not_supported(NotSupportedFeature::Lateral));
    }
    let subquery = subquery
        .as_deref()
        .ok_or_else(|| HeadError::internal("a subquery FROM item without a query"))?;
    let PgNode::SelectStmt(select) = node(Some(subquery))? else {
        return Err(HeadError::internal(
            "a subquery FROM item that is not a SELECT",
        ));
    };
    let inner = query(select)?;
    let alias = alias
        .as_ref()
        .ok_or_else(|| HeadError::raise(PgError::SubqueryInFromRequiresAlias))?;
    Ok(FromItem::Derived {
        query: Box::new(inner),
        alias: TableName::from_parse_tree(PROOF, alias.aliasname.clone())?,
        columns: alias_columns(alias)?,
    })
}

fn from_item_function(function: &RangeFunction) -> Result<FromItem, HeadError> {
    let RangeFunction {
        lateral,
        ordinality,
        // `ROWS FROM (...)` with exactly one function (checked via the
        // slice pattern below regardless) produces the same output as a
        // bare function call in `FROM`, live-checked against PostgreSQL
        // 18; this flag only changes anything when combined with more
        // than one function, which the slice pattern already refuses.
        is_rowsfrom: _,
        functions,
        alias,
        coldeflist,
    } = function;
    if *lateral {
        return Err(HeadError::not_supported(NotSupportedFeature::Lateral));
    }
    if *ordinality {
        return Err(HeadError::not_supported(
            NotSupportedFeature::WithOrdinality,
        ));
    }
    if !coldeflist.is_empty() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ColumnDefinitionList,
        ));
    }
    let [entry] = functions.as_slice() else {
        return Err(HeadError::not_supported(
            NotSupportedFeature::RowsFromMultipleFunctions,
        ));
    };
    let PgNode::List(list) = node(Some(entry))? else {
        return Err(HeadError::internal(
            "a table function entry that is not a list",
        ));
    };
    let (call, coldeflist) = match list.items.as_slice() {
        [call, coldeflist] => (call, coldeflist),
        _ => {
            return Err(HeadError::internal(
                "a table function entry that is not a (call, column list) pair",
            ))
        }
    };
    if coldeflist.node.is_some() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ColumnDefinitionList,
        ));
    }
    let PgNode::FuncCall(call) = node(Some(call))? else {
        return Err(HeadError::not_supported(
            NotSupportedFeature::ThisTableFunction,
        ));
    };
    let (name, args) = expr::func_call(PROOF, call)?;
    let alias = alias.as_ref();
    let table_alias = alias
        .map(|alias| TableName::from_parse_tree(PROOF, alias.aliasname.clone()))
        .transpose()?;
    let columns = alias.map(alias_columns).transpose()?.flatten();
    let location = Location::from_raw(call.location)
        .ok_or_else(|| HeadError::internal("a parsed table function call has no location"))?;
    Ok(FromItem::Function {
        name,
        args,
        alias: table_alias,
        columns,
        location,
    })
}
