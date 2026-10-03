# pg

A PostgreSQL interface for Turso

### Goals:

- **100% compatible, not 100% coverage.** Everything `pg` admits behaves
  exactly as PostgreSQL does, and everything else is refused. As coverage
  grows, existing behavior never changes, so widening `pg` is never a
  breaking change for anyone already using it.
- **A flawless path to PostgreSQL proper.** A database on `pg` can always
  be dumped with the real `pg_dump` and restored into PostgreSQL, with
  nothing lost or reinterpreted along the way.
- **Access control that cannot be circumvented.** Roles, privileges and
  row security are enforced on every statement, with no path around them.

The design that falls out of thoe goals is:

- **One pipeline, with chokepoints.** Every statement crosses the same
  stages, and each stage's output can only be produced by that stage. A
  statement that skipped authorization or row security does not compile.
  Each security decision is made in exactly one place.
- **Full enumeration, failing closed.** Every parsed field, statement kind,
  expression position and type states its rules exhaustively, so adding one
  without deciding its behavior is a compile error. Anything not yet
  implemented is refused with `0A000`, never approximated. What `pg` admits
  is exactly PostgreSQL; what it does not is a loud refusal.
- **As much as possible is a compile error.** What the compiler cannot
  check, PostgreSQL does: expected behavior is recorded from PostgreSQL 18,
  never typed by hand.

## The pipeline

Every statement is turned into engine input in five stages: **parse** it
with PostgreSQL's own grammar, **analyze** it (resolve every name and type),
**authorize** it against the stored privileges, **enforce** row security,
and **lower** the result to a typed AST for `turso_core`. Each stage's
output can only be built by that stage, so none can be skipped.

`pg_catalog` is structural, not a reporting view: it holds PostgreSQL 18's
own catalog tables, and analysis, authorization and row security consult it
to make every decision. `pg_dump` reads the same rows, which is how we hold
the catalog to 100% fidelity: if the dump is the same as PostgreSQL's,
the catalog holds everything enforcement depends on.

```
   SQL
    |
    v
  parse ........ libpg_query; every field handled or refused
    |
    v
  analyze <--------+
    |              |      pg_catalog
    v              |      PostgreSQL 18's own tables, every column:
  authorize <------+----  roles, ACLs, policies, types
    |              |               |
    v              |               | the same rows
  enforce (RLS) <--+               v
    |                           pg_dump
    v
  lower ........ typed turso_parser AST, never SQL text
    |
    v
  turso_core
```

## Current Scope

`pg` lands only what `pg_dump` needs: tables, roles, `GRANT`, row-level
security policies, and the queries and session statements `pg_dump` sends.
Within that surface the real `pg_dump` 18.6 dumps `pg` byte for byte as it
dumps PostgreSQL.

That is a proposed foundation, not the scope. A matching dump is the
graduation path itself, already proven, and an objective check for every
feature added next. Widening `pg` is then filling in against an oracle.
Each addition maps through the fully enumerated
surfaces (parse nodes, statement rules, expression positions, types,
errors), so the compiler lists everything that needs a decision.

## Testing

`pg_catalog` and `pg_dump` are two ends of one serialization pipeline:
DDL deserializes into the catalog, and `pg_dump` serializes it back to
SQL. That gives a property any schema must satisfy, and it is tested as
one:

- **Round-trip property.** Generated schemas are applied to `pg` and to a real PostgreSQL 18.6,
  both are dumped by the same `pg_dump`, and the two dumps must be
  identical. Where PostgreSQL refuses a generated input, `pg` must refuse it
  with the same SQLSTATE. The generator is seeded and deterministic, so
  every failure reproduces; `PG_DUMP_CASES` raises the case count. See
  [`generated_schemas_dump_from_the_head_exactly_as_from_postgres`](../wire/tests/pg_dump_generated.rs#L191),
  and [`a_schema_dumps_from_the_head_exactly_as_from_postgres`](../wire/tests/pg_dump_round_trip.rs#L53)
  for the same loop over one fixed schema.
- **Recorded corpus.** Expected behavior beyond the dump (query results,
  errors, privilege and row-security decisions) is a psql transcript
  recorded from PostgreSQL 18.6, and the output must be identical. Nothing is
  typed by hand, and there is no known-bad list.

An exact, objective signal is also what makes the long tail tractable:
each new feature is done when its generated and recorded cases match
PostgreSQL. `make test-pg-head` runs everything.

## Try it

[`examples/pg/demo.sh`](../../examples/pg/demo.sh) starts a server and shows
a row-security policy hiding another user's row, an unsupported statement
refused, and `pg_dump` reading the schema and policy back.
