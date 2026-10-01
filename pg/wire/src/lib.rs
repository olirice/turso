use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::{stream, Sink, SinkExt};
use pgwire::api::auth::{
    finish_authentication, protocol_negotiation, save_startup_parameters_to_metadata,
    ServerParameterProvider, StartupHandler,
};
use pgwire::api::query::SimpleQueryHandler;
use pgwire::api::results::{DataRowEncoder, FieldFormat, FieldInfo, QueryResponse, Response, Tag};
use pgwire::api::{ClientInfo, PgWireServerHandlers, Type};
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};
use pgwire::messages::{PgWireBackendMessage, PgWireFrontendMessage};
use pgwire::tokio::process_socket;
use pgwire::types::format::FormatOptions;
use tokio::net::TcpListener;
use turso_pg_head::{CommandTag, Head, HeadError, Outcome, OutputColumn, Session, SERVER_VERSION};

pub async fn serve(head: Arc<Head>, listener: TcpListener) -> std::io::Result<()> {
    loop {
        let (socket, _) = listener.accept().await?;
        let handlers = Handlers {
            connection: Arc::new(Connection {
                head: head.clone(),
                session: Mutex::new(None),
            }),
        };
        tokio::spawn(async move {
            if let Err(error) = process_socket(socket, None, handlers).await {
                tracing::warn!(%error, "a head wire connection ended with an error");
            }
        });
    }
}

struct Handlers {
    connection: Arc<Connection>,
}

struct Connection {
    head: Arc<Head>,
    session: Mutex<Option<Session>>,
}

struct Parameters {
    user: String,
    superuser: bool,
}

impl PgWireServerHandlers for Handlers {
    fn simple_query_handler(&self) -> Arc<impl SimpleQueryHandler> {
        self.connection.clone()
    }

    fn startup_handler(&self) -> Arc<impl StartupHandler> {
        self.connection.clone()
    }
}

#[async_trait]
impl StartupHandler for Connection {
    async fn on_startup<C>(
        &self,
        client: &mut C,
        message: PgWireFrontendMessage,
    ) -> PgWireResult<()>
    where
        C: ClientInfo + Sink<PgWireBackendMessage> + Unpin + Send,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        let PgWireFrontendMessage::Startup(startup) = message else {
            return Ok(());
        };
        protocol_negotiation(client, &startup).await?;
        save_startup_parameters_to_metadata(client, &startup);
        let user = startup.parameters.get("user").cloned().unwrap_or_default();
        let session = match self.head.connect(&user) {
            Ok(session) => session,
            Err(error) => {
                client
                    .send(PgWireBackendMessage::ErrorResponse(
                        error_info("FATAL", &error).into(),
                    ))
                    .await?;
                client.close().await?;
                return Ok(());
            }
        };
        let parameters = Parameters {
            user,
            superuser: session.is_superuser(),
        };
        *self
            .session
            .lock()
            .map_err(|_| PgWireError::ApiError("session lock poisoned".into()))? = Some(session);
        finish_authentication(client, &parameters).await
    }
}

impl ServerParameterProvider for Parameters {
    fn server_parameters<C>(&self, client: &C) -> Option<HashMap<String, String>>
    where
        C: ClientInfo,
    {
        let application_name = client
            .metadata()
            .get("application_name")
            .cloned()
            .unwrap_or_default();
        let on_off = |value: bool| if value { "on" } else { "off" }.to_string();
        Some(HashMap::from([
            ("server_version".to_string(), SERVER_VERSION.to_string()),
            ("server_encoding".to_string(), "UTF8".to_string()),
            ("client_encoding".to_string(), "UTF8".to_string()),
            ("DateStyle".to_string(), "ISO, MDY".to_string()),
            ("IntervalStyle".to_string(), "postgres".to_string()),
            ("TimeZone".to_string(), "UTC".to_string()),
            ("integer_datetimes".to_string(), "on".to_string()),
            ("standard_conforming_strings".to_string(), "on".to_string()),
            ("in_hot_standby".to_string(), "off".to_string()),
            (
                "default_transaction_read_only".to_string(),
                "off".to_string(),
            ),
            ("search_path".to_string(), "\"$user\", public".to_string()),
            ("is_superuser".to_string(), on_off(self.superuser)),
            ("session_authorization".to_string(), self.user.clone()),
            ("application_name".to_string(), application_name),
        ]))
    }
}

#[async_trait]
impl SimpleQueryHandler for Connection {
    async fn do_query<C>(&self, _client: &mut C, query: &str) -> PgWireResult<Vec<Response>>
    where
        C: ClientInfo + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        let guard = self
            .session
            .lock()
            .map_err(|_| PgWireError::ApiError("session lock poisoned".into()))?;
        let session = guard
            .as_ref()
            .ok_or_else(|| PgWireError::ApiError("a query arrived before startup".into()))?;
        let outcome = session
            .execute(query)
            .map_err(|error| error_response(&error))?;
        Ok(vec![response(outcome)?])
    }
}

fn response(outcome: Outcome) -> PgWireResult<Response> {
    match outcome {
        Outcome::Command(tag) => Ok(match tag {
            CommandTag::Begin => Response::TransactionStart(Tag::new(tag.name())),
            CommandTag::Commit | CommandTag::Rollback => {
                Response::TransactionEnd(Tag::new(tag.name()))
            }
            CommandTag::CreateTable
            | CommandTag::Insert
            | CommandTag::Select
            | CommandTag::CreateRole
            | CommandTag::Grant
            | CommandTag::CreatePolicy
            | CommandTag::AlterTable
            | CommandTag::Lock
            | CommandTag::Set
            | CommandTag::Prepare => Response::Execution(Tag::new(tag.name())),
        }),
        Outcome::Inserted(rows) => Ok(Response::Execution(
            Tag::new(CommandTag::Insert.name())
                .with_oid(0)
                .with_rows(rows),
        )),
        Outcome::Rows { columns, values } => {
            let fields: Result<Vec<_>, _> = columns.iter().map(field).collect();
            let fields = Arc::new(fields?);
            let rows = values
                .iter()
                .map(|row| {
                    let mut encoder = DataRowEncoder::new(fields.clone());
                    for value in row {
                        encoder.encode_field_with_type_and_format(
                            &turso_pg_head::wire_text(value),
                            &Type::TEXT,
                            FieldFormat::Text,
                            &FormatOptions::default(),
                        )?;
                    }
                    encoder.finish()
                })
                .collect::<Vec<_>>();
            Ok(Response::Query(QueryResponse::new(
                fields,
                stream::iter(rows),
            )))
        }
    }
}

fn field(column: &OutputColumn) -> Result<FieldInfo, PgWireError> {
    let datatype = u32::try_from(column.type_oid)
        .ok()
        .and_then(Type::from_oid)
        .ok_or_else(|| {
            PgWireError::ApiError("the head only reports type oids pgwire knows".into())
        })?;
    Ok(FieldInfo::new(
        column.name.clone(),
        None,
        None,
        datatype,
        FieldFormat::Text,
    ))
}

fn error_response(error: &HeadError) -> PgWireError {
    PgWireError::UserError(Box::new(error_info("ERROR", error)))
}

fn error_info(severity: &str, error: &HeadError) -> ErrorInfo {
    let mut info = ErrorInfo::new(
        severity.to_string(),
        error.state.code().to_string(),
        error.message.clone(),
    );
    info.detail.clone_from(&error.detail);
    info.hint.clone_from(&error.hint);
    info.position = error.position.map(|position| position.to_string());
    info
}
