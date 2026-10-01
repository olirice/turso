#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="$(mktemp -d /tmp/pg_capture.XXXXXX)"
SOCK="$(mktemp -d /tmp/pg_capture_sock.XXXXXX)"
PORT=55432

# $PG_BIN if set, else the flake's pinned PostgreSQL (`.#postgres`). The
# capture defines the version every recording and pg_dump test must match,
# so capture with the pinned build unless deliberately changing it.
if [ -n "${PG_BIN:-}" ]; then
  pg() { "$PG_BIN/$1" "${@:2}"; }
else
  PINNED="$(nix build "$HERE/../../..#postgres^out" --no-link --print-out-paths)"
  pg() { "$PINNED/bin/$1" "${@:2}"; }
fi

cleanup() {
  pg pg_ctl -D "$WORK/data" stop -m fast >/dev/null 2>&1 || true
  rm -rf "$WORK" "$SOCK"
}
trap cleanup EXIT

pg initdb -D "$WORK/data" -U postgres --locale=C -E UTF8

pg pg_ctl -D "$WORK/data" -l "$WORK/pg.log" \
  -o "-p $PORT -k $SOCK -h ''" start

psql() {
  pg psql -h "$SOCK" -p "$PORT" -U postgres -d postgres "$@"
}

psql -Atq -c "SELECT version();" > "$HERE/out/version.txt"

psql -Atq -f "$HERE/sql/01_relations.sql" > "$HERE/out/relations.json"
psql -Atq -f "$HERE/sql/02_columns.sql" > "$HERE/out/columns.json"
psql -Atq -f "$HERE/sql/03_indexes.sql" > "$HERE/out/indexes.json"
psql -Atq -f "$HERE/sql/04_types.sql" > "$HERE/out/types.json"
psql -Atq -f "$HERE/sql/05_casts.sql" > "$HERE/out/casts.json"

psql -Atq -f "$HERE/sql/06a_bootstrap_pg_authid.sql" > "$HERE/out/bootstrap_rows/pg_authid.json"
psql -Atq -f "$HERE/sql/06b_bootstrap_pg_namespace.sql" > "$HERE/out/bootstrap_rows/pg_namespace.json"
psql -Atq -f "$HERE/sql/06c_bootstrap_pg_am.sql" > "$HERE/out/bootstrap_rows/pg_am.json"
psql -Atq -f "$HERE/sql/06e_bootstrap_pg_database.sql" > "$HERE/out/bootstrap_rows/pg_database.json"
psql -Atq -f "$HERE/sql/06f_bootstrap_pg_tablespace.sql" > "$HERE/out/bootstrap_rows/pg_tablespace.json"
psql -Atq -f "$HERE/sql/06g_bootstrap_pg_init_privs.sql" > "$HERE/out/bootstrap_rows/pg_init_privs.json"
psql -Atq -f "$HERE/sql/06h_bootstrap_pg_description.sql" > "$HERE/out/bootstrap_rows/pg_description.json"

psql -Atq -f "$HERE/sql/07a_bootstrap_pg_class.sql" > "$HERE/out/bootstrap_rows/pg_class.json"
psql -Atq -f "$HERE/sql/07b_bootstrap_pg_attribute.sql" > "$HERE/out/bootstrap_rows/pg_attribute.json"
psql -Atq -f "$HERE/sql/07c_bootstrap_pg_index.sql" > "$HERE/out/bootstrap_rows/pg_index.json"

psql -Atq -f "$HERE/sql/08a_bootstrap_pg_proc.sql" > "$HERE/out/bootstrap_rows/pg_proc.json"

echo "Captured into $HERE/out. Diff against the checked-in tree is the review." >&2
