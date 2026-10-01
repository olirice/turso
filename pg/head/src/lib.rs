mod analyze;
mod catalog;
mod engine;
mod engine_hooks;
mod error;
mod head;
mod ident;
mod lower;
mod parse;
mod pipeline;
mod render;
mod security;
mod session;

use turso_core::Value;

pub use error::{HeadError, SqlState};

/// The PostgreSQL version the head reports (`server_version`): the exact
/// server its catalog was captured from (`capture/out/version.txt`).
pub const SERVER_VERSION: &str = env!("PG_HEAD_SERVER_VERSION");
pub use head::Head;
pub use parse::statement::CommandTag;
pub use render::wire_text;
pub use session::Session;

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Command(CommandTag),
    /// `INSERT`'s completion carries its row count.
    Inserted(usize),
    Rows {
        columns: Vec<OutputColumn>,
        values: Vec<Vec<Value>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutputColumn {
    pub name: String,
    pub type_oid: i64,
}
