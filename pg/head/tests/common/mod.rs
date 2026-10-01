#![allow(dead_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use turso_core::{MemoryIO, Value};
use turso_pg_head::{Head, HeadError, Outcome, SqlState};

pub fn head() -> Head {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = format!("head-test-{}.db", NEXT.fetch_add(1, Ordering::Relaxed));
    Head::open(Arc::new(MemoryIO::new()), &path).expect("opening an in-memory head succeeds")
}

pub fn assert_error(
    result: Result<impl std::fmt::Debug, HeadError>,
    state: SqlState,
    message: &str,
) {
    let error = result.expect_err("the statement must be refused");
    assert_eq!((error.state, error.message.as_str()), (state, message));
}

pub fn run(head: &Head, role: &str, sql: &str) -> Outcome {
    head.connect(role)
        .expect("the role can log in")
        .execute(sql)
        .expect(sql)
}

pub fn values(outcome: Outcome) -> Vec<Vec<Value>> {
    match outcome {
        Outcome::Rows { values, .. } => values,
        other @ (Outcome::Command(_) | Outcome::Inserted(_)) => {
            panic!("expected rows, got {other:?}")
        }
    }
}
