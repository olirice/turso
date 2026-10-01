//! What moved to `postgres/conformance/head/prepare_execute.sql` is deleted
//! from here. One case cannot cross that bridge yet:
//!
//! - Two sessions interleaved (a GRANT committed by one session while
//!   another session's earlier `PREPARE` is still live) needs genuine
//!   concurrency: psql's (and so the runner's) `\c` always opens a brand
//!   new connection, which drops the first session's prepared statements
//!   entirely rather than merely switching role on the same one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod common;

use common::{assert_error, run};
use turso_pg_head::SqlState;

fn with_notes() -> turso_pg_head::Head {
    let head = common::head();
    run(
        &head,
        "postgres",
        "CREATE TABLE notes (id integer PRIMARY KEY, body text)",
    );
    run(
        &head,
        "postgres",
        "INSERT INTO notes VALUES (1, 'first'), (2, 'second')",
    );
    head
}

#[test]
fn a_statement_prepared_before_a_grant_sees_the_grant_at_execute() {
    let head = with_notes();
    run(&head, "postgres", "CREATE ROLE bob LOGIN");
    let bob = head.connect("bob").expect("bob can log in");
    bob.execute("PREPARE getnotes AS SELECT body FROM notes")
        .expect("PREPARE does not check privileges");
    assert_error(
        bob.execute("EXECUTE getnotes"),
        SqlState::InsufficientPrivilege,
        "permission denied for table notes",
    );
    run(&head, "postgres", "GRANT SELECT ON notes TO bob");
    assert!(matches!(
        bob.execute("EXECUTE getnotes")
            .expect("bob can now execute it"),
        turso_pg_head::Outcome::Rows { .. }
    ));
}
