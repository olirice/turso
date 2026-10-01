# pg_dump 18 capture

`statements.sql` is every statement pg_dump 18 sent, in order, while
running `pg_dump --schema-only` against a real PostgreSQL 18 loaded with
`schema.sql`. `capture.json` gives, for each `SELECT`-shaped statement,
PostgreSQL's own result column names, types and rows, keyed by the
statement's zero-based index in `statements.sql`.

`schema.sql` is the same schema `pg/wire/tests/pg_dump_round_trip.rs`
applies (its `CAPTURED_SCHEMA`), one statement per line, `SET ROLE`/`RESET
ROLE` marking which role runs the statements between them.

Every statement must succeed on the head and, where `capture.json` has an
entry, match PostgreSQL's own shape and rows; `pg/wire/tests/
pg_dump_statements.rs` has no allowlist for a failing statement, so a
regression here fails the test directly, naming the statement, its index
and the actual error.

## Format

`statements.sql` holds the statements separated by lines of `;;;;`, exactly
as libpq logs them (no trailing semicolon is added or removed). `capture.
json` maps a statement's index (as a zero-padded two-digit string, matching
the order in `statements.sql`) to an object holding an ordered `columns`
list of `[column name, PostgreSQL type name]` pairs and a `rows` list, each
row an array of that statement's columns (in `columns`'s order) rendered as
`::text`, `NULL` as JSON `null`; a statement missing from `capture.json` is
not `SELECT`-shaped (`SET`, `BEGIN`, `LOCK`, `PREPARE`, `EXECUTE`, ...) and
carries no column description or rows. A statement whose `columns` is
recorded but whose text contains a `$1`-style bind parameter has no `rows`
key: pg_dump sends exactly one such statement (a per-table column ACL
query), always with a real bound table oid the capture has no way to
reconstruct after the fact.

`pg/wire/tests/pg_dump_statements.rs` compares each of these rows
against the head's own answer for the same statement over the same schema,
normalizing only what legitimately differs:

- Any `oid`-typed column, once both sides are `>=` PostgreSQL's own
  `FirstNormalObjectId` (16384): the head and the PostgreSQL cluster this was
  captured against each allocate their own project-object oids independently
  starting there, so the exact numbers differ without either being wrong.
  Everything below 16384 is one of PostgreSQL's fixed, portable catalog
  oids and is compared exactly.
- Any `xid`-typed column (`relfrozenxid`, `relminmxid`, `tfrozenxid`,
  `tminmxid`): the head models no MVCC freezing or multixact history, so
  `engine::constant::class_deviations` fixes these to its own constant for
  every relation, never the source cluster's actual vacuum history.
- `reltype`: `engine::constant::class_deviations` fixes this to 0 for
  every relation, since the head implements no composite row type for a
  table's own row type.
- `toid`, `toastpages`, `toast_reloptions`: pg_dump's own query reaches
  these through `LEFT JOIN pg_class tc ON (c.reltoastrelid = tc.oid ...)`;
  `class_deviations` fixes `reltoastrelid` to 0 for every relation (the head
  implements no TOAST storage layer), so this join never matches and these
  are always NULL on the head's side.
- `relpages`: `class_deviations` fixes this to a page count derived only
  from whether the relation is an index, never the source cluster's actual
  storage.

A statement's rows are compared as a set, not position-for-position: every
row the head returns must equal, once normalized, some row PostgreSQL
returned, and no two head rows may claim the same PostgreSQL row. A row
PostgreSQL has that the head does not is not a mismatch (a fixed table's own
built-in inventory belongs to PostgreSQL, not to the closed set of rows the
head's own statements can create, ARCH.md's "pg_dump is an ordinary
client"); a row the head has that matches no PostgreSQL row, or an
unmatched row count in the head's favor, is.

## How it was produced

Using the same nix-provided PostgreSQL 18 the round trip test uses
(`nix shell nixpkgs#postgresql_18`):

1. `initdb` and start a PostgreSQL 18 server with `log_statement = all` and
   `log_min_messages = log`, logging to a file.
2. Connect as `postgres` and run `CAPTURED_SCHEMA` from
   `pg_dump_round_trip.rs`, switching role with `SET ROLE`/`RESET ROLE`
   exactly where that test switches connections.
3. Run `pg_dump --schema-only -U postgres <database>` against the same
   server. Every statement pg_dump sends is now in the server log between
   the schema-setup statements and the log's shutdown lines.
4. Extract those statements from the log into `statements.sql`, joined by
   `\n;;;;\n`.
5. For each statement that is a plain `SELECT` (skip `SET`/`BEGIN`/`LOCK`/
   `PREPARE`/`EXECUTE`), run `psql -X -c '<statement>' -c '\gdesc'` against
   the same server and record the printed column name/type pairs into
   `capture.json` under that statement's index, as `columns`.
6. The `rows` half of `capture.json` is captured against a fresh
   `initdb`'d cluster and database in one continuous `psql` session (`-A
   -t`, unaligned so a row's `\o`-redirected output is exactly its
   columns), running `schema.sql` first so the session's own project-object
   oids exist, then, for each `columns`-described statement without a `$1`,
   `\o`-redirecting to a file and running `SELECT coalesce(json_agg
   (json_build_array(<column list>::text, ...)), '[]'::json) FROM
   (<statement>) t`, naming every column from `columns` so the array is
   positional and in that same order; a `boolean` column is rendered `CASE
   WHEN col THEN 't' WHEN NOT col THEN 'f' ELSE NULL END` instead, since
   PostgreSQL's own wire text for a `boolean` column is `t`/`f`, not
   `::text`'s `true`/`false`. The three statements whose text embeds a
   literal table-oid array (`unnest('{16386,16393,16400}'::pg_catalog.
   oid[])` and its two-element sibling) have that literal replaced with a
   `SELECT array_agg(oid ORDER BY relname = 'notes' DESC, ...) FROM pg_class
   WHERE relname IN (...)` naming the same tables by name instead: a
   cluster's oid allocator counts across its whole lifetime, so the exact
   literal from the original capture's environment does not reproduce even
   from a fresh `initdb` on the same machine, let alone a different one, but
   the tables it names do not change. Every statement's `row_<NN>.json` this
   produces is merged into that statement's entry in `capture.json`, as
   `rows`, keyed by the same zero-padded index; the `columns` a statement
   has no matching `row_<NN>.json` for keeps no `rows` key.

## Regenerating

Re-run the steps above against a freshly initdb'd PostgreSQL 18.6 (or
whichever 18.x is on `PATH`/`$PG_BIN`); pg_dump's fixed catalog queries
change only across major PostgreSQL versions, so a capture from any 18.x
should match. `schema.sql` only needs to change if
`CAPTURED_SCHEMA` in `pg_dump_round_trip.rs` changes. The `rows` in
`capture.json` only need recapturing if `schema.sql` changes or a genuine
PostgreSQL 18 answer this compares against turns out to be wrong.
`capture.json` is written compactly (no pretty-printing); do not hand-edit
it.

## Direction of the value comparison

Every row the head returns must match a distinct PostgreSQL row for the
same statement; PostgreSQL may return rows the head does not. That
direction is deliberate: PostgreSQL reports objects the head does not
have and does not claim to have (TOAST relations, table row types,
predefined roles, the pg_toast and information_schema namespaces), so a
missing row is not by itself an error here. A row the head wrongly omits
for an object it does have is caught by the round trips instead, because
pg_dump's output then lacks that object and differs from PostgreSQL's
byte for byte.
