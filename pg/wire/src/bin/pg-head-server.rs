// Copyright 2023-2026 the Turso authors. All rights reserved. MIT license.

//! A tiny PostgreSQL wire-protocol server in front of `turso_pg_head`, for
//! the regress runner (`postgres/regress/`) to point at. Two positional
//! arguments, nothing else: the listen address and the database file path.

use std::sync::Arc;

use turso_core::PlatformIO;
use turso_pg_head::Head;
use turso_pg_head_wire::serve;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(listen), Some(db_path)) = (args.next(), args.next()) else {
        anyhow::bail!("usage: pg-head-server <listen-address> <db-path>");
    };
    let io = Arc::new(PlatformIO::new()?);
    let head = Arc::new(Head::open(io, &db_path)?);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    serve(head, listener).await?;
    Ok(())
}
