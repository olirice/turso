# pg_dump DDL round-trip corpus

Measures catalog fidelity — does `tursopg`'s `pg_catalog` emulation
(`postgres/frontend/catalog.rs`) represent DDL objects well enough for a real
`pg_dump` to reconstruct them correctly — as opposed to SQL statement/result
conformance, which the `upstream/` and `pg-sqltests/` corpora next door
already cover.

## Provenance

`upstream/002_pg_dump.pl` is PostgreSQL's own test suite for `pg_dump`,
imported **verbatim** from `src/bin/pg_dump/t/002_pg_dump.pl`, pinned to
`REL_16_STABLE`. Like `conformance/upstream/`, never edit it to match Turso
behavior — it is the corpus's oracle, not a bug to fix.

That file defines one `%tests` entry per DDL construct: the SQL that creates
it (`create_sql`) and the regexp pg_dump's own tests require to appear in a
`--schema-only` dump of it (`regexp`) — literally PostgreSQL's own
"is this DDL construct dumped correctly" assertions. `extract.pl` pulls out
the entries tagged `section_pre_data` (schema-defining DDL, as opposed to
data/ACL/option-flag tests) into `corpus.json`.

## Regenerating the corpus

```bash
perl extract.pl upstream/002_pg_dump.pl > corpus.json
```

To pick up a newer PostgreSQL release, replace `upstream/002_pg_dump.pl` with
the corresponding file from that release's tag and re-run extraction.

## Running the compliance check

```bash
perl check.pl                  # whole corpus
perl check.pl --filter TABLE   # only entries whose name contains TABLE
perl check.pl --keep           # keep the temp workdir + schema dump for inspection
```

Builds `tursopg`, starts it on a fresh temp database, applies each entry's
`create_sql` over the wire protocol, runs one real `pg_dump --schema-only`
against the result, and checks each entry's regexp against the dump text.
Reports two numbers, deliberately kept separate:

- **exec rate** — fraction of DDL constructs `tursopg` accepts at all.
- **dump rate** — of those accepted, fraction whose `pg_dump` output matches
  what real PostgreSQL's own tests require. A construct can execute fine and
  still fail here if the catalog emulation misrepresents it (wrong type name,
  dropped constraint, missing default) — that gap is exactly what this corpus
  exists to catch.

A per-entry, machine-readable report is written to `results.json` (git-ignored)
for trend tracking across commits.

This check is exploratory, not CI-gated: `tursopg`'s DDL surface is still
young, so a low pass rate today is expected. Track the trend over time rather
than reading any single run as pass/fail.
