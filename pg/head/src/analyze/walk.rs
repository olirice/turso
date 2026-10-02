use std::collections::BTreeMap;
use std::rc::Rc;

use crate::analyze::functions::FunctionHandle;
use crate::analyze::plan::{
    ResolvedFrom, ResolvedFromItem, ResolvedJoin, ResolvedOrderItem, ResolvedOrderTarget,
    ResolvedQuery, ResolvedSimpleSelect,
};
use crate::analyze::typing::{self, RelationShape, RelationSlot, Scope, ScopeRelation, Typed};
use crate::catalog::{Backing, Column, Oid, Table};
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::{ColumnName, FunctionName, TableName};
use crate::parse::expr::{Expr, Literal};
use crate::parse::statement::{
    FromClause, FromItem, Join, OrderItem, Query, RelationName, RelationSchema, SelectItem,
    SimpleSelect,
};
use crate::parse::Location;
use crate::security::row_security::{self, RowSecurityDecision};

use super::Lookup;

mod subquery;

use subquery::type_check_expr;

pub(crate) struct RelationFact {
    pub(crate) oid: Oid,
    pub(crate) name: TableName,
    pub(crate) owner: Oid,
    pub(crate) checked_as: Oid,
    pub(crate) refused: bool,
}

pub(crate) struct TableRef {
    pub(crate) oid: Oid,
    pub(crate) name: TableName,
    pub(crate) owner: Oid,
    pub(crate) backing: Backing,
}

pub(super) struct WalkCtx<'a> {
    pub(super) lookup: &'a Lookup<'a>,
    pub(super) next_slot: usize,
    pub(super) references: Vec<RelationFact>,
    pub(super) slot_relations: BTreeMap<RelationSlot, RelationShape<'a>>,
    pub(super) privilege_actor: Oid,
}

impl<'a> WalkCtx<'a> {
    fn allocate_slot(&mut self) -> RelationSlot {
        let slot = RelationSlot(self.next_slot);
        self.next_slot += 1;
        slot
    }
}

pub(super) fn resolve_query<'a>(
    query: Query,
    ctx: &mut WalkCtx<'a>,
    outer: Option<&Scope<'a, '_>>,
) -> Result<(ResolvedQuery, Vec<ColumnName>), HeadError> {
    let (first, first_relations, names) = resolve_simple_select(query.first, ctx, outer)?;
    let arm_len = first.output.len();
    let mut combined = Vec::with_capacity(query.combined.len());
    for simple in query.combined {
        let (resolved, _, _) = resolve_simple_select(simple, ctx, outer)?;
        if resolved.output.len() != arm_len {
            return Err(HeadError::raise(PgError::UnionColumnCountMismatch));
        }
        combined.push(resolved);
    }
    let order_by = if query.order_by.is_empty() {
        Vec::new()
    } else {
        let is_compound = !combined.is_empty();
        let plain_scope = (!is_compound).then_some(Scope {
            relations: first_relations,
            outer,
        });
        let order_by_cx = typing::Context::new(typing::Position::OrderBy);
        query
            .order_by
            .into_iter()
            .map(|item| {
                let OrderItem {
                    expr,
                    desc,
                    nulls_first,
                } = item;
                let target = match expr {
                    Expr::Literal(Literal::Integer(position), location) => {
                        let index = ordinal_output_index(position, arm_len)
                            .map_err(|error| error.at(location))?;
                        let output = first.output.get(index).ok_or_else(|| {
                            HeadError::internal(
                                "ORDER BY position resolved outside the output list",
                            )
                        })?;
                        typing::refuse_row_computed_in_order_by_reference(output)?;
                        ResolvedOrderTarget::OutputPosition(index)
                    }
                    Expr::Column(None, column_name, location) => {
                        match output_alias_index(&names, &column_name) {
                            Some(index) => {
                                let output = first.output.get(index).ok_or_else(|| {
                                    HeadError::internal(
                                        "ORDER BY alias resolved outside the output list",
                                    )
                                })?;
                                typing::refuse_row_computed_in_order_by_reference(output)?;
                                ResolvedOrderTarget::OutputPosition(index)
                            }
                            None => match &plain_scope {
                                Some(scope) => {
                                    let expr = type_check_expr(
                                        Expr::Column(None, column_name, location),
                                        scope,
                                        ctx,
                                        &order_by_cx,
                                    )?;
                                    ResolvedOrderTarget::Expr(expr)
                                }
                                None => {
                                    return Err(undefined_order_column(&column_name).at(location))
                                }
                            },
                        }
                    }
                    other @ Expr::Column(..)
                    | other @ Expr::CurrentUser
                    | other @ Expr::Literal(..)
                    | other @ Expr::Param(..)
                    | other @ Expr::Cast(..)
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
                    | other @ Expr::Resolved(_) => match &plain_scope {
                        Some(scope) => {
                            let expr = type_check_expr(other, scope, ctx, &order_by_cx)?;
                            ResolvedOrderTarget::Expr(expr)
                        }
                        None => {
                            return Err(HeadError::not_supported(
                                NotSupportedFeature::UnionOrderByExpression,
                            ))
                        }
                    },
                };
                Ok(ResolvedOrderItem {
                    target,
                    desc,
                    nulls_first,
                })
            })
            .collect::<Result<Vec<_>, HeadError>>()?
    };
    Ok((
        ResolvedQuery {
            first,
            combined,
            order_by,
        },
        names,
    ))
}

fn resolve_simple_select<'a>(
    simple: SimpleSelect,
    ctx: &mut WalkCtx<'a>,
    outer: Option<&Scope<'a, '_>>,
) -> Result<
    (
        ResolvedSimpleSelect,
        Vec<ScopeRelation<'a>>,
        Vec<ColumnName>,
    ),
    HeadError,
> {
    let (from, relations) = match simple.from {
        None => (None, Vec::new()),
        Some(from_clause) => {
            let (from, relations) = resolve_from_clause(from_clause, ctx, outer)?;
            (Some(from), relations)
        }
    };
    let scope = Scope { relations, outer };
    if simple.items.is_empty() {
        return Err(HeadError::not_supported(
            NotSupportedFeature::SelectWithNoColumns,
        ));
    }
    let items = expand_select_items(simple.items, &scope)?;
    let names: Vec<ColumnName> = items
        .iter()
        .map(|(item, alias)| match alias {
            Some(alias) => Ok(alias.clone()),
            None => Ok(crate::parse::expr::select_item_display_name(item)?
                .unwrap_or_else(|| ColumnName::literal("?column?"))),
        })
        .collect::<Result<Vec<_>, HeadError>>()?;
    let select_target_cx = typing::Context::new(typing::Position::SelectTarget);
    let output = items
        .into_iter()
        .map(|(item, _)| type_check_expr(item, &scope, ctx, &select_target_cx))
        .collect::<Result<Vec<_>, _>>()?;
    if output.iter().any(typing::contains_aggregate) {
        for item in &output {
            typing::refuse_ungrouped_columns(item, &scope)?;
        }
    }
    for item in &output {
        typing::refuse_row_computed_in_select_item(item)?;
    }
    let where_cx = typing::Context::new(typing::Position::Where);
    let filter = simple
        .filter
        .map(|filter| type_check_expr(filter, &scope, ctx, &where_cx))
        .transpose()?;
    if let Some(filter) = &filter {
        where_cx.require_boolean(filter)?;
    }
    Ok((
        ResolvedSimpleSelect {
            distinct: simple.distinct,
            output,
            from,
            filter,
        },
        scope.relations,
        names,
    ))
}

type SelectExpr = (Expr, Option<ColumnName>);

fn expand_select_items(
    items: Vec<SelectItem>,
    scope: &Scope<'_, '_>,
) -> Result<Vec<SelectExpr>, HeadError> {
    let mut expanded = Vec::new();
    for item in items {
        match item {
            SelectItem::Expr(expr, alias) => expanded.push((expr, alias)),
            SelectItem::AllColumns => {
                if scope.relations.is_empty() {
                    return Err(HeadError::not_supported(
                        NotSupportedFeature::StarWithoutFromClause,
                    ));
                }
                for relation in &scope.relations {
                    expand_relation_columns(relation, &mut expanded)?;
                }
            }
            SelectItem::AllColumnsOf(name) => {
                let relation = scope
                    .relations
                    .iter()
                    .find(|relation| relation.visible_as == name)
                    .ok_or_else(|| typing::missing_from_clause_entry(&name))?;
                expand_relation_columns(relation, &mut expanded)?;
            }
        }
    }
    Ok(expanded)
}

fn expand_relation_columns(
    relation: &ScopeRelation,
    expanded: &mut Vec<SelectExpr>,
) -> Result<(), HeadError> {
    for attnum in relation.shape.attnums() {
        let (name, _) = relation.shape.column_at(attnum)?;
        expanded.push((
            Expr::Column(Some(relation.visible_as.clone()), name, None),
            None,
        ));
    }
    Ok(())
}

fn resolve_from_clause<'a>(
    from: FromClause,
    ctx: &mut WalkCtx<'a>,
    outer: Option<&Scope<'a, '_>>,
) -> Result<(ResolvedFrom, Vec<ScopeRelation<'a>>), HeadError> {
    let (first_relation, first_item) = resolve_from_item(from.first, ctx, outer)?;
    let mut relations = vec![first_relation];
    let mut joins = Vec::with_capacity(from.joins.len());
    for join in from.joins {
        joins.push(resolve_join(join, ctx, &mut relations, outer)?);
    }
    Ok((
        ResolvedFrom {
            first: first_item,
            joins,
        },
        relations,
    ))
}

fn resolve_join<'a>(
    join: Join,
    ctx: &mut WalkCtx<'a>,
    relations: &mut Vec<ScopeRelation<'a>>,
    outer: Option<&Scope<'a, '_>>,
) -> Result<ResolvedJoin, HeadError> {
    let (relation, item) = resolve_from_item(join.item, ctx, outer)?;
    relations.push(relation);
    let on = match join.on {
        Some(on_expr) => {
            let scope = Scope {
                relations: relations.clone(),
                outer,
            };
            let join_on_cx = typing::Context::new(typing::Position::JoinOn);
            let typed = type_check_expr(on_expr, &scope, ctx, &join_on_cx)?;
            join_on_cx.require_boolean(&typed)?;
            Some(typed)
        }
        None => None,
    };
    Ok(ResolvedJoin {
        kind: join.kind,
        item,
        on,
    })
}

fn resolve_from_item<'a>(
    item: FromItem,
    ctx: &mut WalkCtx<'a>,
    outer: Option<&Scope<'a, '_>>,
) -> Result<(ScopeRelation<'a>, ResolvedFromItem), HeadError> {
    match item {
        FromItem::Table { relation, alias } => resolve_table_ref(&relation, alias, ctx),
        FromItem::Derived {
            query,
            alias,
            columns,
        } => resolve_derived_ref(*query, alias, columns, ctx, outer),
        FromItem::Function {
            name,
            args,
            alias,
            columns,
            location,
        } => resolve_function_ref(name, args, alias, columns, location, ctx, outer),
    }
}

fn resolve_table_ref<'a>(
    relation: &RelationName,
    alias: Option<TableName>,
    ctx: &mut WalkCtx<'a>,
) -> Result<(ScopeRelation<'a>, ResolvedFromItem), HeadError> {
    let found = match table(relation, ctx.lookup) {
        Ok(found) => found,
        Err(error) => {
            return match view_for(relation)? {
                Some(view) => resolve_view_ref(view, relation.name.clone(), alias, ctx),
                // PostgreSQL 18's query analyzer attaches a FROM item's own
                // position to its "relation does not exist" error, unlike
                // a utility command's relation lookup (ALTER TABLE, GRANT,
                // LOCK, CREATE POLICY), which never carries one.
                None => Err(error.at(relation.location)),
            };
        }
    };
    let slot = ctx.allocate_slot();
    let visible_as = alias.unwrap_or_else(|| relation.name.clone());
    let (reference, security) = reference_for(
        found,
        &relation.name,
        ctx.privilege_actor,
        row_security::Access::Read,
        ctx.lookup,
    )?;
    let security = security.retargeted(slot);
    ctx.references.push(reference);
    let shape = RelationShape::Catalog(found);
    ctx.slot_relations.insert(slot, shape.clone());
    Ok((
        ScopeRelation {
            slot,
            visible_as,
            shape,
        },
        ResolvedFromItem::Relation {
            slot,
            oid: found.oid,
            backing: found.backing,
            security,
        },
    ))
}

pub(super) fn view_for(
    relation: &RelationName,
) -> Result<Option<crate::analyze::views::ViewDef>, HeadError> {
    match relation.schema {
        RelationSchema::Public => Ok(None),
        RelationSchema::PgCatalog | RelationSchema::Unqualified => {
            crate::analyze::views::lookup(&relation.name)
        }
    }
}

fn resolve_view_ref<'a>(
    view: crate::analyze::views::ViewDef,
    relation_name: TableName,
    alias: Option<TableName>,
    ctx: &mut WalkCtx<'a>,
) -> Result<(ScopeRelation<'a>, ResolvedFromItem), HeadError> {
    let visible_as = alias.unwrap_or_else(|| relation_name.clone());
    ctx.references.push(RelationFact {
        oid: view.oid,
        name: relation_name,
        owner: view.owner,
        checked_as: ctx.privilege_actor,
        refused: false,
    });
    let previous_actor = ctx.privilege_actor;
    ctx.privilege_actor = view.owner;
    let result = resolve_derived_ref(view.query, visible_as, None, ctx, None);
    ctx.privilege_actor = previous_actor;
    result
}

fn resolve_derived_ref<'a>(
    inner: Query,
    alias: TableName,
    columns: Option<Vec<ColumnName>>,
    ctx: &mut WalkCtx<'a>,
    outer: Option<&Scope<'a, '_>>,
) -> Result<(ScopeRelation<'a>, ResolvedFromItem), HeadError> {
    let (resolved, names) = resolve_query(inner, ctx, outer)?;
    let width = resolved.first.output.len();
    let column_names = match columns {
        Some(explicit) => {
            if explicit.len() != width {
                return Err(HeadError::raise(
                    PgError::ColumnListLengthMismatchDerivedTable,
                ));
            }
            explicit
        }
        None => names,
    };
    let column_defs: Vec<Column> = column_names
        .into_iter()
        .zip(&resolved.first.output)
        .map(|(name, item)| {
            Ok(Column {
                name,
                ty: item.result_type()?,
                not_null: false,
            })
        })
        .collect::<Result<Vec<_>, HeadError>>()?;
    let slot = ctx.allocate_slot();
    let shape = RelationShape::Synthetic(Rc::new(column_defs));
    ctx.slot_relations.insert(slot, shape.clone());
    Ok((
        ScopeRelation {
            slot,
            visible_as: alias,
            shape,
        },
        ResolvedFromItem::Derived {
            slot,
            query: Box::new(resolved),
        },
    ))
}

fn resolve_function_ref<'a>(
    name: FunctionName,
    args: Vec<Expr>,
    alias: Option<TableName>,
    columns: Option<Vec<ColumnName>>,
    location: Location,
    ctx: &mut WalkCtx<'a>,
    outer: Option<&Scope<'a, '_>>,
) -> Result<(ScopeRelation<'a>, ResolvedFromItem), HeadError> {
    let empty_scope = Scope {
        relations: Vec::new(),
        outer,
    };
    let from_function_cx = typing::Context::new(typing::Position::FromFunction);
    let typed_args = args
        .into_iter()
        .map(|arg| type_check_expr(arg, &empty_scope, ctx, &from_function_cx))
        .collect::<Result<Vec<_>, _>>()?;
    let arg_types = typed_args
        .iter()
        .map(Typed::result_type)
        .collect::<Result<Vec<_>, _>>()?;
    let handle = FunctionHandle::lookup_table_function(&name, arg_types.len())
        .ok_or_else(|| typing::undefined_function(name.as_str(), &arg_types).at(location))?;
    let shape = handle.table_function_shape(&arg_types)?;
    let default_name = TableName::literal(shape.default_name);
    let rename_sole_column_to_alias = alias.is_some() && shape.columns.len() == 1;
    let visible_as = alias.unwrap_or(default_name);
    let column_defs: Vec<Column> = match columns {
        Some(explicit) => {
            if explicit.len() != shape.columns.len() {
                return Err(HeadError::raise(PgError::ColumnListLengthMismatchFunction));
            }
            explicit
                .into_iter()
                .zip(shape.columns)
                .map(|(name, (_, ty))| Column {
                    name,
                    ty,
                    not_null: false,
                })
                .collect()
        }
        None => shape
            .columns
            .into_iter()
            .map(|(name, ty)| Column {
                name: if rename_sole_column_to_alias {
                    ColumnName::literal(visible_as.as_str().to_string())
                } else {
                    ColumnName::literal(name)
                },
                ty,
                not_null: false,
            })
            .collect(),
    };
    let slot = ctx.allocate_slot();
    let column_shape = RelationShape::Synthetic(Rc::new(column_defs));
    ctx.slot_relations.insert(slot, column_shape.clone());
    Ok((
        ScopeRelation {
            slot,
            visible_as,
            shape: column_shape,
        },
        ResolvedFromItem::Function {
            slot,
            handle,
            args: typed_args,
        },
    ))
}

/// A relation reference's row-security decision (`security::row_security::decide`,
/// the one place it is made) alongside the bookkeeping `authorization` and
/// `refuse_when_row_security_is_off` need for it: the one mechanism both
/// `resolve_table_ref` (`Access::Read`) and `analyze::ddl::insert`
/// (`Access::Insert`) go through, so an insert's target is checked the
/// same way a read's relation reference is.
pub(super) fn reference_for(
    found: &Table,
    relation_name: &TableName,
    checked_as: Oid,
    access: row_security::Access,
    lookup: &Lookup,
) -> Result<(RelationFact, RowSecurityDecision), HeadError> {
    let security = row_security::decide(found, lookup.role, access, lookup.row_security)?;
    let reference = RelationFact {
        oid: found.oid,
        name: relation_name.clone(),
        owner: found.owner,
        checked_as,
        refused: security.is_refused(),
    };
    Ok((reference, security))
}

pub(super) fn table<'a>(
    relation: &RelationName,
    lookup: &Lookup<'a>,
) -> Result<&'a Table, HeadError> {
    let found = match relation.schema {
        RelationSchema::PgCatalog => lookup.catalog.catalog_relation(&relation.name),
        RelationSchema::Public => lookup.catalog.table(&relation.name),
        RelationSchema::Unqualified => {
            lookup.catalog.catalog_relation(&relation.name).or_else(|| {
                lookup
                    .catalog
                    .table(&relation.name)
                    .filter(|_| lookup.search_path_includes_public)
            })
        }
    };
    found.ok_or_else(|| {
        HeadError::raise(PgError::UndefinedRelation {
            qualified: relation.name.render_message().as_message().to_string(),
        })
    })
}

pub(super) fn table_ref(relation: &RelationName, table: &Table) -> TableRef {
    TableRef {
        oid: table.oid,
        name: relation.name.clone(),
        owner: table.owner,
        backing: table.backing,
    }
}

fn output_alias_index(names: &[ColumnName], name: &ColumnName) -> Option<usize> {
    let mut found = None;
    for (index, candidate) in names.iter().enumerate() {
        if candidate == name {
            if found.is_some() {
                return None;
            }
            found = Some(index);
        }
    }
    found
}

fn ordinal_output_index(position: i64, arity: usize) -> Result<usize, HeadError> {
    let index = usize::try_from(position)
        .ok()
        .and_then(|position| position.checked_sub(1))
        .filter(|index| *index < arity);
    index.ok_or_else(|| HeadError::raise(PgError::OrderByPositionNotInSelectList(position)))
}

fn undefined_order_column(name: &ColumnName) -> HeadError {
    HeadError::raise(PgError::UndefinedColumn {
        qualifier: None,
        column: name.clone(),
    })
}
