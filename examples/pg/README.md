# pg demo

A minimal demo of `turso_pg_head` (`pg/head`, see its `ARCH.md`), the
minimal PostgreSQL head: a row-security policy hiding another user's row, an
`UPDATE` refused with `0A000`, and the real `pg_dump` 18 dumping the result.

```
./examples/pg/demo.sh
```

Needs cargo and nix: it builds `pg-head-server` and takes `psql` and
`pg_dump` from the flake's pinned PostgreSQL (`nix build .#postgres`). It
runs on a scratch database that is deleted afterwards.
