#!/usr/bin/env python3
# Copyright 2023-2026 the Turso authors. All rights reserved. MIT license.
"""Records and runs postgres/conformance/head/*.sql, the pgregress corpus
for turso_pg_head (the minimal PostgreSQL head; see pg/head/ARCH.md and
head/README.md).

`record` re-records every `*.sql` file's expected `.out` transcript.
PostgreSQL-authored files (everything except `refusals.sql`) are recorded
by running the script through a real, throwaway PostgreSQL server of
exactly the captured version (`$PG_BIN`, else the flake's pinned
`.#postgres`) with real `psql -X -a -q`, under pg_regress's pinned
environment (`PGTZ`, `PGDATESTYLE`, `PGOPTIONS`; the same values
`postgres/regress/main.rs` sends as startup parameters, so a recording
matches what the runner itself would send). `refusals.sql` is
head-authored: PostgreSQL succeeds where the head refuses by design, so
there is nothing to record from PostgreSQL, and its `.out` is instead
recorded straight from a fresh `pg-head-server` (a reviewed snapshot).
After recording, `record` points the pgregress runner (`postgres/regress`)
at a second, independently fresh instance of the same kind of server and
diffs its output against the just-recorded `.out`, proving the runner's
psql-transcript emulation is faithful. A mismatch there is a runner bug,
not a recording bug: it is reported, not papered over.

`run` starts a fresh `pg-head-server` on a temporary database file and a
free port, runs the corpus (or the given names) through `pgregress` in
schedule order, and fails on any diff. Unlike `run.py` (the upstream
corpus, which ratchets known-bad tests), every test here must pass.

Usage:
    postgres/conformance/head.py record            # every file
    postgres/conformance/head.py record create_table
    postgres/conformance/head.py run                # whole corpus
    postgres/conformance/head.py run create_table
"""

import os
import socket
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run as pgrun  # noqa: E402 (reuses target_dir/free_port/wait_for_server/stop)

REPO_ROOT = Path(__file__).resolve().parents[2]
HEAD_DIR = REPO_ROOT / "postgres" / "conformance" / "head"

# Recorded straight from the head, never from PostgreSQL: see the module
# docstring.
HEAD_AUTHORED = {"refusals"}

# What pg_regress's own psql environment pins, replicated exactly by
# postgres/regress/main.rs's startup parameters, so a script's output does
# not depend on the machine's locale or timezone.
PINNED_ENV = {
    "PGTZ": "America/Los_Angeles",
    "PGDATESTYLE": "Postgres, MDY",
    "PGOPTIONS": "-c intervalstyle=postgres_verbose",
}


CAPTURED_VERSION = (REPO_ROOT / "pg/head/capture/out/version.txt").read_text().split()[1]


def pg_bin() -> Path:
    """The PostgreSQL the capture came from, exactly: $PG_BIN, else the
    flake's pinned build (`nix build .#postgres`)."""
    bin_dir = os.environ.get("PG_BIN")
    if bin_dir:
        path = Path(bin_dir)
    else:
        out = subprocess.run(
            ["nix", "build", f"{REPO_ROOT}#postgres^out", "--no-link", "--print-out-paths"],
            check=True, capture_output=True, text=True,
        ).stdout.strip()
        path = Path(out) / "bin"
    version = subprocess.run(
        [str(path / "postgres"), "--version"], check=True, capture_output=True, text=True
    ).stdout.split()[2]
    if version != CAPTURED_VERSION:
        sys.exit(
            f"error: {path} is PostgreSQL {version}, but the capture and every recorded "
            f"transcript are PostgreSQL {CAPTURED_VERSION}; unset PG_BIN to use the "
            "flake's pinned build"
        )
    return path


def build_binaries() -> Path:
    subprocess.run(
        ["cargo", "build", "-p", "turso_pg_head_wire", "-p", "turso_pg_regress"],
        cwd=REPO_ROOT,
        check=True,
    )
    return pgrun.target_dir() / "debug"


class PostgresServer:
    """A throwaway, trust-authenticated PostgreSQL 18 server. Each instance
    is a fresh cluster: the recording pass and the verification pass must
    not share one, or the second pass's CREATE TABLE statements find the
    first pass's tables already there."""

    def __init__(self, work: Path, bin_dir: Path):
        self.bin_dir = bin_dir
        self.port = pgrun.free_port()
        self.data = work / "pgdata"
        subprocess.run(
            [str(bin_dir / "initdb"), "-U", "postgres", "--auth=trust", "-D", str(self.data)],
            check=True,
            capture_output=True,
        )
        self.proc = subprocess.Popen(
            [
                str(bin_dir / "pg_ctl"),
                "-D",
                str(self.data),
                "-l",
                str(work / "pg.log"),
                "-o",
                f"-p {self.port} -h 127.0.0.1 -k {work}",
                "-w",
                "start",
            ],
        )
        self.proc.wait()
        subprocess.run(
            [str(bin_dir / "createdb"), "-h", "127.0.0.1", "-p", str(self.port), "-U", "postgres", "regression"],
            check=True,
            capture_output=True,
        )

    def dsn(self) -> str:
        return f"postgres://postgres@127.0.0.1:{self.port}/regression"

    def stop(self) -> None:
        subprocess.run(
            [str(self.bin_dir / "pg_ctl"), "-D", str(self.data), "-m", "fast", "stop"],
            capture_output=True,
        )


class HeadServer:
    """A fresh `pg-head-server` over a throwaway database file."""

    def __init__(self, work: Path, bins: Path):
        self.port = pgrun.free_port()
        db_file = work / "regression.db"
        self.proc = subprocess.Popen(
            [str(bins / "pg-head-server"), f"127.0.0.1:{self.port}", str(db_file)],
        )
        # Shared with the upstream corpus's tursopg wait loop; a timeout
        # error names tursopg rather than pg-head-server, harmless since it
        # only fires when the server never came up at all.
        pgrun.wait_for_server(self.proc, self.port)

    def dsn(self) -> str:
        return f"postgres://postgres@127.0.0.1:{self.port}/regression"

    def stop(self) -> None:
        pgrun.stop(self.proc)


def fresh_server(kind: str, work: Path, bin_dir: Path, bins: Path):
    work.mkdir(parents=True, exist_ok=True)
    if kind == "head":
        return HeadServer(work, bins)
    return PostgresServer(work, bin_dir)


def run_psql(bin_dir: Path, host: str, port: int, sql_path: Path) -> str:
    """Runs a script through real psql under pg_regress's pinned transcript
    environment, returning the transcript. Errors interleave with query
    results in the order psql wrote them (like pg_regress, which redirects
    both to the same file), not stdout-then-stderr: capturing the two
    streams separately would reorder every error to the end."""
    env = dict(os.environ, **PINNED_ENV)
    # pg_regress feeds the script on stdin (`psql ... < test.sql`), not
    # `-f`: `-f` prefixes every error with `psql:path:line:`, which the
    # runner's own transcript (and real pg_regress's) never has.
    with open(sql_path) as script:
        result = subprocess.run(
            [
                str(bin_dir / "psql"),
                "-X",
                "-a",
                "-q",
                "-h",
                host,
                "-p",
                str(port),
                "-U",
                "postgres",
                "-d",
                "regression",
            ],
            env=env,
            stdin=script,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
    return result.stdout


def verify_runner_fidelity(pgregress_bin: Path, dsn: str, sql_path: Path, results_dir: Path) -> bool:
    """Points the pgregress runner itself at the same server the recording
    came from, and reports whether it reproduces the just-written `.out`
    byte for byte."""
    result = subprocess.run(
        [str(pgregress_bin), "--dsn", dsn, "--results", str(results_dir), str(sql_path)],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    print(result.stdout)
    if result.stderr:
        print(result.stderr, file=sys.stderr)
    return result.returncode == 0


def cmd_record(names: list[str]) -> int:
    names = names or sorted(p.stem for p in HEAD_DIR.glob("*.sql"))
    bin_dir = pg_bin()
    bins = build_binaries()
    pgregress_bin = bins / "pgregress"

    ok = True
    # Under /tmp, not the default (often much longer) TMPDIR: a PostgreSQL
    # unix socket path must fit in a `sockaddr_un`, about 100 bytes.
    with tempfile.TemporaryDirectory(prefix="pghead-record-", dir="/tmp") as tmp:
        work = Path(tmp)
        results_dir = work / "results"
        results_dir.mkdir()

        for name in names:
            sql_path = HEAD_DIR / f"{name}.sql"
            out_path = HEAD_DIR / f"{name}.out"
            if not sql_path.is_file():
                sys.exit(f"error: no such script: {sql_path}")

            kind = "head" if name in HEAD_AUTHORED else "postgres"
            label = "the head" if kind == "head" else "PostgreSQL 18"

            # A fresh server for the recording pass: CREATE TABLE and
            # friends must land in an empty database.
            recorder = fresh_server(kind, work / f"{name}-record", bin_dir, bins)
            try:
                print(f"recording {name} from {label} ...")
                transcript = run_psql(bin_dir, "127.0.0.1", recorder.port, sql_path)
                out_path.write_text(transcript)
                print(f"  wrote {out_path}")
            finally:
                recorder.stop()

            # A second, independently fresh server for the verification
            # pass: the runner must see the same empty starting state the
            # recording pass saw, not the tables the recording pass left
            # behind.
            verifier = fresh_server(kind, work / f"{name}-verify", bin_dir, bins)
            try:
                print(f"verifying the runner reproduces {name}.out from {label} ...")
                faithful = verify_runner_fidelity(pgregress_bin, verifier.dsn(), sql_path, results_dir)
                if faithful:
                    print(f"  runner is faithful for {name}")
                else:
                    ok = False
                    print(
                        f"  RUNNER GAP: pgregress does not reproduce {name}.out from {label} byte "
                        f"for byte; see the diff above. The .out is still recorded from the real "
                        f"client, unchanged; the runner needs a fix, not a different recording."
                    )
            finally:
                verifier.stop()

    return 0 if ok else 1


def resolve_run_paths(names: list[str]) -> list[str]:
    if not names:
        return [str(HEAD_DIR)]
    paths = []
    for name in names:
        path = HEAD_DIR / f"{name}.sql"
        if not path.is_file():
            sys.exit(f"error: no such test: {name}")
        paths.append(str(path))
    return paths


def cmd_run(names: list[str]) -> int:
    paths = resolve_run_paths(names)
    bins = build_binaries()

    port = pgrun.free_port()
    with tempfile.TemporaryDirectory(prefix="pghead-run-") as tmp:
        db_file = Path(tmp) / "regression.db"
        server = subprocess.Popen(
            [str(bins / "pg-head-server"), f"127.0.0.1:{port}", str(db_file)]
        )
        try:
            pgrun.wait_for_server(server, port)
            result = subprocess.run(
                [str(bins / "pgregress"), "--dsn", f"postgres://postgres@127.0.0.1:{port}/regression"]
                + paths,
                cwd=REPO_ROOT,
            )
            return result.returncode
        finally:
            pgrun.stop(server)


def main() -> int:
    if len(sys.argv) < 2 or sys.argv[1] not in ("record", "run"):
        sys.exit(__doc__)
    command, names = sys.argv[1], sys.argv[2:]
    return cmd_record(names) if command == "record" else cmd_run(names)


if __name__ == "__main__":
    sys.exit(main())
