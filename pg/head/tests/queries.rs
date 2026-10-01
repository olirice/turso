#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod common;

use common::{assert_error, head, run};
use turso_pg_head::SqlState;

// Everything else in this file moved to
// postgres/conformance/head/queries.sql (with the refused-by-design cases,
// including the multi-array unnest overload PostgreSQL 18 admits and the
// head refuses by name, in refusals.sql). What stays here is behavior
// PostgreSQL 18 itself shows differently than the head, in ways psql
// cannot pin (see this batch's report for each gap):
//
// - array_agg_with_order_by_is_refused_in_a_query_with_its_own_order_by:
//   the exact statement is also invalid on PostgreSQL 18, but for an
//   unrelated reason (grouping error, since the outer ORDER BY references
//   an ungrouped column); the head's own "aggregate ORDER BY in a query
//   that itself has ORDER BY" restriction fires first instead.
// - order_by_on_a_union_refuses_an_arbitrary_expression: both refuse
//   (0A000), but PostgreSQL 18's message, detail, hint and position all
//   differ from the head's; the head expression tree has no generic
//   location accessor to attach one.

#[test]
fn array_agg_with_order_by_is_refused_in_a_query_with_its_own_order_by() {
    let head = head();
    run(
        &head,
        "postgres",
        "CREATE TABLE t (id integer PRIMARY KEY, g integer)",
    );
    let session = head.connect("postgres").expect("postgres can log in");
    assert_error(
        session.execute("SELECT array_agg(id ORDER BY id) FROM t ORDER BY id"),
        SqlState::FeatureNotSupported,
        "aggregate ORDER BY in a query that itself has ORDER BY is not supported",
    );
}

#[test]
fn order_by_on_a_union_refuses_an_arbitrary_expression() {
    let head = head();
    let session = head.connect("postgres").expect("postgres exists");
    assert_error(
        session.execute("SELECT 1 AS a, 2 AS b UNION ALL SELECT 3, 4 ORDER BY (a = b)"),
        SqlState::FeatureNotSupported,
        "an ORDER BY expression on a UNION other than a result column name or position is not \
         supported",
    );
}
