#![allow(dead_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub mod pgwire;

const OPT_OUT_VAR: &str = "PG_TESTS";
const OPT_OUT_VALUE: &str = "skip";

/// The server `pg/head/capture` was captured from, the same build the
/// corpus transcripts were recorded with (`PostgreSQL 18.6 on ...`).
const CAPTURED: &str = include_str!("../../../head/capture/out/version.txt");

/// How to get the pinned build: the flake pins it apart from the toolchain.
const PROVIDE: &str = "`nix build .#postgres` (then PG_BIN=result/bin) or `nix develop`";

/// `18.6` from `PostgreSQL 18.6 on ...`.
fn captured_version() -> &'static str {
    CAPTURED
        .split_whitespace()
        .nth(1)
        .expect("capture/out/version.txt starts with `PostgreSQL <version>`")
}

/// The pg_dump round trips compare against output recorded from one
/// PostgreSQL build, and pg_dump's output changes between minor releases,
/// so only that exact version is accepted.
pub fn require_postgres(test_name: &str, tools: &[&str]) -> Option<PathBuf> {
    let wanted = captured_version();
    let found = postgres_bins(tools);
    if let Some((bin, _)) = found.iter().find(|(_, version)| version == wanted) {
        return Some(bin.clone());
    }
    let seen = found
        .iter()
        .map(|(bin, version)| format!("{version} in {}", bin.display()))
        .collect::<Vec<_>>()
        .join(", ");
    let problem = if seen.is_empty() {
        format!(
            "PostgreSQL {wanted}'s server and {} were not found together on PATH or in $PG_BIN",
            tools.join(", ")
        )
    } else {
        format!("it needs PostgreSQL {wanted} exactly, but found {seen}")
    };
    if env::var(OPT_OUT_VAR).as_deref() == Ok(OPT_OUT_VALUE) {
        eprintln!("SKIP: {test_name}: {problem}; skipped because {OPT_OUT_VAR}={OPT_OUT_VALUE}");
        return None;
    }
    panic!(
        "{test_name}: {problem}. {PROVIDE} provides it; set {OPT_OUT_VAR}={OPT_OUT_VALUE} to skip this test instead of failing"
    );
}

/// Every directory holding all of `tools` plus the `postgres` server
/// (client-only installs such as Homebrew's libpq ship `initdb` and
/// `pg_dump` without it), with its version: the server's and pg_dump's
/// when they agree, since the dump compares both.
fn postgres_bins(tools: &[&str]) -> Vec<(PathBuf, String)> {
    let path = env::var_os("PATH").unwrap_or_default();
    env::var_os("PG_BIN")
        .map(PathBuf::from)
        .into_iter()
        .chain(env::split_paths(&path))
        .filter(|dir| {
            tools
                .iter()
                .chain(["postgres", "pg_dump"].iter())
                .all(|tool| dir.join(tool).exists())
        })
        .filter_map(|dir| {
            let server = tool_version(&dir, "postgres")?;
            let dump = tool_version(&dir, "pg_dump")?;
            let version = if server == dump {
                server
            } else {
                format!("{server} (pg_dump {dump})")
            };
            Some((dir, version))
        })
        .collect()
}

/// `18.6` from `postgres (PostgreSQL) 18.6` or `pg_dump (PostgreSQL) 18.6`.
fn tool_version(dir: &Path, tool: &str) -> Option<String> {
    let output = Command::new(dir.join(tool))
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    text.split_whitespace().nth(2).map(str::to_string)
}
