# turso_pg_head regress corpus

The primary tests for `turso_pg_head` (the minimal PostgreSQL head, see
`pg/head/ARCH.md`): `pgregress` (`postgres/regress/`) run against a real
`pg-head-server`, comparing byte for byte against recorded psql
transcripts. Internals are constrained by types and lints, not internal
tests; this corpus is the external, PostgreSQL-authored yardstick.

## Provenance

Two kinds of file, never mixed in the same `.out`:

- **PostgreSQL-authored** (every `*.sql` except `refusals.sql`): the
  expected `.out` is recorded by running the script through real
  `psql -X -a -q` against a throwaway PostgreSQL 18 server, under
  pg_regress's pinned transcript environment. Never edited by hand.
- **`refusals.sql`, head-authored**: statements PostgreSQL 18 admits but
  the head refuses outright (`0A000`) by design, because the feature is
  outside its closed, admitted set (see `pg/head/ARCH.md`'s Scope
  section). There is nothing to record from PostgreSQL for these, so the
  `.out` is instead recorded straight from a fresh `pg-head-server`: a
  reviewed snapshot, not a live oracle result.

## Recording

```
postgres/conformance/head.py record                  # every file
postgres/conformance/head.py record create_table     # one file
```

Recording needs PostgreSQL of exactly the captured version
(`pg/head/capture/out/version.txt`): the flake's pinned `.#postgres`, used
automatically when `$PG_BIN` is unset. `head.py` refuses any other version,
and the pg_dump tests in `pg/wire/tests` do the same.

Without Nix, any PostgreSQL 18.6 build with the server and its tools in
one bin directory works through `$PG_BIN` (a pinned PGDG package while
18.6 is still published, or a source build). The official `postgres:18.6`
Docker image contains the exact build, but only for use inside the image:
its binaries link the image's own libraries and `initdb` reads its data
files, so copying `bin/` out does not give a working `$PG_BIN`.

### Changing the PostgreSQL version

pg_dump's output changes between minor releases, so the version moves only
as one change: point the flake's `nixpkgs-postgres` input at a nixpkgs
revision with the new build (`nix flake lock`), re-run
`pg/head/capture/capture.sh`, re-record with `head.py record`, and fix
whatever the new transcripts and pg_dump tests show.

For each PostgreSQL-authored file, `head.py record` initdb's a throwaway,
trust-authenticated PostgreSQL 18 cluster, runs the script through real
`psql -X -a -q` (fed on stdin, like pg_regress itself, so error messages
are not prefixed with `psql:path:line:`) under the pinned environment
(`PGTZ=America/Los_Angeles`, `PGDATESTYLE=Postgres, MDY`,
`PGOPTIONS=-c intervalstyle=postgres_verbose`; the same values
`postgres/regress/main.rs` sends as startup parameters), and writes the
combined stdout/stderr transcript as `.out`.

It then points the `pgregress` runner itself at a second, independently
fresh instance of the same kind of server (real PostgreSQL for a
PostgreSQL-authored file, a fresh `pg-head-server` for `refusals.sql`)
and diffs its output against the just-recorded `.out`. This proves the
runner's psql-transcript emulation is faithful for that file: a mismatch
here is a bug in the runner's emulation, not in the recording, and is
reported rather than papered over (the `.out` is still written as
recorded from the real client either way).

## Running

```
make -C postgres/conformance run-head
```

Starts a fresh `pg-head-server` on a temporary database file and a free
port, runs `pgregress` over this directory in schedule order, and fails
on any diff. Wired into `make test-pg-head` and so the `pg-head-run` CI
job.

## Known gaps

Reported, not papered over:

- **Syntax errors carry no position.** `pg_query`'s public API discards
  `libpg_query`'s `cursorpos` for a parse error (`pg_query::Error::Parse`
  is a bare `String`), so a bare syntax error cannot get a `LINE n:` / `^`
  pair the way PostgreSQL 18 itself reports one. Every other position
  PostgreSQL attaches (undefined column/table/type/function, ambiguous
  column, multiple primary keys) comes from a pg_query node's own
  `location` field instead, and the head does carry those (`HeadError`'s
  `position`, sent as the wire `P` field); see `pg/head/src/error.rs`.
  Until the head can recover a syntax error's cursor position, the
  corresponding cases stay as Rust tests in `pg/head/tests/`.
- **One statement per wire message only.** psql, and so the runner, never
  batches more than one `;`-terminated statement into a single message,
  so the head's "one statement per call" refusal is not reachable through
  this corpus.
- **Long `LINE n:` excerpts are not clipped.** Real psql truncates a
  statement's echoed `LINE n:` text (replacing the clipped end with
  `...`) once it exceeds a fixed width; `postgres/regress/main.rs` does
  not emulate this yet (documented in its own module comment). A
  position-bearing statement here must stay short enough that PostgreSQL
  18 would not clip it (60 characters was the measured cutoff), or the
  recording and the runner's replay diverge.
- **A connection failure embeds its port, which is not reproducible.**
  `record` and `run` each start a server on an independently chosen free
  port (`postgres/conformance/run.py`'s `free_port`), so a `\c`/`\connect`
  failure's `connection to server at "...", port N failed` line can never
  byte-match between a recording and a later run: `N` differs every time.
  The runner's `\c` role-switch support itself is exercised directly (see
  `postgres/regress/main.rs`'s tests) and works; only a *recorded*
  connection-failure transcript is not reproducible with this harness's
  per-run random ports. `pg/head/tests/create_table.rs`'s
  `an_unknown_role_cannot_connect` stays a Rust test for this reason.
- **psql only ever holds one connection at a time.** A case that needs two
  genuinely concurrent sessions (one idle mid-transaction while another
  acts, or a statement prepared in one session seeing a privilege change
  committed by another) has no corpus form; `pg/wire`'s own wire test and
  the two-session cases in `pg/head/tests/rows.rs`, `session.rs` and
  `prepare_execute.rs` stay as Rust tests for this reason.
- **Reopening the same on-disk database needs two `Head`s over one file,
  not one live server.** `pg/head/tests/pg_get_functions.rs`'s
  `a_policys_using_expression_persists_across_a_reopen_of_the_same_database`
  stays a Rust test for this reason.
- **A handful of cases are PostgreSQL 18 and the head genuinely disagreeing,
  not a refusal either file's format can record.** Recording these from
  PostgreSQL would pin PostgreSQL's own output as "expected" over a
  statement the head answers differently; recording from the head would
  misrepresent a real divergence as a "refuses by design" entry. Each
  stays a Rust test with the divergence spelled out, reported rather than
  silently snapshotted either way:
  - `pg/head/tests/pg_catalog.rs`: the head's catalog is deliberately
    reduced (no `information_schema`, `pg_toast`, extensions, or most
    built-in opclasses), so a live PostgreSQL 18's own catalog content
    never matches it row for row. `insert_into_pg_class_is_refused` is
    the same kind of mismatch for a write: PostgreSQL 18 admits the
    `INSERT` (a superuser bypasses the catalog-protection check) and then
    fails for an unrelated reason (a `NOT NULL` violation on a system
    column this `INSERT` never supplied); the head refuses it by name
    instead, before it ever reaches that check.
  - `pg/head/tests/queries.rs`: `array_agg(... ORDER BY ...)` in a query
    that itself has `ORDER BY`, and an arbitrary expression ordering a
    `UNION`, are refused by both, but for different underlying reasons
    (and, for the `UNION` case, a different message, detail, hint and
    position the head's expression tree cannot yet attach).

## Layout

- `<name>.sql` / `<name>.out`: script and its recorded transcript
- `schedule`: run order (see `postgres/conformance/upstream/schedule` for
  why order matters even for a serial runner)
- `head.py` (`postgres/conformance/`): the `record` and `run` subcommands
