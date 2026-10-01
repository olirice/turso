#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod common;

use common::{assert_error, head, run, values};
use turso_core::Value;
use turso_pg_head::SqlState;

// Everything else in this file moved to postgres/conformance/head/pg_catalog.sql
// (with two refused-by-name cases in refusals.sql). What stays here is
// behavior that pins the head's own, deliberately reduced catalog model,
// which a live PostgreSQL 18 oracle cannot check, since its own default
// database has a far richer catalog: an information_schema and pg_toast
// namespace, a plpgsql extension, hundreds of pg_opclass rows, and so on.
//
// - pg_namespace_has_exactly_pg_catalog_and_public and
//   a_catalog_relation_the_head_has_no_rows_for_is_empty_not_refused pin
//   the reduced catalog model.
// - insert_into_pg_class_is_refused: PostgreSQL 18 actually admits the
//   write (a superuser bypasses the catalog-protection check the GRANT
//   and CREATE TABLE refusals hit), then fails for an unrelated reason (a
//   NOT NULL violation on a system column this INSERT never supplied);
//   the head refuses by name instead. Matching this would mean modeling
//   every pg_class column's insertability, out of scope for this batch.

#[test]
fn pg_namespace_has_exactly_pg_catalog_and_public() {
    let head = head();
    let mut names: Vec<String> = values(run(&head, "postgres", "SELECT nspname FROM pg_namespace"))
        .into_iter()
        .map(|row| match row.as_slice() {
            [Value::Text(name)] => name.as_str().to_string(),
            other => panic!("unexpected row {other:?}"),
        })
        .collect();
    names.sort();
    assert_eq!(names, vec!["pg_catalog", "public"]);
}

#[test]
fn insert_into_pg_class_is_refused() {
    let head = head();
    assert_error(
        head.connect("postgres")
            .expect("postgres can log in")
            .execute("INSERT INTO pg_class (relname) VALUES ('x')"),
        SqlState::FeatureNotSupported,
        "INSERT on pg_catalog relation \"pg_class\" is not supported",
    );
}

#[test]
fn a_catalog_relation_the_head_has_no_rows_for_is_empty_not_refused() {
    let head = head();
    assert_eq!(
        values(run(
            &head,
            "postgres",
            "SELECT extname FROM pg_catalog.pg_extension"
        )),
        Vec::<Vec<Value>>::new()
    );
    assert_eq!(
        values(run(
            &head,
            "postgres",
            "SELECT opcname FROM pg_catalog.pg_opclass"
        )),
        Vec::<Vec<Value>>::new()
    );
    assert_eq!(
        values(run(
            &head,
            "postgres",
            "SELECT inhrelid, inhparent FROM pg_catalog.pg_inherits"
        )),
        Vec::<Vec<Value>>::new()
    );
}
