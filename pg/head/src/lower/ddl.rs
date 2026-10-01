use turso_core::Value;
use turso_parser::ast;

use crate::catalog::{Attnum, Oid};
use crate::engine::{Command, Output};
use crate::error::HeadError;
use crate::lower::sql::{self, Relation};
use crate::parse::statement::ColumnDef;

pub(crate) fn create_table(oid: Oid, columns: &[ColumnDef]) -> Result<Command, HeadError> {
    let columns: Result<Vec<_>, _> = (1..)
        .zip(columns)
        .map(|(attnum, column)| column_definition(attnum, column))
        .collect();
    let columns = columns?;
    Ok(Command {
        cmd: sql::create_table(Relation::Table(oid), false, columns, Vec::new()),
        params: Vec::new(),
        output: Output::Nothing,
    })
}

pub(crate) fn insert(oid: Oid, rows: Vec<Vec<Value>>) -> Result<Command, HeadError> {
    let width = rows.first().map_or(0, Vec::len);
    let columns: Vec<sql::Column> = (1..=width)
        .map(|attnum| Attnum::new(attnum).into())
        .collect();
    let (cmd, params) = sql::insert(Relation::Table(oid), false, &columns, rows)?;
    Ok(Command {
        cmd,
        params,
        output: Output::Nothing,
    })
}

fn column_definition(
    attnum: usize,
    column: &ColumnDef,
) -> Result<ast::ColumnDefinition, HeadError> {
    let engine = column.ty.engine()?;
    Ok(sql::column(
        Attnum::new(attnum),
        engine.element,
        sql::ColumnConstraints {
            primary_key: column.primary_key.is_some(),
            not_null: column.not_null,
            unique: false,
        },
    ))
}
