# PostgreSQL 18 system catalog capture

Data captured from a throwaway, freshly-`initdb`'d PostgreSQL 18 cluster of
the build the flake pins as `.#postgres` (input `nixpkgs-postgres`). No extensions were loaded, no
user objects were created; every query below ran against the database exactly
as `initdb` left it.

(see `out/version.txt` for the exact captured version string; `build.rs`
fails the build if its major version is not `POSTGRES_MAJOR`)

All output files are JSON, each one a single JSON array of row objects
(`json_agg(row_to_json(t))` over the query), so the format is consistent
across every file in `out/`. Every field is emitted as PostgreSQL's own text
representation of the value (the same thing `to_json`/`row_to_json` produce
for a type with no dedicated JSON cast: its output-function text, not a
reinterpreted numeric/boolean). `oid`-typed columns therefore appear as
decimal-string text, not JSON numbers.

`pg/head/src/catalog/pg.rs`'s `generated` module is produced from
this capture by `pg/head/build.rs` (generator logic in
`pg/head/build/generate.rs`) every time the crate builds, and written
to `OUT_DIR`; nothing generated is checked in.

### `out/errcodes.txt` -- PostgreSQL 18's own SQLSTATE list

Not a capture (nothing to query for it): a verbatim copy of PostgreSQL
18.6's `share/postgresql/errcodes.txt`, the source file PostgreSQL itself
generates `errcodes.h`, `plerrcodes.h` and its documentation table from,
copied from a matching PostgreSQL 18.6 install
(`/nix/store/9p6vh3k81xpyqmrj6qp8yh1q1nw30hib-postgresql-18.6`, the
flake's `.#postgres`) on 2026-09-28.
Diffed byte-for-byte identical against PostgreSQL 18.4's copy of the same
file, so a patch release does not change it. `build/sqlstate.rs` parses it
into `SqlState`, one variant per condition it lists, with no code
hand-listed in the crate.

## Regenerating

Run `./capture.sh` from this directory. It creates a throwaway cluster under
`/tmp`, runs every file in `sql/` against it in order, writes the results
into `out/`, and tears the cluster down. It uses `$PG_BIN` if set, else
the flake's pinned `.#postgres`. A diff against the checked-in `out/`
tree is the review; the next `cargo build -p turso_pg_head` regenerates the
Rust source automatically, since `build.rs` reruns whenever a file under
`out/` changes.

## Files

### `out/relations.json` -- the tables and indexes in `pg_catalog`, and the head's views

191 rows: 64 ordinary tables (`relkind = 'r'`), 124 indexes (`'i'`) and the
three views the head defines (`pg_roles`, `pg_settings`, `pg_seclabels`).
SQL: `sql/01_relations.sql`.

### `out/columns.json` -- every column of every table above

985 rows. `attnum > 0` are ordinary columns; `attnum < 0` are the system
columns (`tableoid`, `cmax`, `xmax`, `cmin`, `xmin`, `ctid`), flagged with
`is_system_column = true`. `reloid` joins back to `relations.json`'s `oid`.
SQL: `sql/02_columns.sql`.

### `out/indexes.json` -- every index on those catalogs

124 rows, matching the 124 `relkind = 'i'` rows in `relations.json`. SQL:
`sql/03_indexes.sql`.

### `out/types.json` -- full `pg_type` rows for every type in use

50 rows: every `atttypid` used by a column in (2), the element type of any
array among those, a fixed list of well-known types (`bool`, `oid`,
`aclitem`, `int2vector`, ...), and the polymorphic pseudo-types used in
function signatures (`any`, `anyarray`, `anynonarray`, `anycompatible`,
`anycompatiblearray`). SQL: `sql/04_types.sql`.

### `out/casts.json` -- every implicit `pg_cast`

117 rows: `castsource`/`casttarget` (plus each side's `typname`, for a
readable diff) for every `pg_cast` row with `castcontext = 'i'`, the
casts PostgreSQL performs without an explicit `CAST`/`::`. SQL:
`sql/05_casts.sql`. `pg::implicit_cast_exists` is the one place this head
reads it, replacing a hand-ranked type width
(`analyze/typing/check.rs`'s `case_anchor_merge`/`widen_numeric`).

### `out/bootstrap_rows/*.json` -- full rows from the freshly-initdb'd database

One file per table (`pg_authid`, `pg_namespace`, `pg_am`,
`pg_database`, `pg_tablespace`, `pg_init_privs`, `pg_description`); see
`sql/06*.sql` for the exact query behind each file.

`pg_authid.json` has 17 rows: `postgres` (oid 10), `pg_database_owner` (oid
6171, a virtual/pseudo role with no members by default), and 15 further
predefined roles under oid 16384 (the first oid `initdb` hands to a
user-created object; PostgreSQL 18 added `pg_signal_autovacuum_worker` to
this list). `pg_namespace.json` has 4 rows: `pg_catalog` (11),
`pg_toast` (99), `public` (2200), `information_schema` (13699 on this build
-- not a portable/fixed oid).

`pg_class.json` (177 rows), `pg_attribute.json` (1204 rows) and
`pg_index.json` (110 rows) are full rows -- every column -- for the 64
catalog tables and their unique indexes: `sql/07a_bootstrap_pg_class.sql`,
`07b_bootstrap_pg_attribute.sql`, `07c_bootstrap_pg_index.sql`. These feed
the constant catalog layer's own `PG_CLASS_ROWS`, `PG_ATTRIBUTE_ROWS` and
`PG_INDEX_ROWS`, served read-only through `engine_hooks::EngineHooks` as
`const_<oid>` internal virtual tables; `engine::constant` applies a small,
listed set of deviations where the head does not implement what PostgreSQL
captured (no TOAST, no composite row types, no separate storage layer, no
autovacuum/analyze statistics).
