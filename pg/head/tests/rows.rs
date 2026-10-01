#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod common;

use turso_core::Value;
use turso_pg_head::{Outcome, Session};

fn rows(session: &Session, sql: &str) -> Vec<Vec<Value>> {
    match session.execute(sql).expect(sql) {
        Outcome::Rows { values, .. } => values,
        Outcome::Command(_) | Outcome::Inserted(_) => panic!("{sql} returned no rows"),
    }
}

fn row(values: &[Option<&str>]) -> Vec<Value> {
    values
        .iter()
        .map(|value| match value {
            None => Value::Null,
            Some(text) => match text.parse::<i64>() {
                Ok(integer) => Value::from_i64(integer),
                Err(_) => Value::from_text(text.to_string()),
            },
        })
        .collect()
}

// Everything else in this file moved to postgres/conformance/head/rows.sql
// (with the two head-refuses-what-PostgreSQL-admits cases in
// refusals.sql). What stays here needs a second, concurrent session:
// psql only ever holds one connection at a time, so this has no corpus
// form.

#[test]
fn a_session_sees_a_table_another_session_created() {
    let head = common::head();
    let reader = head.connect("postgres").expect("postgres exists");
    head.connect("postgres")
        .expect("postgres exists")
        .execute("CREATE TABLE later (id integer)")
        .expect("the table is created");
    reader
        .execute("INSERT INTO later VALUES (1)")
        .expect("the first session sees the new table");
    assert_eq!(
        rows(&reader, "SELECT id FROM later"),
        vec![row(&[Some("1")])]
    );
}
