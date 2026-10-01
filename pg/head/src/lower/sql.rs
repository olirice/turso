use std::num::NonZeroU32;

use turso_core::Value;
use turso_parser::ast;

use crate::catalog::pg::{ColumnId, EngineStorageType};
use crate::catalog::{Attnum, Oid};
use crate::error::HeadError;

#[derive(Clone, Copy)]
pub(crate) enum Relation {
    Table(Oid),
    Index(Oid),
    HeadState,
    EngineSchema,
    Constant(Oid),
}

#[derive(Clone, Copy)]
pub(crate) enum Column {
    Attnum(Attnum),
    HeadState(HeadStateColumn),
    EngineSchemaName,
}

#[derive(Clone, Copy)]
pub(crate) enum HeadStateColumn {
    Id,
    NextOid,
    CatalogVersion,
    FormatVersion,
}

impl From<Attnum> for Column {
    fn from(attnum: Attnum) -> Self {
        Column::Attnum(attnum)
    }
}

impl From<ColumnId> for Column {
    fn from(column: ColumnId) -> Self {
        Column::Attnum(column.attnum())
    }
}

pub(crate) fn engine_name(relation: Relation) -> String {
    match relation {
        Relation::Table(oid) => format!("rel_{}", oid.get()),
        Relation::Index(oid) => format!("idx_{}", oid.get()),
        Relation::HeadState => "head_internal_state".to_string(),
        Relation::EngineSchema => "sqlite_master".to_string(),
        Relation::Constant(oid) => format!("const_{}", oid.get()),
    }
}

fn column_name(column: Column) -> String {
    match column {
        Column::Attnum(attnum) => format!("col_{}", attnum.get()),
        Column::HeadState(HeadStateColumn::Id) => "id".to_string(),
        Column::HeadState(HeadStateColumn::NextOid) => "next_oid".to_string(),
        Column::HeadState(HeadStateColumn::CatalogVersion) => "catalog_version".to_string(),
        Column::HeadState(HeadStateColumn::FormatVersion) => "format_version".to_string(),
        Column::EngineSchemaName => "name".to_string(),
    }
}

pub(crate) fn engine_type_keyword(ty: EngineStorageType) -> &'static str {
    match ty {
        EngineStorageType::Boolean => "BOOLEAN",
        EngineStorageType::SmallInt => "SMALLINT",
        EngineStorageType::Integer => "INTEGER",
        EngineStorageType::Real => "REAL",
        EngineStorageType::Text => "TEXT",
        EngineStorageType::Name => "NAME",
        EngineStorageType::TimestampTz => "TIMESTAMPTZ",
        EngineStorageType::Blob => "BLOB",
        EngineStorageType::Char => "\"char\"",
    }
}

pub(crate) fn engine_type_decode_function(ty: EngineStorageType) -> Option<&'static str> {
    match ty {
        EngineStorageType::Char => Some("char_out"),
        EngineStorageType::Boolean
        | EngineStorageType::SmallInt
        | EngineStorageType::Integer
        | EngineStorageType::Real
        | EngineStorageType::Text
        | EngineStorageType::Name
        | EngineStorageType::TimestampTz
        | EngineStorageType::Blob => None,
    }
}

fn ident(name: &str) -> ast::Name {
    ast::Name::exact(name.to_string())
}

fn qualified(relation: Relation) -> ast::QualifiedName {
    ast::QualifiedName::single(ident(&engine_name(relation)))
}

pub(crate) fn column_ref(column: impl Into<Column>) -> Box<ast::Expr> {
    Box::new(ast::Expr::Id(ident(&column_name(column.into()))))
}

pub(crate) fn qualified_column_ref(alias: &str, column: impl Into<Column>) -> Box<ast::Expr> {
    Box::new(ast::Expr::Qualified(
        ident(alias),
        ident(&column_name(column.into())),
    ))
}

pub(crate) fn as_alias(alias: &str) -> ast::As {
    ast::As::As(ident(alias))
}

pub(crate) fn name_the_sole_result_column(select: &mut ast::Select, name: impl Into<Column>) {
    let name = column_name(name.into());
    if let ast::OneSelect::Select { columns, .. } = &mut select.body.select {
        if let [ast::ResultColumn::Expr(_, alias)] = columns.as_mut_slice() {
            *alias = Some(ast::As::As(ident(&name)));
        }
    }
}

pub(crate) fn name_result_columns_by_position(select: &mut ast::Select) {
    if let ast::OneSelect::Select { columns, .. } = &mut select.body.select {
        for (index, column) in columns.iter_mut().enumerate() {
            if let ast::ResultColumn::Expr(_, alias) = column {
                let name = column_name(Column::Attnum(Attnum::new(index + 1)));
                *alias = Some(ast::As::As(ident(&name)));
            }
        }
    }
}

pub(crate) fn aliased_table(relation: Relation, alias: &str) -> ast::SelectTable {
    ast::SelectTable::Table(qualified(relation), Some(ast::As::As(ident(alias))), None)
}

pub(crate) fn aliased_union(
    relations: &[Relation],
    columns: &[Column],
    alias: &str,
) -> Result<ast::SelectTable, HeadError> {
    let (first, rest) = relations
        .split_first()
        .ok_or_else(|| HeadError::internal("a union of layers needs at least one relation"))?;
    let exprs =
        || -> Vec<ast::Expr> { columns.iter().map(|column| *column_ref(*column)).collect() };
    let compounds = rest
        .iter()
        .map(|relation| ast::CompoundSelect {
            operator: ast::CompoundOperator::UnionAll,
            select: one_select(*relation, exprs(), None),
        })
        .collect();
    let select = ast::Select {
        with: None,
        body: ast::SelectBody {
            select: one_select(*first, exprs(), None),
            compounds,
        },
        order_by: Vec::new(),
        limit: None,
    };
    Ok(ast::SelectTable::Select(
        select,
        Some(ast::As::As(ident(alias))),
    ))
}

pub(crate) fn aliased_empty(columns: &[Column], alias: &str) -> ast::SelectTable {
    let result_columns = columns
        .iter()
        .map(|column| {
            ast::ResultColumn::Expr(
                Box::new(ast::Expr::Literal(ast::Literal::Null)),
                Some(ast::As::As(ident(&column_name(*column)))),
            )
        })
        .collect();
    let select = ast::Select {
        with: None,
        body: ast::SelectBody {
            select: ast::OneSelect::Select {
                distinctness: None,
                columns: result_columns,
                from: None,
                where_clause: Some(Box::new(ast::Expr::Literal(ast::Literal::Numeric(
                    "0".to_string(),
                )))),
                group_by: None,
                window_clause: Vec::new(),
            },
            compounds: Vec::new(),
        },
        order_by: Vec::new(),
        limit: None,
    };
    ast::SelectTable::Select(select, Some(ast::As::As(ident(alias))))
}

pub(crate) fn aliased_table_call(
    name: &'static str,
    args: Vec<ast::Expr>,
    alias: &str,
) -> ast::SelectTable {
    ast::SelectTable::TableCall(
        ast::QualifiedName::single(ident(name)),
        args.into_iter().map(Box::new).collect(),
        Some(ast::As::As(ident(alias))),
    )
}

pub(crate) fn aliased_renamed_table_call(
    name: &'static str,
    args: Vec<ast::Expr>,
    source_columns: &[&'static str],
    columns: &[Column],
    alias: &str,
) -> ast::SelectTable {
    let result_columns = source_columns
        .iter()
        .zip(columns)
        .map(|(source, column)| {
            ast::ResultColumn::Expr(
                Box::new(ast::Expr::Id(ident(source))),
                Some(ast::As::As(ident(&column_name(*column)))),
            )
        })
        .collect();
    let select = ast::Select {
        with: None,
        body: ast::SelectBody {
            select: ast::OneSelect::Select {
                distinctness: None,
                columns: result_columns,
                from: Some(ast::FromClause {
                    select: Box::new(ast::SelectTable::TableCall(
                        ast::QualifiedName::single(ident(name)),
                        args.into_iter().map(Box::new).collect(),
                        None,
                    )),
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
    ast::SelectTable::Select(select, Some(ast::As::As(ident(alias))))
}

pub(crate) fn policed(
    base: ast::SelectTable,
    alias: &str,
    predicate: Box<ast::Expr>,
) -> ast::SelectTable {
    let select = ast::Select {
        with: None,
        body: ast::SelectBody {
            select: ast::OneSelect::Select {
                distinctness: None,
                columns: vec![ast::ResultColumn::Star],
                from: Some(ast::FromClause {
                    select: Box::new(base),
                    joins: Vec::new(),
                }),
                where_clause: Some(predicate),
                group_by: None,
                window_clause: Vec::new(),
            },
            compounds: Vec::new(),
        },
        order_by: Vec::new(),
        limit: None,
    };
    ast::SelectTable::Select(select, Some(ast::As::As(ident(alias))))
}

pub(crate) fn bind(params: &mut Vec<Value>, value: Value) -> Result<Box<ast::Expr>, HeadError> {
    params.push(value);
    let index = u32::try_from(params.len())
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| {
            HeadError::internal("a statement has more parameters than the engine can number")
        })?;
    Ok(Box::new(ast::Expr::Variable(ast::Variable::indexed(index))))
}

pub(crate) fn function_call(name: &'static str, args: Vec<ast::Expr>) -> Box<ast::Expr> {
    Box::new(ast::Expr::FunctionCall {
        name: ast::Name::from_bytes(name.as_bytes()),
        distinctness: None,
        args: args.into_iter().map(Box::new).collect(),
        order_by: Vec::new(),
        within_group: Vec::new(),
        filter_over: ast::FunctionTail {
            filter_clause: None,
            over_clause: None,
        },
    })
}

pub(crate) fn array(
    params: &mut Vec<Value>,
    values: Vec<Value>,
) -> Result<Box<ast::Expr>, HeadError> {
    let elements = values
        .into_iter()
        .map(|value| bind(params, value).map(|expr| *expr))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(function_call("array", elements))
}

pub(crate) fn array_length(column: impl Into<Column>) -> Box<ast::Expr> {
    function_call("array_length", vec![*column_ref(column)])
}

pub(crate) fn array_element(column: impl Into<Column>, index: i64) -> Box<ast::Expr> {
    function_call(
        "array_element",
        vec![
            *column_ref(column),
            ast::Expr::Literal(ast::Literal::Numeric(index.to_string())),
        ],
    )
}

pub(crate) fn array_index_at(array: Box<ast::Expr>, index: Box<ast::Expr>) -> Box<ast::Expr> {
    function_call("array_element", vec![*array, *index])
}

pub(crate) fn array_length_at(array: Box<ast::Expr>, dim: Box<ast::Expr>) -> Box<ast::Expr> {
    function_call("array_length", vec![*array, *dim])
}

pub(crate) fn array_contains_value(array: Box<ast::Expr>, value: Box<ast::Expr>) -> Box<ast::Expr> {
    function_call("array_contains", vec![*array, *value])
}

pub(crate) fn array_agg_of(value: Box<ast::Expr>) -> Box<ast::Expr> {
    function_call("array_agg", vec![*value])
}

pub(crate) fn coalesce(value: Box<ast::Expr>, fallback: Box<ast::Expr>) -> Box<ast::Expr> {
    function_call("coalesce", vec![*value, *fallback])
}

pub(crate) fn nullif(value: Box<ast::Expr>, sentinel: Box<ast::Expr>) -> Box<ast::Expr> {
    function_call("nullif", vec![*value, *sentinel])
}

pub(crate) fn integer_literal(value: i64) -> Box<ast::Expr> {
    Box::new(ast::Expr::Literal(ast::Literal::Numeric(value.to_string())))
}

pub(crate) fn equals(
    column: impl Into<Column>,
    params: &mut Vec<Value>,
    value: Value,
) -> Result<Box<ast::Expr>, HeadError> {
    Ok(Box::new(ast::Expr::Binary(
        column_ref(column),
        ast::Operator::Equals,
        bind(params, value)?,
    )))
}

pub(crate) fn and_all(conditions: Vec<ast::Expr>) -> Option<Box<ast::Expr>> {
    conditions
        .into_iter()
        .map(Box::new)
        .reduce(|left, right| Box::new(ast::Expr::Binary(left, ast::Operator::And, right)))
}

fn sorted_columns(columns: &[Column]) -> Vec<ast::SortedColumn> {
    columns
        .iter()
        .map(|column| ast::SortedColumn {
            expr: column_ref(*column),
            order: None,
            nulls: None,
        })
        .collect()
}

/// The column-level constraints `column` and `array_column` can render;
/// bundled into one type so neither function takes more than one bool
/// parameter of its own (`array`).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ColumnConstraints {
    pub(crate) primary_key: bool,
    pub(crate) not_null: bool,
    pub(crate) unique: bool,
}

pub(crate) fn column(
    name: impl Into<Column>,
    ty: EngineStorageType,
    constraints: ColumnConstraints,
) -> ast::ColumnDefinition {
    array_column(name, ty, false, constraints)
}

pub(crate) fn array_column(
    name: impl Into<Column>,
    ty: EngineStorageType,
    array: bool,
    constraints: ColumnConstraints,
) -> ast::ColumnDefinition {
    let ColumnConstraints {
        primary_key,
        not_null,
        unique,
    } = constraints;
    let mut rendered = Vec::new();
    if primary_key {
        rendered.push(unnamed(ast::ColumnConstraint::PrimaryKey {
            order: None,
            conflict_clause: None,
            auto_increment: false,
        }));
    }
    if primary_key || not_null {
        rendered.push(unnamed(ast::ColumnConstraint::NotNull {
            nullable: false,
            conflict_clause: None,
        }));
    }
    if unique {
        rendered.push(unnamed(ast::ColumnConstraint::Unique(None)));
    }
    ast::ColumnDefinition {
        col_name: ident(&column_name(name.into())),
        col_type: Some(ast::Type {
            name: engine_type_keyword(ty).to_string(),
            size: None,
            array_dimensions: u32::from(array),
        }),
        constraints: rendered,
    }
}

fn unnamed(constraint: ast::ColumnConstraint) -> ast::NamedColumnConstraint {
    ast::NamedColumnConstraint {
        name: None,
        constraint,
    }
}

pub(crate) fn create_table(
    table: Relation,
    if_not_exists: bool,
    columns: Vec<ast::ColumnDefinition>,
    constraints: Vec<ast::NamedTableConstraint>,
) -> ast::Cmd {
    ast::Cmd::Stmt(ast::Stmt::CreateTable {
        temporary: false,
        if_not_exists,
        tbl_name: qualified(table),
        body: ast::CreateTableBody::ColumnsAndConstraints {
            columns,
            constraints,
            options: ast::TableOptions {
                without_rowid_text: None,
                strict_text: Some("STRICT".to_string()),
            },
        },
    })
}

#[cfg(test)]
pub(crate) fn drop_table(table: Relation, if_exists: bool) -> ast::Cmd {
    ast::Cmd::Stmt(ast::Stmt::DropTable {
        if_exists,
        tbl_name: qualified(table),
    })
}

pub(crate) fn create_index(
    index: Relation,
    table: Relation,
    unique: bool,
    columns: &[Column],
) -> ast::Cmd {
    ast::Cmd::Stmt(ast::Stmt::CreateIndex {
        unique,
        if_not_exists: false,
        idx_name: qualified(index),
        tbl_name: ident(&engine_name(table)),
        using: None,
        columns: sorted_columns(columns),
        with_clause: Vec::new(),
        where_clause: None,
    })
}

pub(crate) enum Cell {
    Scalar(Value),
    Array(Vec<Value>),
}

pub(crate) fn insert(
    table: Relation,
    or_ignore: bool,
    columns: &[Column],
    rows: Vec<Vec<Value>>,
) -> Result<(ast::Cmd, Vec<Value>), HeadError> {
    insert_cells(
        table,
        or_ignore,
        columns,
        rows.into_iter()
            .map(|row| row.into_iter().map(Cell::Scalar).collect())
            .collect(),
    )
}

pub(crate) fn insert_cells(
    table: Relation,
    or_ignore: bool,
    columns: &[Column],
    rows: Vec<Vec<Cell>>,
) -> Result<(ast::Cmd, Vec<Value>), HeadError> {
    let mut params = Vec::new();
    let value_rows = rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| match cell {
                    Cell::Scalar(value) => bind(&mut params, value),
                    Cell::Array(values) => array(&mut params, values),
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let select = ast::Select {
        with: None,
        body: ast::SelectBody {
            select: ast::OneSelect::Values(value_rows),
            compounds: Vec::new(),
        },
        order_by: Vec::new(),
        limit: None,
    };
    let cmd = ast::Cmd::Stmt(ast::Stmt::Insert {
        with: None,
        or_conflict: if or_ignore {
            Some(ast::ResolveType::Ignore)
        } else {
            None
        },
        tbl_name: qualified(table),
        columns: columns
            .iter()
            .map(|column| ident(&column_name(*column)))
            .collect(),
        body: ast::InsertBody::Select(select, None),
        returning: Vec::new(),
    });
    Ok((cmd, params))
}

pub(crate) fn select(
    table: Relation,
    columns: &[Column],
    filter: Option<Box<ast::Expr>>,
    order_by: &[Column],
) -> ast::Cmd {
    select_exprs(
        table,
        columns.iter().map(|column| *column_ref(*column)).collect(),
        filter,
        order_by,
    )
}

fn one_select(
    table: Relation,
    columns: Vec<ast::Expr>,
    filter: Option<Box<ast::Expr>>,
) -> ast::OneSelect {
    let result_columns = columns
        .into_iter()
        .map(|expr| ast::ResultColumn::Expr(Box::new(expr), None))
        .collect();
    let from = Some(ast::FromClause {
        select: Box::new(ast::SelectTable::Table(qualified(table), None, None)),
        joins: Vec::new(),
    });
    ast::OneSelect::Select {
        distinctness: None,
        columns: result_columns,
        from,
        where_clause: filter,
        group_by: None,
        window_clause: Vec::new(),
    }
}

pub(crate) fn select_exprs(
    table: Relation,
    columns: Vec<ast::Expr>,
    filter: Option<Box<ast::Expr>>,
    order_by: &[Column],
) -> ast::Cmd {
    ast::Cmd::Stmt(ast::Stmt::Select(ast::Select {
        with: None,
        body: ast::SelectBody {
            select: one_select(table, columns, filter),
            compounds: Vec::new(),
        },
        order_by: sorted_columns(order_by),
        limit: None,
    }))
}

pub(crate) fn update(
    table: Relation,
    sets: Vec<(Column, Box<ast::Expr>)>,
    filter: Box<ast::Expr>,
) -> ast::Cmd {
    ast::Cmd::Stmt(ast::Stmt::Update(ast::Update {
        with: None,
        or_conflict: None,
        tbl_name: qualified(table),
        indexed: None,
        sets: sets
            .into_iter()
            .map(|(column, expr)| ast::Set {
                col_names: vec![ident(&column_name(column))],
                expr,
            })
            .collect(),
        from: None,
        where_clause: Some(filter),
        returning: Vec::new(),
    }))
}

pub(crate) fn delete(table: Relation, filter: Box<ast::Expr>) -> ast::Cmd {
    ast::Cmd::Stmt(ast::Stmt::Delete {
        with: None,
        tbl_name: qualified(table),
        indexed: None,
        where_clause: Some(filter),
        returning: Vec::new(),
    })
}
