#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use turso_core::MemoryIO;
use turso_pg_head::Head;
use turso_pg_head_wire::serve;

mod common;

use common::pgwire::{BackendEvent, ConnParams, Error, PgConn};

fn server() -> u16 {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = format!("wire-test-{}.db", NEXT.fetch_add(1, Ordering::Relaxed));
    let head = Arc::new(Head::open(Arc::new(MemoryIO::new()), &path).expect("the head opens"));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
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

fn connect(port: u16, user: &str) -> Result<PgConn, Error> {
    let params = ConnParams::parse(&format!("postgres://{user}@127.0.0.1:{port}/postgres"));
    PgConn::connect(&params)
}

fn query(conn: &mut PgConn, sql: &str) -> Vec<BackendEvent> {
    conn.simple_query(sql).expect(sql)
}

fn tags(events: &[BackendEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            BackendEvent::CommandComplete(tag) => Some(tag.clone()),
            BackendEvent::RowDescription(_)
            | BackendEvent::DataRow(_)
            | BackendEvent::ErrorResponse(_)
            | BackendEvent::ReadyForQuery(_)
            | BackendEvent::Other(_) => None,
        })
        .collect()
}

fn status(events: &[BackendEvent]) -> u8 {
    events
        .iter()
        .find_map(|event| match event {
            BackendEvent::ReadyForQuery(status) => Some(*status),
            BackendEvent::RowDescription(_)
            | BackendEvent::DataRow(_)
            | BackendEvent::CommandComplete(_)
            | BackendEvent::ErrorResponse(_)
            | BackendEvent::Other(_) => None,
        })
        .expect("every query ends with ReadyForQuery")
}

fn error_code(events: &[BackendEvent]) -> Option<String> {
    events.iter().find_map(|event| match event {
        BackendEvent::ErrorResponse(fields) => fields.get(&b'C').cloned(),
        BackendEvent::RowDescription(_)
        | BackendEvent::DataRow(_)
        | BackendEvent::CommandComplete(_)
        | BackendEvent::ReadyForQuery(_)
        | BackendEvent::Other(_) => None,
    })
}

#[test]
fn a_client_creates_inserts_and_reads_rows_over_the_wire() {
    let mut conn = connect(server(), "postgres").expect("postgres can log in");
    assert_eq!(
        tags(&query(
            &mut conn,
            "CREATE TABLE notes (id integer PRIMARY KEY, body text)"
        )),
        vec!["CREATE TABLE"]
    );
    assert_eq!(
        tags(&query(
            &mut conn,
            "INSERT INTO notes VALUES (1, 'a'), (2, NULL)"
        )),
        vec!["INSERT 0 2"]
    );
    let events = query(&mut conn, "SELECT id, body FROM notes");
    let columns = events
        .iter()
        .find_map(|event| match event {
            BackendEvent::RowDescription(columns) => Some(
                columns
                    .iter()
                    .map(|column| (column.name.clone(), column.type_oid))
                    .collect::<Vec<_>>(),
            ),
            BackendEvent::DataRow(_)
            | BackendEvent::CommandComplete(_)
            | BackendEvent::ErrorResponse(_)
            | BackendEvent::ReadyForQuery(_)
            | BackendEvent::Other(_) => None,
        })
        .expect("a row description");
    assert_eq!(
        columns,
        vec![("id".to_string(), 23), ("body".to_string(), 25)]
    );
    let rows = events
        .iter()
        .filter_map(|event| match event {
            BackendEvent::DataRow(row) => Some(row.clone()),
            BackendEvent::RowDescription(_)
            | BackendEvent::CommandComplete(_)
            | BackendEvent::ErrorResponse(_)
            | BackendEvent::ReadyForQuery(_)
            | BackendEvent::Other(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        vec![
            vec![Some("1".to_string()), Some("a".to_string())],
            vec![Some("2".to_string()), None]
        ]
    );
    assert_eq!(tags(&events), vec!["SELECT 2"]);
}

#[test]
fn errors_carry_their_sqlstate_and_the_transaction_status_follows_postgres() {
    let mut conn = connect(server(), "postgres").expect("postgres can log in");
    let events = query(&mut conn, "SELECT id FROM nosuch");
    assert_eq!(error_code(&events).as_deref(), Some("42P01"));
    assert_eq!(status(&events), b'I');
    assert_eq!(status(&query(&mut conn, "BEGIN")), b'T');
    let events = query(&mut conn, "SELECT id FROM nosuch");
    assert_eq!(status(&events), b'E');
    let events = query(&mut conn, "SELECT 1 FROM nosuch");
    assert_eq!(error_code(&events).as_deref(), Some("25P02"));
    let events = query(&mut conn, "ROLLBACK");
    assert_eq!(tags(&events), vec!["ROLLBACK"]);
    assert_eq!(status(&events), b'I');
}

#[test]
fn a_role_that_cannot_log_in_is_refused_at_startup() {
    let port = server();
    let mut postgres = connect(port, "postgres").expect("postgres can log in");
    query(&mut postgres, "CREATE ROLE carol");
    query(&mut postgres, "CREATE ROLE dave LOGIN");
    let refused = connect(port, "carol").err().map(|error| error.to_string());
    assert!(
        refused
            .as_deref()
            .is_some_and(|message| message.contains("role \"carol\" is not permitted to log in")),
        "{refused:?}"
    );
    assert!(connect(port, "dave").is_ok());
}

fn read_raw_message(stream: &mut std::net::TcpStream) -> Option<(u8, Vec<u8>)> {
    use std::io::Read;
    let mut tag = [0u8; 1];
    match stream.read_exact(&mut tag) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return None,
        Err(error) => panic!("failed to read a message tag: {error}"),
    }
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).expect("a message length");
    let length =
        usize::try_from(u32::from_be_bytes(length)).expect("a message length fits in usize");
    let mut body = vec![0u8; length - 4];
    stream.read_exact(&mut body).expect("a message body");
    Some((tag[0], body))
}

#[test]
fn a_fatal_startup_error_closes_the_socket_without_a_spurious_ready_for_query() {
    use std::io::Write;
    let port = server();
    let mut postgres = connect(port, "postgres").expect("postgres can log in");
    query(&mut postgres, "CREATE ROLE dave NOLOGIN");
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("a raw connection");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("a read timeout can be set");
    let mut startup = Vec::new();
    startup.extend_from_slice(&196608u32.to_be_bytes());
    for (key, value) in [("user", "dave"), ("database", "postgres")] {
        startup.extend_from_slice(key.as_bytes());
        startup.push(0);
        startup.extend_from_slice(value.as_bytes());
        startup.push(0);
    }
    startup.push(0);
    stream
        .write_all(
            &(u32::try_from(startup.len()).expect("the startup packet fits in u32") + 4)
                .to_be_bytes(),
        )
        .expect("the startup length can be written");
    stream
        .write_all(&startup)
        .expect("the startup body can be written");
    stream.flush().expect("the startup packet can be flushed");
    let error = loop {
        match read_raw_message(&mut stream).expect("the server sends an ErrorResponse") {
            (b'E', body) => break body,
            _ => continue,
        }
    };
    assert!(String::from_utf8_lossy(&error).contains("dave"));
    assert_eq!(
        read_raw_message(&mut stream),
        None,
        "PostgreSQL closes the socket right after a FATAL startup error, with no ReadyForQuery"
    );
}
