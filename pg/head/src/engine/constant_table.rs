use std::sync::Arc;

use parking_lot::RwLock;
use turso_core::schema::Schema;
use turso_core::{
    Connection, InternalVirtualTable, InternalVirtualTableCursor, LimboError, Value, VirtualTable,
};
use turso_ext::{ConstraintInfo, ConstraintUsage, IndexInfo, OrderByInfo, ResultCode, VTabKind};

use crate::engine::constant::{self, ConstantRelation};
use crate::engine::store;
use crate::lower::sql::{self, Relation};

pub(crate) fn register_all(schema: &mut Schema) -> turso_core::Result<()> {
    for relation in ConstantRelation::ALL {
        let table = relation.table();
        let oid = table.relation_oid();
        let columns = store::declared_columns(table)
            .map_err(|error| LimboError::InternalError(error.to_string()))?;
        let name = sql::engine_name(Relation::Constant(oid));
        let sql_text =
            sql::create_table(Relation::Constant(oid), false, columns, Vec::new()).to_string();
        let vtab = VirtualTable::new_internal(
            name.clone(),
            sql_text.clone(),
            VTabKind::VirtualTable,
            Arc::new(RwLock::new(ConstantTable {
                relation,
                name,
                sql_text,
            })),
        )?;
        schema.add_virtual_table(Arc::new(vtab))?;
    }
    Ok(())
}

#[derive(Debug)]
struct ConstantTable {
    relation: ConstantRelation,
    name: String,
    sql_text: String,
}

impl InternalVirtualTable for ConstantTable {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn sql(&self) -> String {
        self.sql_text.clone()
    }

    fn open(
        &self,
        _conn: Arc<Connection>,
    ) -> turso_core::Result<Arc<RwLock<dyn InternalVirtualTableCursor>>> {
        let rows = constant::relation_rows(self.relation)
            .map_err(|error| LimboError::InternalError(error.to_string()))?;
        Ok(Arc::new(RwLock::new(ConstantCursor {
            rows,
            current_row: 0,
        })))
    }

    fn best_index(
        &self,
        constraints: &[ConstraintInfo],
        _order_by: &[OrderByInfo],
    ) -> Result<IndexInfo, ResultCode> {
        Ok(IndexInfo {
            idx_num: 0,
            idx_str: None,
            order_by_consumed: false,
            estimated_cost: 100.0,
            estimated_rows: 100,
            constraint_usages: constraints
                .iter()
                .map(|_| ConstraintUsage {
                    argv_index: None,
                    omit: false,
                })
                .collect(),
        })
    }
}

struct ConstantCursor {
    rows: Arc<Vec<Vec<Value>>>,
    current_row: usize,
}

impl InternalVirtualTableCursor for ConstantCursor {
    fn next(&mut self) -> Result<bool, LimboError> {
        self.current_row += 1;
        Ok(self.current_row < self.rows.len())
    }

    fn rowid(&self) -> i64 {
        i64::try_from(self.current_row).unwrap_or(i64::MAX)
    }

    fn column(&self, column: usize) -> Result<Value, LimboError> {
        Ok(self
            .rows
            .get(self.current_row)
            .and_then(|row| row.get(column))
            .cloned()
            .unwrap_or(Value::Null))
    }

    fn filter(
        &mut self,
        _args: &[Value],
        _idx_str: Option<String>,
        _idx_num: i32,
    ) -> Result<bool, LimboError> {
        self.current_row = 0;
        Ok(!self.rows.is_empty())
    }
}
