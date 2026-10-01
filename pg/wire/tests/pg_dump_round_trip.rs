#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use turso_core::MemoryIO;
use turso_pg_head::Head;
use turso_pg_head_wire::serve;

mod common;

const CAPTURED_SCHEMA: &[(&str, &str)] = &[
    ("postgres", "CREATE ROLE alice LOGIN"),
    ("postgres", "CREATE ROLE bob"),
    ("postgres", "GRANT CREATE ON SCHEMA public TO alice"),
    (
        "alice",
        "CREATE TABLE notes (id integer PRIMARY KEY, owner text NOT NULL, body text, views bigint)",
    ),
    (
        "alice",
        "CREATE TABLE \"Mixed Case\" (\"Key\" bigint PRIMARY KEY, \"a.b\" text)",
    ),
    ("alice", "CREATE TABLE plain (n integer)"),
    ("alice", "INSERT INTO notes VALUES (1, 'alice', 'hi', 3)"),
    ("alice", "GRANT SELECT, INSERT ON notes TO bob"),
    ("alice", "GRANT SELECT ON \"Mixed Case\" TO bob, alice"),
    ("alice", "ALTER TABLE notes ENABLE ROW LEVEL SECURITY"),
    ("alice", "ALTER TABLE \"Mixed Case\" ENABLE ROW LEVEL SECURITY"),
    ("alice", "ALTER TABLE \"Mixed Case\" FORCE ROW LEVEL SECURITY"),
    (
        "alice",
        "CREATE POLICY own ON notes FOR SELECT TO bob USING (owner = current_user)",
    ),
    (
        "alice",
        "CREATE POLICY pair ON notes FOR SELECT TO bob, alice USING (id = 1 AND body = 'x' OR views = 3)",
    ),
    (
        "alice",
        "CREATE POLICY everyone ON \"Mixed Case\" FOR SELECT USING (\"Key\" = 1)",
    ),
];

#[test]
fn a_schema_dumps_from_the_head_exactly_as_from_postgres() {
    let Some(postgres) = Postgres::start() else {
        return;
    };
    let roles = ["alice", "bob"];
    assert_eq!(
        dump_from_head(&postgres, CAPTURED_SCHEMA),
        postgres.dump_of(CAPTURED_SCHEMA, &roles)
    );
}

fn dump_from_head(postgres: &Postgres, statements: &[(&str, &str)]) -> String {
    let path = format!("dump-test-{}.db", NEXT.fetch_add(1, Ordering::Relaxed));
    let head = Arc::new(Head::open(Arc::new(MemoryIO::new()), &path).expect("the head opens"));
    for (role, sql) in statements {
        let session = head.connect(role).expect("the role can log in");
        if let Err(error) = session.execute(sql) {
            panic!("the head refused {sql}: {error:?}");
        }
    }
    postgres.pg_dump(spawn_server(head), "postgres")
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn spawn_server(head: Arc<Head>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("a bound address").port();
    listener
        .set_nonblocking(true)
        .expect("the listener can be non-blocking");
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .expect("a runtime starts")
            .block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(listener).expect("tokio adopts the listener");
                serve(head, listener).await
            })
    });
    port
}

struct Postgres {
    bin: PathBuf,
    data: tempfile::TempDir,
    port: u16,
}

impl Postgres {
    fn start() -> Option<Postgres> {
        let bin = common::require_postgres(
            "the pg_dump round trip",
            &["initdb", "pg_ctl", "psql", "pg_dump"],
        )?;
        let data = tempfile::tempdir().expect("a scratch directory");
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("a free port")
            .local_addr()
            .expect("a bound address")
            .port();
        run(Command::new(bin.join("initdb"))
            .args(["-U", "postgres", "--auth=trust", "-D"])
            .arg(data.path().join("cluster")));
        run(Command::new(bin.join("pg_ctl"))
            .arg("-D")
            .arg(data.path().join("cluster"))
            .arg("-l")
            .arg(data.path().join("log"))
            .args([
                "-o",
                &format!("-p {port} -h 127.0.0.1 -k /tmp"),
                "-w",
                "start",
            ]));
        Some(Postgres { bin, data, port })
    }

    fn dump_of(&self, statements: &[(&str, &str)], roles: &[&str]) -> String {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let database = format!("case_{}", NEXT.fetch_add(1, Ordering::Relaxed));
        self.psql("postgres", &format!("CREATE DATABASE {database}"));
        let script = statements
            .iter()
            .map(|(role, sql)| match *role {
                "postgres" => format!("{sql};\n"),
                role => format!("SET ROLE {role};\n{sql};\nRESET ROLE;\n"),
            })
            .collect::<String>();
        self.psql(&database, &script);
        let dump = self.pg_dump(self.port, &database);
        self.psql("postgres", &format!("DROP DATABASE {database}"));
        for role in roles {
            self.psql("postgres", &format!("DROP ROLE IF EXISTS {role}"));
        }
        dump
    }

    fn psql(&self, database: &str, script: &str) {
        run(Command::new(self.bin.join("psql"))
            .args([
                "-X",
                "-q",
                "-v",
                "ON_ERROR_STOP=1",
                "-h",
                "127.0.0.1",
                "-U",
                "postgres",
            ])
            .args(["-p", &self.port.to_string(), "-d", database, "-c", script]));
    }

    fn pg_dump(&self, port: u16, database: &str) -> String {
        let output = Command::new(self.bin.join("pg_dump"))
            .args(["--schema-only", "-h", "127.0.0.1", "-U", "postgres"])
            .args(["-p", &port.to_string(), database])
            .arg(format!("--restrict-key={RESTRICT_KEY}"))
            .output()
            .expect("pg_dump runs");
        assert!(
            output.status.success(),
            "pg_dump failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("pg_dump writes UTF-8")
    }
}

impl Drop for Postgres {
    fn drop(&mut self) {
        let _ = Command::new(self.bin.join("pg_ctl"))
            .arg("-D")
            .arg(self.data.path().join("cluster"))
            .args(["-m", "immediate", "stop"])
            .output();
    }
}

/// Fixed on both sides (the head's own `pg_dump` and PostgreSQL's) so the
/// `\restrict`/`\unrestrict` lines pg_dump 17.6+/18 emit with an otherwise
/// random key compare byte-for-byte instead of needing to be filtered out.
const RESTRICT_KEY: &str = "tursopgheadpgdumproundtriptestrestrictkey0";

fn run(command: &mut Command) {
    let output = command.output().expect("the PostgreSQL tool runs");
    assert!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
