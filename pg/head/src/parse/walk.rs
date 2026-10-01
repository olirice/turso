use crate::error::HeadError;
use crate::parse::expr::Expr;
use crate::parse::statement::{
    FromClause, FromItem, Join, OrderItem, Query, SelectItem, SimpleSelect,
};

pub(crate) trait ExprMap {
    fn map(&mut self, expr: Expr) -> Result<Expr, HeadError>;

    fn map_query(&mut self, query: Query) -> Result<Query, HeadError> {
        walk_query(self, query)
    }
}

pub(crate) fn walk_expr_map<M: ExprMap + ?Sized>(
    mapper: &mut M,
    expr: Expr,
) -> Result<Expr, HeadError> {
    Ok(match expr {
        leaf @ (Expr::Column(..)
        | Expr::CurrentUser
        | Expr::Literal(..)
        | Expr::Param(..)
        | Expr::Resolved(_)) => leaf,
        Expr::Cast(inner, ty) => Expr::Cast(Box::new(mapper.map(*inner)?), ty),
        Expr::Not(inner) => Expr::Not(Box::new(mapper.map(*inner)?)),
        Expr::And(items) => Expr::And(map_all(mapper, items)?),
        Expr::Or(items) => Expr::Or(map_all(mapper, items)?),
        Expr::Compare(op, left, right, location) => Expr::Compare(
            op,
            Box::new(mapper.map(*left)?),
            Box::new(mapper.map(*right)?),
            location,
        ),
        Expr::IsNull(inner, negated) => Expr::IsNull(Box::new(mapper.map(*inner)?), negated),
        Expr::Is(inner, truth, negated) => Expr::Is(Box::new(mapper.map(*inner)?), truth, negated),
        Expr::DistinctFrom(left, right, negated, location) => Expr::DistinctFrom(
            Box::new(mapper.map(*left)?),
            Box::new(mapper.map(*right)?),
            negated,
            location,
        ),
        Expr::In(needle, list, negated, location) => Expr::In(
            Box::new(mapper.map(*needle)?),
            map_all(mapper, list)?,
            negated,
            location,
        ),
        Expr::Concat(left, right, location) => Expr::Concat(
            Box::new(mapper.map(*left)?),
            Box::new(mapper.map(*right)?),
            location,
        ),
        Expr::Case {
            base,
            arms,
            otherwise,
        } => Expr::Case {
            base: base
                .map(|base| mapper.map(*base))
                .transpose()?
                .map(Box::new),
            arms: arms
                .into_iter()
                .map(|(when, then)| Ok((mapper.map(when)?, mapper.map(then)?)))
                .collect::<Result<Vec<_>, HeadError>>()?,
            otherwise: otherwise
                .map(|otherwise| mapper.map(*otherwise))
                .transpose()?
                .map(Box::new),
        },
        Expr::Subquery(query, location) => {
            Expr::Subquery(Box::new(mapper.map_query(*query)?), location)
        }
        Expr::Exists(query, location) => {
            Expr::Exists(Box::new(mapper.map_query(*query)?), location)
        }
        Expr::InSelect(needle, query, negated, location) => Expr::InSelect(
            Box::new(mapper.map(*needle)?),
            Box::new(mapper.map_query(*query)?),
            negated,
            location,
        ),
        Expr::ArrayFromQuery(query, empty_result, location) => {
            Expr::ArrayFromQuery(Box::new(mapper.map_query(*query)?), empty_result, location)
        }
        Expr::ArrayLiteral(elements) => Expr::ArrayLiteral(map_all(mapper, elements)?),
        Expr::Call(name, args, location) => Expr::Call(name, map_all(mapper, args)?, location),
        Expr::Subscript(base, index) => {
            Expr::Subscript(Box::new(mapper.map(*base)?), Box::new(mapper.map(*index)?))
        }
        Expr::AnyEq(needle, array, location) => Expr::AnyEq(
            Box::new(mapper.map(*needle)?),
            Box::new(mapper.map(*array)?),
            location,
        ),
    })
}

fn map_all<M: ExprMap + ?Sized>(mapper: &mut M, items: Vec<Expr>) -> Result<Vec<Expr>, HeadError> {
    items.into_iter().map(|item| mapper.map(item)).collect()
}

pub(crate) fn walk_query<M: ExprMap + ?Sized>(
    mapper: &mut M,
    query: Query,
) -> Result<Query, HeadError> {
    Ok(Query {
        first: map_simple_select(mapper, query.first)?,
        combined: query
            .combined
            .into_iter()
            .map(|simple| map_simple_select(mapper, simple))
            .collect::<Result<_, HeadError>>()?,
        order_by: query
            .order_by
            .into_iter()
            .map(|item| {
                Ok(OrderItem {
                    expr: mapper.map(item.expr)?,
                    desc: item.desc,
                    nulls_first: item.nulls_first,
                })
            })
            .collect::<Result<_, HeadError>>()?,
    })
}

fn map_simple_select<M: ExprMap + ?Sized>(
    mapper: &mut M,
    simple: SimpleSelect,
) -> Result<SimpleSelect, HeadError> {
    Ok(SimpleSelect {
        distinct: simple.distinct,
        items: simple
            .items
            .into_iter()
            .map(|item| map_select_item(mapper, item))
            .collect::<Result<_, HeadError>>()?,
        from: simple
            .from
            .map(|from| map_from_clause(mapper, from))
            .transpose()?,
        filter: simple.filter.map(|filter| mapper.map(filter)).transpose()?,
    })
}

fn map_select_item<M: ExprMap + ?Sized>(
    mapper: &mut M,
    item: SelectItem,
) -> Result<SelectItem, HeadError> {
    Ok(match item {
        SelectItem::Expr(expr, alias) => SelectItem::Expr(mapper.map(expr)?, alias),
        other @ (SelectItem::AllColumns | SelectItem::AllColumnsOf(_)) => other,
    })
}

fn map_from_clause<M: ExprMap + ?Sized>(
    mapper: &mut M,
    from: FromClause,
) -> Result<FromClause, HeadError> {
    Ok(FromClause {
        first: map_from_item(mapper, from.first)?,
        joins: from
            .joins
            .into_iter()
            .map(|join| map_join(mapper, join))
            .collect::<Result<_, HeadError>>()?,
    })
}

fn map_join<M: ExprMap + ?Sized>(mapper: &mut M, join: Join) -> Result<Join, HeadError> {
    Ok(Join {
        kind: join.kind,
        item: map_from_item(mapper, join.item)?,
        on: join.on.map(|on| mapper.map(on)).transpose()?,
    })
}

fn map_from_item<M: ExprMap + ?Sized>(
    mapper: &mut M,
    item: FromItem,
) -> Result<FromItem, HeadError> {
    Ok(match item {
        table @ FromItem::Table { .. } => table,
        FromItem::Derived {
            query,
            alias,
            columns,
        } => FromItem::Derived {
            query: Box::new(mapper.map_query(*query)?),
            alias,
            columns,
        },
        FromItem::Function {
            name,
            args,
            alias,
            columns,
            location,
        } => FromItem::Function {
            name,
            args: map_all(mapper, args)?,
            alias,
            columns,
            location,
        },
    })
}
