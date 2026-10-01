#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use std::collections::HashMap;
use std::fmt;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::TcpStream;

pub type ErrorFields = HashMap<u8, String>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Protocol(String),
    Rejected(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "I/O error: {error}"),
            Error::Protocol(message) => write!(f, "protocol error: {message}"),
            Error::Rejected(message) => write!(f, "connection rejected: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Error {
        Error::Io(error)
    }
}

impl From<std::num::TryFromIntError> for Error {
    fn from(error: std::num::TryFromIntError) -> Error {
        Error::Protocol(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn error_message(fields: &ErrorFields) -> &str {
    fields.get(&b'M').map(String::as_str).unwrap_or("unknown")
}

#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub type_oid: u32,
}

#[derive(Debug)]
pub enum BackendEvent {
    RowDescription(Vec<Column>),
    DataRow(Vec<Option<String>>),
    CommandComplete(String),
    ErrorResponse(ErrorFields),
    ReadyForQuery(u8),
    Other(u8),
}

pub struct ConnParams {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
}

impl ConnParams {
    pub fn parse(dsn: &str) -> ConnParams {
        let rest = dsn
            .strip_prefix("postgres://")
            .unwrap_or_else(|| panic!("DSN must start with postgres://, got {dsn}"));
        let (user, rest) = rest
            .split_once('@')
            .unwrap_or_else(|| panic!("DSN must name a user, got {dsn}"));
        let (hostport, database) = rest
            .split_once('/')
            .unwrap_or_else(|| panic!("DSN must name a database, got {dsn}"));
        let (host, port) = hostport
            .split_once(':')
            .unwrap_or_else(|| panic!("DSN must name a port, got {dsn}"));
        ConnParams {
            host: host.to_string(),
            port: port
                .parse()
                .unwrap_or_else(|_| panic!("bad port in DSN: {port}")),
            user: user.to_string(),
            database: database.to_string(),
        }
    }
}

pub struct PgConn {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

impl PgConn {
    pub fn connect(params: &ConnParams) -> Result<PgConn> {
        let stream = TcpStream::connect((params.host.as_str(), params.port))?;
        let reader = BufReader::new(stream.try_clone()?);
        let writer = BufWriter::new(stream);
        let mut conn = PgConn { reader, writer };

        let mut startup = Vec::new();
        startup.extend_from_slice(&196608u32.to_be_bytes());
        for (k, v) in [
            ("user", params.user.as_str()),
            ("database", params.database.as_str()),
        ] {
            startup.extend_from_slice(k.as_bytes());
            startup.push(0);
            startup.extend_from_slice(v.as_bytes());
            startup.push(0);
        }
        startup.push(0);
        conn.writer
            .write_all(&((u32::try_from(startup.len())? + 4).to_be_bytes()))?;
        conn.writer.write_all(&startup)?;
        conn.writer.flush()?;

        loop {
            let (tag, body) = conn.read_message()?;
            match tag {
                b'R' | b'S' | b'K' | b'N' => {}
                b'Z' => return Ok(conn),
                b'E' => {
                    let fields = parse_error_fields(&body)?;
                    return Err(Error::Rejected(error_message(&fields).to_string()));
                }
                other => {
                    return Err(Error::Protocol(format!(
                        "unexpected message {:?} during startup",
                        char::from(other)
                    )));
                }
            }
        }
    }

    pub fn send_query(&mut self, sql: &str) -> Result<()> {
        let mut body = Vec::from(sql.as_bytes());
        body.push(0);
        self.write_message(b'Q', &body)
    }

    pub fn read_message(&mut self) -> Result<(u8, Vec<u8>)> {
        let mut header = [0u8; 5];
        self.reader.read_exact(&mut header).map_err(|e| {
            Error::Protocol(format!(
                "reading message header (server closed the connection?): {e}"
            ))
        })?;
        let len = usize::try_from(u32::from_be_bytes([
            header[1], header[2], header[3], header[4],
        ]))?;
        if len < 4 {
            return Err(Error::Protocol(format!("invalid message length {len}")));
        }
        let mut body = vec![0u8; len - 4];
        self.reader
            .read_exact(&mut body)
            .map_err(|e| Error::Protocol(format!("reading message body: {e}")))?;
        Ok((header[0], body))
    }

    pub fn read_event(&mut self) -> Result<BackendEvent> {
        let (tag, body) = self.read_message()?;
        Ok(match tag {
            b'T' => BackendEvent::RowDescription(parse_row_description(&body)?),
            b'D' => BackendEvent::DataRow(parse_data_row(&body)?),
            b'C' => {
                let mut r = Reader::new(&body);
                BackendEvent::CommandComplete(r.cstring()?)
            }
            b'E' => BackendEvent::ErrorResponse(parse_error_fields(&body)?),
            b'Z' => {
                let mut r = Reader::new(&body);
                BackendEvent::ReadyForQuery(r.u8()?)
            }
            other => BackendEvent::Other(other),
        })
    }

    pub fn simple_query(&mut self, sql: &str) -> Result<Vec<BackendEvent>> {
        self.send_query(sql)?;
        let mut events = Vec::new();
        loop {
            let event = self.read_event()?;
            let done = matches!(event, BackendEvent::ReadyForQuery(_));
            events.push(event);
            if done {
                return Ok(events);
            }
        }
    }

    fn write_message(&mut self, tag: u8, body: &[u8]) -> Result<()> {
        self.writer.write_all(&[tag])?;
        self.writer
            .write_all(&((u32::try_from(body.len())? + 4).to_be_bytes()))?;
        self.writer.write_all(body)?;
        self.writer.flush()?;
        Ok(())
    }
}

fn parse_row_description(body: &[u8]) -> Result<Vec<Column>> {
    let mut r = Reader::new(body);
    let nfields = usize::from(r.u16()?);
    let mut columns = Vec::with_capacity(nfields);
    for _ in 0..nfields {
        let name = r.cstring()?;
        r.skip(4 + 2)?;
        let type_oid = r.u32()?;
        r.skip(2 + 4 + 2)?;
        columns.push(Column { name, type_oid });
    }
    Ok(columns)
}

fn parse_data_row(body: &[u8]) -> Result<Vec<Option<String>>> {
    let mut r = Reader::new(body);
    let ncols = usize::from(r.u16()?);
    let mut row = Vec::with_capacity(ncols);
    for _ in 0..ncols {
        let len = r.i32()?;
        if len < 0 {
            row.push(None);
        } else {
            let bytes = r.bytes(usize::try_from(len)?)?;
            row.push(Some(String::from_utf8_lossy(bytes).into_owned()));
        }
    }
    Ok(row)
}

fn parse_error_fields(body: &[u8]) -> Result<ErrorFields> {
    let mut r = Reader::new(body);
    let mut fields = HashMap::new();
    loop {
        let code = r.u8()?;
        if code == 0 {
            return Ok(fields);
        }
        fields.insert(code, r.cstring()?);
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0 }
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let end = end.ok_or_else(|| Error::Protocol("truncated message".to_string()))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn cstring(&mut self) -> Result<String> {
        let start = self.pos;
        let nul = self.buf[start..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::Protocol("unterminated string in message".to_string()))?;
        let s = String::from_utf8_lossy(&self.buf[start..start + nul]).into_owned();
        self.pos = start + nul + 1;
        Ok(s)
    }
}
