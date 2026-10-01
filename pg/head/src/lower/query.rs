use turso_core::Value;
use turso_parser::ast;

use crate::analyze::functions::FunctionHandle;
use crate::analyze::plan::{
    ResolvedFrom, ResolvedFromItem, ResolvedOrderItem, ResolvedOrderTarget, ResolvedQuery,
    ResolvedSimpleSelect, RowSecurityFact,
};
use crate::analyze::typing::{RelationSlot, Typed};
use crate::analyze::OutputSpec;
use crate::catalog::{Attnum, Backing, Catalog};
use crate::engine::{Command, Output};
use crate::error::HeadError;
use crate::ident::RoleName;
use crate::lower::expr::lower;
use crate::lower::sql::{self, Relation};
use crate::parse::statement::JoinKind;

pub(crate) fn select_query(
    query: &ResolvedQuery,
    output: &[OutputSpec],
    catalog: &Catalog,
    current_user: &RoleName,
) -> Result<Command, HeadError> {
    let mut params = Vec::new();
    let select = lower_query(query, Some(output), catalog, &mut params, current_user)?;
    Ok(Command {
        cmd: ast::Cmd::Stmt(ast::Stmt::Select(select)),
        params,
        output: Output::Rows,
    })
}

pub(super) fn lower_query(
    query: &ResolvedQuery,
    output: Option<&[OutputSpec]>,
    catalog: &Catalog,
    params: &mut Vec<Value>,
    current_user: &RoleName,
) -> Result<ast::Select, HeadError> {
    let select = lower_one_select(&query.first, output, catalog, params, current_user)?;
    let compounds = query
        .combined
        .iter()
        .map(|simple| {
            Ok(ast::CompoundSelect {
                operator: ast::CompoundOperator::UnionAll,
                select: lower_one_select(simple, None, catalog, params, current_user)?,
            })
        })
        .collect::<Result<Vec<_>, HeadError>>()?;
    if query.order_by.is_empty() {
        return Ok(ast::Select {
            with: None,
            body: ast::SelectBody { select, compounds },
            order_by: Vec::new(),
            limit: None,
        });
    }
    if compounds.is_empty() {
        let order_by = lower_order_items(&query.order_by, |target| match target {
            ResolvedOrderTarget::Expr(expr) => lower(expr, params, current_user, catalog),
            ResolvedOrderTarget::OutputPosition(index) => query
                .first
                .output
                .get(*index)
                .ok_or_else(|| {
                    HeadError::internal("an ORDER BY position falls outside the select list")
                })
                .and_then(|expr| lower(expr, params, current_user, catalog)),
        })?;
        return Ok(ast::Select {
            with: None,
            body: ast::SelectBody { select, compounds },
            order_by,
            limit: None,
        });
    }
    lower_compound_order_by(select, compounds, query)
}

const UNION_ORDER_ALIAS: &str = "u";

fn lower_compound_order_by(
    select: ast::OneSelect,
    compounds: Vec<ast::CompoundSelect>,
    query: &ResolvedQuery,
) -> Result<ast::Select, HeadError> {
    let mut inner = ast::Select {
        with: None,
        body: ast::SelectBody { select, compounds },
        order_by: Vec::new(),
        limit: None,
    };
    sql::name_result_columns_by_position(&mut inner);
    let from = ast::FromClause {
        select: Box::new(ast::SelectTable::Select(
            inner,
            Some(sql::as_alias(UNION_ORDER_ALIAS)),
        )),
        joins: Vec::new(),
    };
    let order_by = lower_order_items(&query.order_by, |target| match target {
        ResolvedOrderTarget::OutputPosition(index) => Ok(sql::qualified_column_ref(
            UNION_ORDER_ALIAS,
            Attnum::new(index + 1),
        )),
        ResolvedOrderTarget::Expr(_) => Err(HeadError::internal(
            "an arbitrary ORDER BY expression reached a compound query; analysis refuses this",
        )),
    })?;
    let outer = ast::OneSelect::Select {
        distinctness: None,
        columns: vec![ast::ResultColumn::Star],
        from: Some(from),
        where_clause: None,
        group_by: None,
        window_clause: Vec::new(),
    };
    Ok(ast::Select {
        with: None,
        body: ast::SelectBody {
            select: outer,
            compounds: Vec::new(),
        },
        order_by,
        limit: None,
    })
}

fn lower_order_items(
    items: &[ResolvedOrderItem],
    mut lower_target: impl FnMut(&ResolvedOrderTarget) -> Result<Box<ast::Expr>, HeadError>,
) -> Result<Vec<ast::SortedColumn>, HeadError> {
    items
        .iter()
        .map(|item| {
            Ok(ast::SortedColumn {
                expr: lower_target(&item.target)?,
                order: Some(if item.desc {
                    ast::SortOrder::Desc
                } else {
                    ast::SortOrder::Asc
                }),
                nulls: Some(if item.nulls_first {
                    ast::NullsOrder::First
                } else {
                    ast::NullsOrder::Last
                }),
            })
        })
        .collect()
}

fn lower_one_select(
    simple: &ResolvedSimpleSelect,
    output: Option<&[OutputSpec]>,
    catalog: &Catalog,
    params: &mut Vec<Value>,
    current_user: &RoleName,
) -> Result<ast::OneSelect, HeadError> {
    let from = simple
        .from
        .as_ref()
        .map(|from| lower_from(from, catalog, params, current_user))
        .transpose()?;
    let columns = lower_result_columns(&simple.output, output, params, current_user, catalog)?;
    let where_clause = simple
        .filter
        .as_ref()
        .map(|filter| lower(filter, params, current_user, catalog))
        .transpose()?;
    Ok(ast::OneSelect::Select {
        distinctness: simple.distinct.then_some(ast::Distinctness::Distinct),
        columns,
        from,
        where_clause,
        group_by: None,
        window_clause: Vec::new(),
    })
}

fn lower_result_columns(
    items: &[Typed],
    output: Option<&[OutputSpec]>,
    params: &mut Vec<Value>,
    current_user: &RoleName,
    catalog: &Catalog,
) -> Result<Vec<ast::ResultColumn>, HeadError> {
    let mut columns = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let spec = output.and_then(|specs| specs.get(index));
        match spec {
            Some(OutputSpec::CatalogRendered(_)) | Some(OutputSpec::SessionCall(_)) => {
                let Typed::Call(_, args) = item else {
                    return Err(HeadError::internal(
                        "a row-computed output spec was computed for a projection item that is not a call",
                    ));
                };
                for arg in args {
                    columns.push(ast::ResultColumn::Expr(
                        lower(arg, params, current_user, catalog)?,
                        None,
                    ));
                }
            }
            _ => columns.push(ast::ResultColumn::Expr(
                lower(item, params, current_user, catalog)?,
                None,
            )),
        }
    }
    Ok(columns)
}

fn lower_from(
    from: &ResolvedFrom,
    catalog: &Catalog,
    params: &mut Vec<Value>,
    current_user: &RoleName,
) -> Result<ast::FromClause, HeadError> {
    let select = Box::new(physical_select_table(
        &from.first,
        catalog,
        params,
        current_user,
    )?);
    let joins = from
        .joins
        .iter()
        .map(|join| {
            let table = Box::new(physical_select_table(
                &join.item,
                catalog,
                params,
                current_user,
            )?);
            let constraint = match &join.on {
                Some(on) => Some(ast::JoinConstraint::On(lower(
                    on,
                    params,
                    current_user,
                    catalog,
                )?)),
                None => None,
            };
            Ok(ast::JoinedSelectTable {
                operator: join_operator(join.kind),
                table,
                constraint,
            })
        })
        .collect::<Result<Vec<_>, HeadError>>()?;
    Ok(ast::FromClause { select, joins })
}

fn join_operator(kind: JoinKind) -> ast::JoinOperator {
    use ast::JoinType as JT;
    let flags = match kind {
        JoinKind::Inner | JoinKind::Cross => JT::INNER,
        JoinKind::Left => JT::LEFT | JT::OUTER,
    };
    ast::JoinOperator::TypedJoin(Some(flags))
}

pub(super) fn engine_alias(slot: RelationSlot) -> String {
    format!("t{}", slot.0)
}

fn physical_select_table(
    item: &ResolvedFromItem,
    catalog: &Catalog,
    params: &mut Vec<Value>,
    current_user: &RoleName,
) -> Result<ast::SelectTable, HeadError> {
    let alias = engine_alias(item.slot());
    match item {
        ResolvedFromItem::Relation {
            oid,
            backing,
            security,
            ..
        } => {
            let base = match backing {
                Backing::User => sql::aliased_table(Relation::Table(*oid), &alias),
                Backing::Catalog { constant, project } => {
                    let found = catalog.catalog_relation_by_oid(*oid).ok_or_else(|| {
                        HeadError::internal(
                            "a resolved catalog relation is missing from the catalog",
                        )
                    })?;
                    let columns: Vec<sql::Column> = found.attnums().map(Into::into).collect();
                    let mut relations = Vec::new();
                    if *constant {
                        relations.push(Relation::Constant(*oid));
                    }
                    if *project {
                        relations.push(Relation::Table(*oid));
                    }
                    match relations.as_slice() {
                        [] => sql::aliased_empty(&columns, &alias),
                        [only] => sql::aliased_table(*only, &alias),
                        _ => sql::aliased_union(&relations, &columns, &alias)?,
                    }
                }
            };
            match security {
                RowSecurityFact::Unfiltered => Ok(base),
                RowSecurityFact::Enforced(predicate) => {
                    let predicate = lower(predicate, params, current_user, catalog)?;
                    Ok(sql::policed(base, &alias, predicate))
                }
                RowSecurityFact::Refused => Err(HeadError::internal(
                    "a relation reached lowering with row security refused; enforcement must raise this first",
                )),
            }
        }
        ResolvedFromItem::Derived { query, .. } => {
            let mut select = lower_query(query, None, catalog, params, current_user)?;
            sql::name_result_columns_by_position(&mut select);
            Ok(ast::SelectTable::Select(
                select,
                Some(sql::as_alias(&alias)),
            ))
        }
        ResolvedFromItem::Function { handle, args, .. } => {
            table_function_call(*handle, args, &alias, params, current_user, catalog)
        }
    }
}

fn table_function_call(
    handle: FunctionHandle,
    args: &[Typed],
    alias: &str,
    params: &mut Vec<Value>,
    current_user: &RoleName,
    catalog: &Catalog,
) -> Result<ast::SelectTable, HeadError> {
    let arg_types = args
        .iter()
        .map(Typed::result_type)
        .collect::<Result<Vec<_>, _>>()?;
    let shape = handle.table_function_shape(&arg_types)?;
    let lowered_args = args
        .iter()
        .map(|arg| lower(arg, params, current_user, catalog).map(|expr| *expr))
        .collect::<Result<Vec<_>, _>>()?;
    let columns: Vec<sql::Column> = (1..=shape.columns.len())
        .map(|attnum| Attnum::new(attnum).into())
        .collect();
    Ok(match handle {
        FunctionHandle::Unnest => {
            sql::aliased_renamed_table_call("unnest", lowered_args, &["value"], &columns, alias)
        }
        FunctionHandle::PgOptionsToTable => {
            sql::aliased_table_call("pg_options_to_table", lowered_args, alias)
        }
        FunctionHandle::GenerateSeries => sql::aliased_renamed_table_call(
            "generate_series",
            lowered_args,
            &["value"],
            &columns,
            alias,
        ),
        FunctionHandle::CurrentDatabase
        | FunctionHandle::CurrentSchemas
        | FunctionHandle::CurrentSetting
        | FunctionHandle::SetConfig
        | FunctionHandle::CurrentSettingMissingOk
        | FunctionHandle::PgIsInRecovery
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
        | FunctionHandle::PgGetConstraintdef
        | FunctionHandle::PgGetConstraintdefPretty
        | FunctionHandle::PgGetIndexdef
        | FunctionHandle::PgGetIndexdefColumn
        | FunctionHandle::PgGetExpr
        | FunctionHandle::ArrayAgg => {
            return Err(HeadError::internal(
                "this function handle is not registered as a table function",
            ))
        }
    })
}
