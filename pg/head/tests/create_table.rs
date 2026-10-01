//! What moved to `postgres/conformance/head/create_table.sql` and
//! `refusals.sql` (PostgreSQL-authored and head-authored transcripts,
//! respectively) is deleted from here. What remains cannot cross that
//! bridge yet, each for a specific, reported reason:
//!
//! - A bare syntax error still has no attached position: pg_query's public
//!   API discards libpg_query's `cursorpos` for a parse error
//!   (`pg_query::Error::Parse` is a bare `String`), so the head cannot
//!   report the position PostgreSQL 18 itself does for one. Every other
//!   position PostgreSQL attaches (undefined column/table/type/function,
//!   ambiguous column, multiple primary keys) comes from a pg_query node's
//!   own `location` field instead, which the head does carry now (see
//!   `pg/head/src/error.rs` and `postgres/conformance/head/README.md`).
//! - An unknown role failing to log in over `\c` has no PostgreSQL-recorded
//!   counterpart either, for a different reason than before: the runner
//!   now supports `\c`'s role-switching forms fully (see
//!   `postgres/regress/main.rs`), but `record`/`run`
//!   (`postgres/conformance/head.py`) each start a server on an
//!   independently chosen free port, so a `\connect ... failed` line's
//!   embedded port number can never byte-match between a recording and a
//!   later run. See `postgres/conformance/head/README.md`.
//! - psql (and so the runner) never sends more than one statement per
//!   wire message, so the head's "one statement per call" refusal is not
//!   reachable through it either.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod common;

use common::{assert_error, head};
use turso_pg_head::SqlState;

#[test]
fn an_unknown_role_cannot_connect() {
    assert_error(
        head().connect("nobody").map(|_| ()),
        SqlState::InvalidAuthorizationSpecification,
        "role \"nobody\" does not exist",
    );
}

#[test]
fn only_one_statement_per_call_is_supported() {
    assert_error(
        head()
            .connect("postgres")
            .expect("postgres exists")
            .execute("CREATE TABLE a (id integer); CREATE TABLE b (id integer)"),
        SqlState::FeatureNotSupported,
        "anything other than exactly one statement per call is not supported",
    );
}

#[test]
fn a_syntax_error_is_reported_as_one() {
    let error = head()
        .connect("postgres")
        .expect("postgres exists")
        .execute("CREATE TABLE (")
        .expect_err("the statement does not parse");
    assert_eq!(error.state, SqlState::SyntaxError);
}

#[test]
fn a_syntax_error_takes_priority_over_a_long_identifier() {
    let long = "x".repeat(64);
    let error = head()
        .connect("postgres")
        .expect("postgres exists")
        .execute(&format!("CREATE TABLE {long} ("))
        .expect_err("the statement does not parse");
    assert_eq!(error.state, SqlState::SyntaxError);
}
