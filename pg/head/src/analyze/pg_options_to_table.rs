use std::sync::Arc;

use parking_lot::RwLock;
use turso_core::schema::Schema;
use turso_core::{
    Connection, InternalVirtualTable, InternalVirtualTableCursor, LimboError, Value, VirtualTable,
};
use turso_ext::{
    ConstraintInfo, ConstraintOp, ConstraintUsage, IndexInfo, OrderByInfo, ResultCode, VTabKind,
};

const COL_NAME: usize = 0;
const COL_VALUE: usize = 1;
const COL_OPTIONS: u32 = 2;

pub(crate) fn register(schema: &mut Schema) -> turso_core::Result<()> {
    let vtab = VirtualTable::new_internal(
        "pg_options_to_table".to_string(),
        PgOptionsToTable.sql(),
        VTabKind::VirtualTable,
        Arc::new(RwLock::new(PgOptionsToTable)),
    )?;
    schema.add_virtual_table(Arc::new(vtab))?;
    Ok(())
}

#[derive(Debug)]
struct PgOptionsToTable;

impl InternalVirtualTable for PgOptionsToTable {
    fn name(&self) -> String {
        "pg_options_to_table".to_string()
    }

    fn sql(&self) -> String {
        "CREATE TABLE pg_options_to_table (
            col_1 TEXT,
            col_2 TEXT,
            options ANY HIDDEN
        );"
        .to_string()
    }

    fn open(
        &self,
        _conn: Arc<Connection>,
    ) -> turso_core::Result<Arc<RwLock<dyn InternalVirtualTableCursor>>> {
        Ok(Arc::new(RwLock::new(PgOptionsToTableCursor {
            rows: Vec::new(),
            current_row: 0,
        })))
    }

    fn best_index(
        &self,
        constraints: &[ConstraintInfo],
        _order_by: &[OrderByInfo],
    ) -> Result<IndexInfo, ResultCode> {
        let mut usages = vec![
            ConstraintUsage {
                argv_index: None,
                omit: false,
            };
            constraints.len()
        ];
        let mut options_index = None;
        for (index, constraint) in constraints.iter().enumerate() {
            if constraint.op == ConstraintOp::Eq
                && constraint.column_index == COL_OPTIONS
                && constraint.usable
            {
                options_index = Some(index);
            }
        }
        let cost = match options_index.and_then(|index| usages.get_mut(index)) {
            Some(usage) => {
                *usage = ConstraintUsage {
                    argv_index: Some(1),
                    omit: true,
                };
                1.0
            }
            None => f64::MAX,
        };
        Ok(IndexInfo {
            idx_num: 0,
            idx_str: None,
            order_by_consumed: false,
            estimated_cost: cost,
            estimated_rows: 10,
            constraint_usages: usages,
        })
    }
}

struct PgOptionsToTableCursor {
    rows: Vec<(turso_core::Value, turso_core::Value)>,
    current_row: usize,
}

impl InternalVirtualTableCursor for PgOptionsToTableCursor {
    fn filter(
        &mut self,
        args: &[Value],
        _idx_str: Option<String>,
        _idx_num: i32,
    ) -> Result<bool, LimboError> {
        let [options] = args else {
            return Err(LimboError::InternalError(format!(
                "pg_options_to_table expects one argument, got {}",
                args.len()
            )));
        };
        let elements = match options {
            Value::Null => Vec::new(),
            array @ Value::Numeric(_) | array @ Value::Text(_) | array @ Value::Blob(_) => {
                turso_core::decode_array(array)?
            }
        };
        self.rows = elements
            .into_iter()
            .map(|element| match element {
                Value::Text(text) => match text.as_str().split_once('=') {
                    Some((name, value)) => Ok((
                        Value::from_text(name.to_string()),
                        Value::from_text(value.to_string()),
                    )),
                    None => Err(LimboError::InternalError(format!(
                        "malformed option \"{}\"",
                        text.as_str()
                    ))),
                },
                other @ Value::Null | other @ Value::Numeric(_) | other @ Value::Blob(_) => {
                    Err(LimboError::InternalError(format!(
                        "a pg_options_to_table element was {other:?}, not text"
                    )))
                }
            })
            .collect::<Result<Vec<_>, LimboError>>()?;
        self.current_row = 0;
        Ok(!self.rows.is_empty())
    }

    fn next(&mut self) -> Result<bool, LimboError> {
        self.current_row += 1;
        Ok(self.current_row < self.rows.len())
    }

    fn rowid(&self) -> i64 {
        i64::try_from(self.current_row).unwrap_or(i64::MAX)
    }

    fn column(&self, column: usize) -> Result<Value, LimboError> {
        let Some((name, value)) = self.rows.get(self.current_row) else {
            return Ok(Value::Null);
        };
        match column {
            COL_NAME => Ok(name.clone()),
            COL_VALUE => Ok(value.clone()),
            _ => Ok(Value::Null),
        }
    }
}
