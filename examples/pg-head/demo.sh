#!/usr/bin/env bash
# A minute with turso_pg_head: row security as PostgreSQL applies it, a
# loud refusal for what the head does not admit, and pg_dump against it.
# Needs cargo and nix (for the pinned PostgreSQL client tools).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PG="$(nix build "$ROOT#postgres^out" --no-link --print-out-paths)/bin"
cargo build -q --manifest-path "$ROOT/Cargo.toml" -p turso_pg_head_wire --bin pg-head-server

WORK="$(mktemp -d)"
PORT=55439
"${CARGO_TARGET_DIR:-$ROOT/target}/debug/pg-head-server" "127.0.0.1:$PORT" "$WORK/demo.db" &
SERVER=$!
trap 'kill "$SERVER"; rm -rf "$WORK"' EXIT
until "$PG/pg_isready" -q -h 127.0.0.1 -p "$PORT"; do sleep 0.2; done

psql() { "$PG/psql" -X -a -h 127.0.0.1 -p "$PORT" -d demo -U "$@"; }

echo "== as postgres: a table only its owner's rows are visible in"
psql postgres <<'SQL'
CREATE ROLE alice LOGIN;
CREATE TABLE notes (id integer PRIMARY KEY, author text NOT NULL, body text);
ALTER TABLE notes ENABLE ROW LEVEL SECURITY;
CREATE POLICY own_notes ON notes FOR SELECT USING (author = CURRENT_USER);
GRANT SELECT ON notes TO alice;
INSERT INTO notes VALUES (1, 'alice', 'hello'), (2, 'bob', 'secret');
SQL

echo "== as alice: the policy hides bob's row"
psql alice <<'SQL'
SELECT * FROM notes;
SQL

echo "== outside the admitted set: refused, never approximated"
psql postgres <<'SQL' || true
\set VERBOSITY verbose
UPDATE notes SET body = 'changed';
SQL

echo "== pg_dump 18 against the head"
"$PG/pg_dump" --schema-only --restrict-key=demo -h 127.0.0.1 -p "$PORT" -U postgres demo
