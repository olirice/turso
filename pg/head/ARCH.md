# turso_pg_head

A minimal PostgreSQL head for Turso. It parses PostgreSQL with
`libpg_query`, admits a closed set of statements, and refuses everything
else with `0A000` naming what was refused. It hands `turso_core` typed
`turso_parser` ASTs, never SQL text. `pg/wire` serves it over the
PostgreSQL wire protocol. It exists to prove, against a live PostgreSQL
18, that a Turso-backed head can round trip `pg_dump 18 --schema-only`
byte for byte and reproduce PostgreSQL's privilege and row-security
decisions through one typed path.

## Rules

1. **One path.** Every statement crosses the same stages; every relation
   reference is resolved by one walk (`analyze/walk.rs`); every tree has
   one traversal (`parse/walk.rs`, `analyze/typing/walk.rs`); SQL text is
   parsed at one entry (`parse::statement`), including stored policy text.
2. **Correct by construction.** Stage outputs have private fields and each
   stage takes only the previous stage's type, so a skipped stage does not
   compile. External identifiers are minted only with a proof one module
   owns (`FromParser`, `FromCatalog`, `FromStartup`); catalog writes need a
   `ProjectObject`.
3. **Decisions are exhaustive tables.** Statement admission
   (`Statement::rules`), expression positions (`Position::rules`), per-type
   facts (`SupportedType::facts`), error templates (`error.rs`) and catalog
   rows (generated structs) are full matches or struct literals, so a new
   variant does not compile until every rule is stated.
4. **Nothing is silently dropped.** Every admitted pg_query struct is
   destructured with every field named; each field is used or required to
   be empty.
5. **Strings are not interfaces.** Names are typed (`ident.rs`) until
   rendered through one renderer per output context (`render/`); command
   tags are `CommandTag`; errors are `PgError` variants with typed fields.
6. **Never hard fail.** Crate lints deny `unwrap`, `expect`, `panic`,
   indexing, string slicing, lossy casts, `unsafe` and wildcard enum arms
   in production code. Every failure is a `HeadError` with a SQLSTATE.
7. **Captured, not remembered.** Catalog shapes, built-in rows, types,
   implicit casts and SQLSTATEs come from a freshly initdb'd PostgreSQL 18
   (`capture/`), generated into Rust by `build.rs`.
8. **Only assert what PostgreSQL shows.** Expected output is recorded from
   PostgreSQL 18, never typed.

## Pipeline

```text
SQL text -> admit -> Admitted -> analyze -> Analyzed -> authorize
  -> Authorized -> enforce -> Enforced -> lower -> Lowered -> engine::execute
```

`admit` (`pipeline.rs`) applies `Statement::rules` against the transaction
state before analysis. Error order differs per statement in PostgreSQL;
each statement's stage placement is pinned by the corpus (for example
`CREATE TABLE` checks the schema privilege before the definition, `SELECT`
reports a missing column before a missing privilege).

| Module | Role |
|---|---|
| `parse/` | pg_query to the closed `Statement` tree; `Location`; `walk.rs` |
| `analyze/` | Names and types against the catalog; `walk.rs` (relations), `types.rs` (the type table), `views.rs` (`CatalogView`) |
| `analyze/typing/` | The typed tree; `context.rs` (the position gate), `check.rs`, `coerce.rs`, `aggregate.rs` |
| `security/` | Privileges and the ACL model, row security, `Stored` |
| `lower/` | Enforced statements to engine AST (`sql.rs`) and catalog writes |
| `engine/` | The only engine access, the catalog store, the constant layer |
| `engine_hooks.rs` | turso_core's `Dialect` hooks: catalog registration, head-computed functions |
| `render/` | Typed values, identifiers, ACLs, floats and policy text to PostgreSQL text |
| `error.rs` | Generated `SqlState`, `PgError`, `NotSupportedFeature`, the one template table |
| `session.rs`, `session/` | Session state, transactions, settings, session functions |
| `catalog.rs`, `catalog/pg.rs` | The catalog snapshot and the generated capture |

## Decisions

**Gates.** `Statement::rules` states per statement kind its tag, whether
it takes the engine write lock, whether it is refused in a read-only
transaction, runs in an aborted one, or takes a snapshot (`LOCK` takes the
lock yet runs read-only, as pg_dump needs). `Position::rules` states per
expression position what subqueries, aggregates, set-returning and
row-computed functions may do there, and whether errors there carry a
position (a policy's `USING` reports none). The type checker needs a
`Context`, and only a `Position` builds one.

**Errors and positions.** A `PgError` renders its SQLSTATE, message,
detail and hint from one table; raise sites pass typed values.
`SqlState` is generated from PostgreSQL 18's `errcodes.txt`. Source
locations are a `Location` built only at the parse edge and attached
through `Context::locate` where PostgreSQL 18 reports one;
`Session::execute` converts them to character positions.

**One engine door.** `EngineConnection` is private to `engine`. Only
`engine::execute` (taking a `Lowered`) and `engine::store` reach it, through
stock `prepare_translated_cmd`. Values cross only as bound parameters.

**The catalog is PostgreSQL 18's own tables**, with all their columns,
stored as `rel_<oid>` with `col_<attnum>` columns. OIDs are stored (a
counter from 16384, PostgreSQL's own for built-ins), so catalog queries
are joins over stored columns. Rows the head writes are generated structs
with every column required.

**Two layers.** Rows fixed for a head version (the catalog's description
of itself, `pg_type`, `pg_am`, registered `pg_proc` rows) are compiled in
and served as read-only engine tables. A project file holds only its own
objects; catalog tables are created by the first write that needs them. A
write to a built-in object is refused: `ProjectObject` exists only for
OIDs from 16384 up and the four objects bootstrap seeds.

**Format version.** The head's state table records a catalog format
version; a database with another version is refused on open.

**Types.** `SupportedType::facts` states for every type the head produces
values of: declarable, castable, btree opclass, engine storage type,
literal folding and text output. `CatalogOnlyType` covers types the
capture exposes but the head never produces. CASE and `ARRAY[...]` take
their common type from `typcategory` and the captured implicit `pg_cast`
rows (mutually castable ties: CASE takes the later arm, `ARRAY[...]` the
earlier element, as probed).

**Stored expressions.** A policy stores the text `pg_get_expr` returns. A
`Stored` is built only by `canonicalize`: render, reparse and re-analyze
through `parse::statement`, render again, require identical text. Catalog
load runs the same check. Forms the renderer cannot produce are refused
at `CREATE POLICY` as a `PolicyExpressionForm`.

**Catalog cache.** Each `Head` shares one cached catalog keyed by the
stored catalog version, which every catalog write bumps. A catalog built
inside a client transaction is never shared.

**Function evaluation.** A function the engine can compute from its
arguments runs in the engine (natively, or through `engine_hooks`). One
that needs session or catalog state runs in the head: folded while
lowering for literal arguments, evaluated per returned row for columns
(only as a top-level select item). No function reads the database from
inside the engine. `FunctionHandle::evaluation` decides this.

**Row security.** The relation walk records, per reference, the role it
is checked as and its row-security decision. Lowering splices each policy
predicate in as a derived table at that reference, so outer joins stay
correct. A view is checked as its owner.

**Uniqueness.** Every write holds the single writer lock, so the head
checks each inserted row's primary key and raises `23505` itself. An
engine constraint error is never parsed; one becomes an internal error.

## Testing

`make test-pg-head` runs everything. Capturing, recording and the pg_dump
tests need PostgreSQL of exactly the captured version (`version.txt`): the
flake pins it as `.#postgres`, apart from the toolchain, and the tests and
`head.py` refuse any other (`PG_TESTS=skip` skips the pg_dump tests
instead). Running the corpus needs none.

| Suite | Proves |
|---|---|
| `postgres/conformance/head/` | The corpus: psql transcripts recorded from PostgreSQL 18 (`head.py record`, which also checks the runner reproduces real psql), run against `pg-head-server` (`head.py run`). No known-bad list. `refusals.sql` is the one head-authored file, including one probe per reason the other 270 of `authmatrix.sql`'s 370 scenarios cannot run |
| `pg/wire/tests/pg_dump_round_trip.rs` | A fixed schema dumps byte-identically to PostgreSQL 18 |
| `pg/wire/tests/pg_dump_generated.rs` | Generated schemas, including fuzzed policy `USING` expressions, dump byte-identically or are refused with PostgreSQL's SQLSTATE or a typed `0A000` (`PG_DUMP_CASES` sets the count) |
| `pg/wire/tests/pg_dump_statements.rs` | Every statement pg_dump 18 sends runs with PostgreSQL's column names, types and values |
| `pg/wire/tests/wire.rs` | The wire protocol: queries, errors, transaction status, startup failures |
| `tests/*.rs` | Only what psql cannot express (two sessions open at once, reopening the same database file, the syntax-error cursor) and a few PostgreSQL/head divergences the corpus cannot record either way, each with its reason |

## Scope

Admitted: `CREATE TABLE` (`integer`, `bigint`, `text`, `NOT NULL`, a
single-column primary key), `CREATE ROLE` (`LOGIN`), `GRANT`, `CREATE
POLICY`, `ALTER TABLE ... ROW LEVEL SECURITY`, literal `INSERT`, the
`SELECT` surface pg_dump uses, and the session statements pg_dump sends
(`BEGIN`, `COMMIT`, `ROLLBACK`, `SET`, `SET TRANSACTION` at `REPEATABLE
READ`, `LOCK ... ACCESS SHARE`, `PREPARE`, `EXECUTE`). Refused, among
everything else:

- `UPDATE`, `DELETE`, `DROP`, `ALTER COLUMN`.
- Schemas other than `public` and `pg_catalog`; role attributes beyond
  `LOGIN`; role membership.
- `GROUP BY`, `RIGHT`/`FULL JOIN`, deduplicating `UNION`, `INTERSECT`,
  `EXCEPT`, `LIMIT`/`OFFSET`, `DISTINCT ON`.
- Sequences, indexes beyond the primary key, views beyond `pg_roles`,
  `pg_settings` and `pg_seclabels`.
- Function calls in a policy; changes to built-in catalog rows.
- More than one statement per wire message; settings other than the ten
  pg_dump sets; isolation levels other than `REPEATABLE READ`.

## Known costs

- The whole catalog reloads after any DDL; invalidating by relation is
  the growth path.
- The duplicate-key check costs one engine lookup per inserted row; a
  typed unique-violation error from `turso_core` would remove it.
- A second concurrent writer gets a busy error instead of waiting; pg_dump
  never writes.
