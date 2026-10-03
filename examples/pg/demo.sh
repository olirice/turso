#!/usr/bin/env bash
# A minimal demo of turso_pg_head: row security as PostgreSQL applies it, a
# loud refusal for what the head does not admit, and pg_dump against it.
# Needs cargo and nix (for the pinned PostgreSQL client tools).
set -euo pipefail

# Find the repo root, get psql and pg_dump from the flake's pinned
# PostgreSQL 18.6, and build the head's wire server.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PG="$(nix build "$ROOT#postgres^out" --no-link --print-out-paths)/bin"
cargo build -q --manifest-path "$ROOT/Cargo.toml" -p turso_pg_head_wire --bin pg-head-server

# Start the server on a scratch database file, stop it and delete the file
# when the script exits, and wait until it accepts connections.
WORK="$(mktemp -d)"
PORT=55439
"${CARGO_TARGET_DIR:-$ROOT/target}/debug/pg-head-server" "127.0.0.1:$PORT" "$WORK/demo.db" &
SERVER=$!
trap 'kill "$SERVER"; rm -rf "$WORK"' EXIT
until "$PG/pg_isready" -q -h 127.0.0.1 -p "$PORT"; do sleep 0.2; done

# psql <role>: run the SQL that follows as that role, echoing each statement.
psql() { "$PG/psql" -X -a -h 127.0.0.1 -p "$PORT" -d demo -U "$@"; }

# Set up a table with a row-security policy: each user sees only their own
# notes. Two rows go in, one written by alice and one by bob.
echo "== as postgres: a table only its owner's rows are visible in"
psql postgres <<'SQL'
CREATE ROLE alice LOGIN;
CREATE TABLE notes (id integer PRIMARY KEY, author text NOT NULL, body text);
ALTER TABLE notes ENABLE ROW LEVEL SECURITY;
CREATE POLICY own_notes ON notes FOR SELECT USING (author = CURRENT_USER);
GRANT SELECT ON notes TO alice;
INSERT INTO notes VALUES (1, 'alice', 'hello'), (2, 'bob', 'secret');
SQL

# Connect as alice: the same SELECT returns only her row.
echo "== as alice: the policy hides bob's row"
psql alice <<'SQL'
SELECT * FROM notes;
SQL

# UPDATE is outside what the head supports, so it is refused with SQLSTATE
# 0A000 instead of being run approximately. Verbose errors show the code;
# "|| true" keeps the script going past the expected error.
echo "== outside the admitted set: refused, never approximated"
psql postgres <<'SQL' || true
\set VERBOSITY verbose
UPDATE notes SET body = 'changed';
SQL

# Run the real pg_dump against the head: the table, its primary key, the
# grant and the policy come back as PostgreSQL would dump them.
echo "== pg_dump 18 against the head"
"$PG/pg_dump" --schema-only --restrict-key=demo -h 127.0.0.1 -p "$PORT" -U postgres demo
