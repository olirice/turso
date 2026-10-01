#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod common;

use common::head;
use turso_pg_head::{Outcome, Session, SqlState};

fn setup() -> turso_pg_head::Head {
    let head = head();
    let postgres = head.connect("postgres").expect("postgres can log in");
    for sql in [
        "CREATE ROLE bob LOGIN",
        "CREATE TABLE notes (id integer PRIMARY KEY, owner text)",
        "INSERT INTO notes VALUES (1, 'bob'), (2, 'carol')",
        "GRANT SELECT ON notes TO bob",
        "CREATE TABLE hidden (id integer)",
    ] {
        postgres.execute(sql).expect(sql);
    }
    head
}

fn count(session: &Session, sql: &str) -> usize {
    match session.execute(sql).expect(sql) {
        Outcome::Rows { values, .. } => values.len(),
        other @ (Outcome::Command(_) | Outcome::Inserted(_)) => {
            panic!("expected rows, got {other:?}")
        }
    }
}

// Everything else in this file moved to postgres/conformance/head/session.sql
// (with several refused-by-name cases, including the "request.jwt.claims"
// placeholder GUC PostgreSQL 18 silently accepts and the head refuses, in
// refusals.sql). What stays here:
//
// - in_a_failed_transaction_only_a_syntax_error_comes_before_the_aborted_error
//   keeps only the syntax-error sub-case: a bare syntax error has no
//   Rust-side counterpart in the corpus (see head/README.md's known gaps).
// - a_sessions_own_uncommitted_ddl_is_visible_to_itself_and_not_to_another_session
//   and after_rollback_the_shared_cache_does_not_serve_the_rolled_back_catalog
//   need two sessions genuinely open at once (one idle mid-transaction
//   while the other acts); psql only ever holds one connection at a time.

#[test]
fn in_a_failed_transaction_only_a_syntax_error_comes_before_the_aborted_error() {
    let session = setup().connect("postgres").expect("postgres can log in");
    session.execute("BEGIN").expect("begin");
    session
        .execute("SELECT id FROM nosuch")
        .expect_err("the transaction is now aborted");
    assert_eq!(
        session
            .execute("SELECT (")
            .expect_err("a syntax error is still reported")
            .state,
        SqlState::SyntaxError
    );
    session.execute("ROLLBACK").expect("rollback");
}

#[test]
fn a_sessions_own_uncommitted_ddl_is_visible_to_itself_and_not_to_another_session() {
    let head = setup();
    let a = head.connect("postgres").expect("postgres can log in");
    let b = head.connect("postgres").expect("postgres can log in");
    for sql in ["BEGIN", "CREATE TABLE in_flight (id integer)"] {
        a.execute(sql).expect(sql);
    }
    assert_eq!(
        count(
            &a,
            "SELECT relname FROM pg_class WHERE relname = 'in_flight'"
        ),
        1
    );
    b.execute("BEGIN").expect("BEGIN");
    assert_eq!(
        count(
            &b,
            "SELECT relname FROM pg_class WHERE relname = 'in_flight'"
        ),
        0
    );
    b.execute("ROLLBACK").expect("ROLLBACK");
    a.execute("ROLLBACK").expect("ROLLBACK");
}

#[test]
fn after_rollback_the_shared_cache_does_not_serve_the_rolled_back_catalog() {
    let head = setup();
    let a = head.connect("postgres").expect("postgres can log in");
    let b = head.connect("postgres").expect("postgres can log in");
    for sql in ["BEGIN", "CREATE TABLE poisoned (id integer)", "ROLLBACK"] {
        a.execute(sql).expect(sql);
    }
    b.execute("CREATE TABLE genuine (id integer)")
        .expect("creating genuine succeeds");
    assert_eq!(
        count(
            &a,
            "SELECT relname FROM pg_class WHERE relname = 'poisoned'"
        ),
        0
    );
    assert_eq!(
        count(&a, "SELECT relname FROM pg_class WHERE relname = 'genuine'"),
        1
    );
}
