pub(crate) mod constant;
pub(crate) mod constant_table;
pub(crate) mod store;

use std::num::NonZero;
use std::sync::Arc;

use turso_core::{Connection, LimboError, Value};
use turso_parser::ast;

use crate::catalog::{Catalog, Oid, PrimaryKey};
#[cfg(test)]
use crate::error::SqlState;
use crate::error::{HeadError, PgError};
use crate::ident::ColumnName;
use crate::lower;
use crate::lower::sql::{self, Column, Relation};
use crate::pipeline::{Engine, InsertRows, Lowered, ResultShape};
use crate::render;
use crate::security::enforcement;
use crate::session::SessionEffect;
use crate::Outcome;

pub(crate) struct EngineConnection {
    conn: Arc<Connection>,
}

pub(crate) enum Output {
    Nothing,
    Rows,
}

pub(crate) struct Command {
    pub(crate) cmd: ast::Cmd,
    pub(crate) params: Vec<Value>,
    pub(crate) output: Output,
}

pub(crate) fn open(conn: Arc<Connection>) -> EngineConnection {
    EngineConnection { conn }
}

impl EngineConnection {
    pub(crate) fn is_auto_commit(&self) -> bool {
        self.conn.get_auto_commit()
    }

    pub(crate) fn begin_deferred(&self) -> Result<(), HeadError> {
        self.control(ast::Stmt::Begin {
            typ: Some(ast::TransactionType::Deferred),
            name: None,
        })
    }

    pub(crate) fn begin_immediate(&self) -> Result<(), HeadError> {
        self.control(ast::Stmt::Begin {
            typ: Some(ast::TransactionType::Immediate),
            name: None,
        })
    }

    pub(crate) fn commit(&self) -> Result<(), HeadError> {
        self.control(ast::Stmt::Commit { name: None })
    }

    pub(crate) fn rollback(&self) -> Result<(), HeadError> {
        self.control(ast::Stmt::Rollback {
            tx_name: None,
            savepoint_name: None,
        })
    }

    fn control(&self, stmt: ast::Stmt) -> Result<(), HeadError> {
        self.write(ast::Cmd::Stmt(stmt), Vec::new())
    }

    fn write(&self, cmd: ast::Cmd, params: Vec<Value>) -> Result<(), HeadError> {
        self.run(Command {
            cmd,
            params,
            output: Output::Nothing,
        })
        .map(|_| ())
    }

    fn query(&self, cmd: ast::Cmd, params: Vec<Value>) -> Result<Vec<Vec<Value>>, HeadError> {
        self.run(Command {
            cmd,
            params,
            output: Output::Rows,
        })
    }

    fn run(&self, command: Command) -> Result<Vec<Vec<Value>>, HeadError> {
        let input = command.cmd.to_string();
        let mut statement = self.conn.prepare_translated_cmd(command.cmd, &input)?;
        for (index, param) in command.params.into_iter().enumerate() {
            let position = NonZero::new(index + 1)
                .ok_or_else(|| HeadError::internal("index + 1 is never zero"))?;
            statement.bind_at(position, param)?;
        }
        let outcome = match command.output {
            Output::Nothing => statement.run_ignore_rows().map(|()| Vec::new()),
            Output::Rows => statement.run_collect_rows(),
        };
        outcome.map_err(map_engine_error)
    }
}

pub(crate) fn execute(
    connection: &EngineConnection,
    catalog: &Catalog,
    lowered: Lowered,
) -> Result<(Outcome, Vec<SessionEffect>), HeadError> {
    let (command, catalog_writes, result, mut effects) = lowered.into_parts();
    let values = match command {
        Engine::None => Vec::new(),
        Engine::One(command) => connection.run(*command)?,
        Engine::InsertRows(insert_rows) => run_insert_rows(connection, catalog, insert_rows)?,
    };
    let outcome = match result {
        ResultShape::Command(tag) => Outcome::Command(tag),
        ResultShape::Insert(rows) => Outcome::Inserted(rows),
        ResultShape::Rows {
            columns,
            output,
            mut settings,
        } => Outcome::Rows {
            values: render::rows(&output, values, catalog, &mut settings, &mut effects)?,
            columns,
        },
    };
    let wrote_to_catalog = !catalog_writes.is_empty();
    for write in catalog_writes {
        store::apply(connection, write)?;
    }
    if wrote_to_catalog {
        store::bump_catalog_version(connection)?;
    }
    Ok((outcome, effects))
}

fn run_insert_rows(
    connection: &EngineConnection,
    catalog: &Catalog,
    insert_rows: InsertRows,
) -> Result<Vec<Vec<Value>>, HeadError> {
    let InsertRows {
        table,
        columns,
        rows,
    } = insert_rows;
    let found = catalog
        .table(&table.name)
        .ok_or_else(|| HeadError::internal("an insert target table is missing from the catalog"))?;
    let primary_key = match &found.primary_key {
        Some(key) => {
            let column = found.columns.get(key.attnum.get() - 1).ok_or_else(|| {
                HeadError::internal("a primary key attnum falls outside its table's columns")
            })?;
            Some((key, &column.name))
        }
        None => None,
    };
    for row in rows {
        enforcement::check_row_not_null(&table, &columns, &row)?;
        if let Some((key, column_name)) = primary_key {
            let value = row.get(key.attnum.get() - 1).ok_or_else(|| {
                HeadError::internal("an insert row is narrower than its table's columns")
            })?;
            check_row_unique(connection, table.oid, key, column_name, value)?;
        }
        let command = lower::insert(table.oid, vec![row])?;
        connection.run(command)?;
    }
    Ok(Vec::new())
}

fn check_row_unique(
    connection: &EngineConnection,
    table_oid: Oid,
    key: &PrimaryKey,
    column_name: &ColumnName,
    value: &Value,
) -> Result<(), HeadError> {
    let mut params = Vec::new();
    let filter = sql::equals(Column::Attnum(key.attnum), &mut params, value.clone())?;
    let select = sql::select(
        Relation::Table(table_oid),
        &[Column::Attnum(key.attnum)],
        Some(filter),
        &[],
    );
    if connection.query(select, params)?.is_empty() {
        return Ok(());
    }
    Err(HeadError::raise(PgError::DuplicateKeyValue {
        constraint: key.name.clone(),
        key: column_name.render().as_sql().to_string(),
        value: value.to_string(),
    }))
}

fn map_engine_error(error: LimboError) -> HeadError {
    match error {
        LimboError::Constraint(_) => HeadError::internal(
            "the engine refused a write the head's own check should have caught",
        ),
        error @ LimboError::Corrupt(_)
        | error @ LimboError::NotADB
        | error @ LimboError::InternalError(_)
        | error @ LimboError::IoBackendUnavailable(_)
        | error @ LimboError::SqlError(_)
        | error @ LimboError::CacheError(_)
        | error @ LimboError::DatabaseFull
        | error @ LimboError::SequenceExhausted { .. }
        | error @ LimboError::ParseError(_)
        | error @ LimboError::LexerError(_)
        | error @ LimboError::ConversionError(_)
        | error @ LimboError::EnvVarError(_)
        | error @ LimboError::TxError(_)
        | error @ LimboError::CompletionError(_)
        | error @ LimboError::LockingError(_)
        | error @ LimboError::ParseIntError(_)
        | error @ LimboError::ParseFloatError(_)
        | error @ LimboError::InvalidDate(_)
        | error @ LimboError::InvalidTime(_)
        | error @ LimboError::InvalidModifier(_)
        | error @ LimboError::InvalidArgument(_)
        | error @ LimboError::InvalidFormatter(_)
        | error @ LimboError::ForeignKeyConstraint(_)
        | error @ LimboError::Raise(..)
        | error @ LimboError::RaiseIgnore
        | error @ LimboError::ExtensionError(_)
        | error @ LimboError::IntegerOverflow
        | error @ LimboError::TooBig
        | error @ LimboError::TableLocked
        | error @ LimboError::ReadOnly
        | error @ LimboError::Busy
        | error @ LimboError::StatementsInProgress(_)
        | error @ LimboError::Interrupt
        | error @ LimboError::BusySnapshot
        | error @ LimboError::Conflict(_)
        | error @ LimboError::SchemaUpdated
        | error @ LimboError::SchemaConflict
        | error @ LimboError::Page1NotAlloc
        | error @ LimboError::TxTerminated
        | error @ LimboError::WriteWriteConflict
        | error @ LimboError::CommitDependencyAborted
        | error @ LimboError::NoSuchTransactionID(_)
        | error @ LimboError::NullValue
        | error @ LimboError::InvalidColumnType
        | error @ LimboError::InvalidBlobSize(_)
        | error @ LimboError::BlobHandleExpired
        | error @ LimboError::PlanningError(_)
        | error @ LimboError::CheckpointFailed(_)
        | error @ LimboError::UnsupportedEncoding(_)
        | error @ LimboError::OutOfMemory => error.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_constraint_backstop_ignores_the_message_text() {
        let unique_wording = map_engine_error(LimboError::Constraint(
            "UNIQUE constraint failed: rel_1.col_1".to_string(),
        ));
        let other_wording = map_engine_error(LimboError::Constraint("anything at all".to_string()));
        assert_eq!(unique_wording.state, SqlState::Internal);
        assert_eq!(unique_wording.message, other_wording.message);
    }
}
