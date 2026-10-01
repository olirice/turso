use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;

#[path = "build/generate.rs"]
mod generate;
#[path = "build/sqlstate.rs"]
mod sqlstate;

/// The PostgreSQL major version this head targets. `capture/out/version.txt`
/// (the server the capture ran against) must match, or the build fails: a
/// stale capture from a different major version silently teaches the head
/// the wrong catalog shape.
const POSTGRES_MAJOR: u32 = 18;

const VERSION_FILE: &str = "capture/out/version.txt";

thread_local! {
    /// Every capture file a generator actually opened this run, recorded
    /// at the one place each of them reads a file (`generate::read_json`,
    /// `sqlstate::generate`) via `record_read` below, rather than a second,
    /// hand-maintained file list here that could name a different set of
    /// files than the generators actually read (the bug a stale
    /// `cargo::rerun-if-changed` list risks: an edited capture file cargo
    /// was never told to watch, so a rebuild keeps serving the old
    /// generated catalog).
    static READ_FILES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn record_read(path: &std::path::Path) {
    READ_FILES.with(|files| files.borrow_mut().push(path.display().to_string()));
}

fn recorded_reads() -> Vec<String> {
    READ_FILES.with(|files| files.borrow().clone())
}

fn main() -> Result<(), String> {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").map_err(|_| "cargo sets CARGO_MANIFEST_DIR".to_string())?,
    );

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build/generate.rs");
    println!("cargo::rerun-if-changed=build/sqlstate.rs");
    println!(
        "cargo::rerun-if-changed={}",
        manifest_dir.join(VERSION_FILE).display()
    );

    check_captured_major(&manifest_dir)?;

    let generated = generate::generate();
    let sqlstate_generated = sqlstate::generate(&manifest_dir.join("capture/out/errcodes.txt"));

    // Printed for whatever was read even if a generator failed partway
    // through: a failing read already fails the build with its own error
    // below, and cargo watching the files read before the failure is
    // still strictly better than watching none of them.
    for path in recorded_reads() {
        println!("cargo::rerun-if-changed={path}");
    }

    let generated = generated?;
    let sqlstate_generated = sqlstate_generated?;

    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|_| "cargo sets OUT_DIR".to_string())?);
    let generated_path = out_dir.join("catalog_generated.rs");
    fs::write(&generated_path, generated)
        .map_err(|error| format!("writing {}: {error}", generated_path.display()))?;

    let sqlstate_path = out_dir.join("sqlstate_generated.rs");
    fs::write(&sqlstate_path, sqlstate_generated)
        .map_err(|error| format!("writing {}: {error}", sqlstate_path.display()))?;
    Ok(())
}

/// Fails the build if `capture/out/version.txt` was not captured from a
/// PostgreSQL `POSTGRES_MAJOR`, and hands the crate the captured version
/// (`18.6`) as `PG_HEAD_SERVER_VERSION`, the version the head reports.
fn check_captured_major(manifest_dir: &std::path::Path) -> Result<(), String> {
    let version_path = manifest_dir.join(VERSION_FILE);
    let version_text = fs::read_to_string(&version_path)
        .map_err(|error| format!("reading {}: {error}", version_path.display()))?;
    let version = version_text
        .strip_prefix("PostgreSQL ")
        .and_then(|rest| rest.split(' ').next())
        .ok_or_else(|| format!("no PostgreSQL version in {version_path:?}: {version_text:?}"))?;
    let major = version
        .split('.')
        .next()
        .and_then(|digits| digits.parse::<u32>().ok())
        .ok_or_else(|| {
            format!(
                "could not find a PostgreSQL major version in {version_path:?}: {version_text:?}"
            )
        })?;
    if major != POSTGRES_MAJOR {
        return Err(format!(
            "capture/out/version.txt was captured from PostgreSQL {major}, but this head targets \
             PostgreSQL {POSTGRES_MAJOR}; re-run capture/capture.sh against a PostgreSQL \
             {POSTGRES_MAJOR} server"
        ));
    }
    println!("cargo::rustc-env=PG_HEAD_SERVER_VERSION={version}");
    Ok(())
}
