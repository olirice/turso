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

#[derive(Debug, Default)]
pub struct UnnestVirtualTable;

impl UnnestVirtualTable {
    pub fn new() -> Self {
        Self
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
                omit: false,
            };
            constraints.len()
        ];

        let mut arr_idx: Option<usize> = None;
        let mut has_arr_eq_constraint = false;
        for (i, c) in constraints.iter().enumerate() {
            if c.op != ConstraintOp::Eq || c.column_index as usize != COL_ARR {
                continue;
            }
            has_arr_eq_constraint = true;
            if c.usable {
                arr_idx = Some(i);
            }
        }
        if has_arr_eq_constraint && arr_idx.is_none() {
            return Err(ResultCode::ConstraintViolation);
        }

        let (cost, rows) = if let Some(idx) = arr_idx {
            usages[idx] = ConstraintUsage {
                argv_index: Some(1),
                omit: true,
            };
            (1., 25)
        } else {
            (f64::MAX, 25)
        };

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
        "CREATE TABLE unnest (
            value ANY,
            arr ANY HIDDEN
        );"
        .to_owned()
    }
}
#[derive(Debug)]
struct UnnestCursor {
    elements: Vec<Value>,
    arr: Value,
    index: usize,
}

impl UnnestCursor {
    fn empty() -> Self {
        Self {
            elements: Vec::new(),
            arr: Value::Null,
            index: 0,
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
        let [arr] = args else {
            return Err(LimboError::InternalError(format!(
                "unnest expects one argument, got {}",
                args.len()
            )));
        };
        self.arr = arr.clone();
        self.elements = array_values_from_any(&self.arr).unwrap_or_default();
        self.index = 0;
        Ok(!self.elements.is_empty())
    }

    fn next(&mut self) -> Result<bool, LimboError> {
        self.index += 1;
        Ok(self.index < self.elements.len())
    }

    fn rowid(&self) -> i64 {
        self.index as i64 + 1
    }

    fn column(&self, idx: usize) -> Result<Value, LimboError> {
        match idx {
            COL_VALUE => self.elements.get(self.index).cloned().ok_or_else(|| {
                LimboError::InternalError(
                    "column is only read while the cursor is on an element".to_string(),
                )
            }),
            COL_ARR => Ok(self.arr.clone()),
            _ => Err(LimboError::InternalError(format!(
                "unnest has no column {idx}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(arr: Value) -> Vec<Value> {
        let mut cursor = UnnestCursor::empty();
        let mut has_row = cursor.filter(&[arr], None, -1).unwrap();
        let mut values = Vec::new();
        while has_row {
            values.push(cursor.column(COL_VALUE).unwrap());
            has_row = cursor.next().unwrap();
        }
        values
    }

    #[test]
    fn test_unnest_integers() {
        let arr = crate::vdbe::array::values_to_record_blob(&[
            Value::from_i64(1),
            Value::from_i64(2),
            Value::from_i64(3),
        ])
        .unwrap();
        assert_eq!(
            collect(arr),
            vec![Value::from_i64(1), Value::from_i64(2), Value::from_i64(3)]
        );
    }

    #[test]
    fn test_unnest_null_elements() {
        let arr = crate::vdbe::array::values_to_record_blob(&[
            Value::from_i64(1),
            Value::Null,
            Value::from_i64(3),
        ])
        .unwrap();
        assert_eq!(
            collect(arr),
            vec![Value::from_i64(1), Value::Null, Value::from_i64(3)]
        );
    }

    #[test]
    fn test_unnest_empty_array() {
        let arr = crate::vdbe::array::values_to_record_blob(&[]).unwrap();
        assert_eq!(collect(arr), Vec::<Value>::new());
    }

    #[test]
    fn test_unnest_null_argument() {
        assert_eq!(collect(Value::Null), Vec::<Value>::new());
    }

    #[test]
    fn test_unnest_non_array_argument() {
        assert_eq!(collect(Value::from_i64(5)), Vec::<Value>::new());
    }
}
