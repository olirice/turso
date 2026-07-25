//! `unnest(array)` table-valued function: expands an array into one row per
//! element. Mirrors the `json_each`/`json_tree` internal vtab pattern
//! (`crate::json::vtab`), reusing the array parsing helpers in
//! `crate::vdbe::array` that already back `ARRAY[]`, `array_agg`, and the
//! other array scalar functions.

use crate::sync::{Arc, RwLock};
use std::result::Result;

use turso_ext::{ConstraintOp, ConstraintUsage, ResultCode};

use crate::{
    vdbe::array::array_values_from_any,
    vtab::{InternalVirtualTable, InternalVirtualTableCursor},
    Connection, LimboError, Value,
};

const COL_VALUE: usize = 0;
const COL_ARR: usize = 1;

#[derive(Debug)]
pub struct UnnestVirtualTable;

impl UnnestVirtualTable {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UnnestVirtualTable {
    fn default() -> Self {
        Self::new()
    }
}

impl InternalVirtualTable for UnnestVirtualTable {
    fn name(&self) -> String {
        "unnest".to_owned()
    }

    fn open(
        &self,
        _conn: Arc<Connection>,
    ) -> crate::Result<Arc<RwLock<dyn InternalVirtualTableCursor + 'static>>> {
        Ok(Arc::new(RwLock::new(UnnestCursor::empty())))
    }

    fn best_index(
        &self,
        constraints: &[turso_ext::ConstraintInfo],
        _order_by: &[turso_ext::OrderByInfo],
    ) -> Result<turso_ext::IndexInfo, ResultCode> {
        let mut usages = vec![
            ConstraintUsage {
                argv_index: None,
                omit: false
            };
            constraints.len()
        ];

        let mut arr_idx: Option<usize> = None;
        let mut has_arr_eq_constraint = false;
        for (i, c) in constraints.iter().enumerate() {
            if c.op != ConstraintOp::Eq {
                continue;
            }
            if c.column_index as usize == COL_ARR {
                has_arr_eq_constraint = true;
                if c.usable {
                    arr_idx = Some(i);
                }
            }
        }

        // The hidden argument column must be usable in the chosen loop. If
        // it's present but unusable, reject this access shape so the
        // optimizer can pick a join order where the argument register is
        // available.
        if has_arr_eq_constraint && arr_idx.is_none() {
            return Err(ResultCode::ConstraintViolation);
        }

        let argc = if arr_idx.is_some() { 1 } else { 0 };
        if let Some(idx) = arr_idx {
            usages[idx] = ConstraintUsage {
                argv_index: Some(1),
                omit: true,
            };
        }

        let (cost, rows) = if argc == 1 { (1., 25) } else { (f64::MAX, 25) };

        Ok(turso_ext::IndexInfo {
            idx_num: -1,
            idx_str: None,
            order_by_consumed: false,
            estimated_cost: cost,
            estimated_rows: rows,
            constraint_usages: usages,
        })
    }

    fn sql(&self) -> String {
        "CREATE TABLE x(
            value ANY,      -- the unnested element
            arr ANY HIDDEN  -- input parameter: the array to unnest
        );"
        .to_owned()
    }
}

struct UnnestCursor {
    rowid: i64,
    values: Vec<Value>,
}

impl UnnestCursor {
    fn empty() -> Self {
        Self {
            rowid: -1,
            values: Vec::new(),
        }
    }
}

impl InternalVirtualTableCursor for UnnestCursor {
    fn filter(
        &mut self,
        args: &[Value],
        _idx_str: Option<String>,
        _idx_num: i32,
    ) -> Result<bool, LimboError> {
        self.rowid = -1;
        self.values = match args.first() {
            Some(arr) => array_values_from_any(arr).unwrap_or_default(),
            None => Vec::new(),
        };
        self.next()
    }

    fn next(&mut self) -> Result<bool, LimboError> {
        self.rowid += 1;
        Ok((self.rowid as usize) < self.values.len())
    }

    fn rowid(&self) -> i64 {
        self.rowid
    }

    fn column(&self, idx: usize) -> Result<Value, LimboError> {
        Ok(match idx {
            COL_VALUE => self.values[self.rowid as usize].clone(),
            _ => Value::Null,
        })
    }
}
