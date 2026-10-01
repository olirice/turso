use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::analyze::types::TypeHandle;
use crate::catalog::{Catalog, PUBLIC_NAMESPACE};
use crate::engine::store::CatalogCache;
use crate::engine::EngineConnection;
use crate::error::{HeadError, PgError, SqlState};
use crate::ident::{PreparedName, RoleName, SchemaName};
use crate::parse;
use crate::parse::statement::{CommandTag, Query, Statement};
use crate::pipeline::{self, TransactionState};
use crate::session::settings::{KnownSetting, SettingValue};
use crate::{engine, Outcome};

pub(crate) mod session_functions;
pub(crate) mod settings;

pub struct Session {
    connection: EngineConnection,
    identity: RoleName,
    superuser: bool,
    catalog_cache: Arc<CatalogCache>,
    state: RefCell<State>,
}

pub struct Context<'a> {
    pub(crate) identity: &'a RoleName,
    pub(crate) settings: &'a Settings,
    pub(crate) in_transaction: bool,
    pub(crate) prepared: &'a BTreeMap<PreparedName, PreparedStatement>,
}

#[derive(Clone)]
pub(crate) struct Settings {
    pub(crate) search_path: Vec<SchemaName>,
    pub(crate) row_security: bool,
    known_text: BTreeMap<KnownSetting, String>,
}

#[derive(Clone)]
pub(crate) struct PreparedStatement {
    pub(crate) param_types: Vec<TypeHandle>,
    pub(crate) query: Query,
}

pub(crate) enum SessionEffect {
    Begin {
        read_only: Option<bool>,
    },
    Commit,
    Rollback,
    SetTransaction {
        read_only: Option<bool>,
    },
    Set {
        target: KnownSetting,
        value: SettingValue,
        local: bool,
    },
    Prepare {
        name: PreparedName,
        statement: Box<PreparedStatement>,
    },
}

struct State {
    settings: Settings,
    transaction: Transaction,
    prepared: BTreeMap<PreparedName, PreparedStatement>,
}

enum Transaction {
    Idle,
    Open {
        read_only: bool,
        snapshot_taken: bool,
        saved: Settings,
        base: Settings,
    },
    Failed {
        saved: Settings,
    },
}

impl Transaction {
    /// The thinner view of this transaction `pipeline::admit` gates on: its
    /// own settings restore points are the one thing that never bears on
    /// whether a statement is admitted.
    fn state(&self) -> TransactionState {
        match self {
            Transaction::Idle => TransactionState::Idle,
            Transaction::Open {
                read_only,
                snapshot_taken,
                ..
            } => TransactionState::Open {
                read_only: *read_only,
                snapshot: *snapshot_taken,
            },
            Transaction::Failed { .. } => TransactionState::Failed,
        }
    }
}

impl Session {
    pub(crate) fn new(
        connection: EngineConnection,
        identity: RoleName,
        superuser: bool,
        catalog_cache: Arc<CatalogCache>,
    ) -> Self {
        Session {
            connection,
            identity,
            superuser,
            catalog_cache,
            state: RefCell::new(State {
                settings: Settings::new(),
                transaction: Transaction::Idle,
                prepared: BTreeMap::new(),
            }),
        }
    }

    pub fn is_superuser(&self) -> bool {
        self.superuser
    }

    pub fn execute(&self, sql: &str) -> Result<Outcome, HeadError> {
        // `sql` is the only place a raised error's byte-offset location can
        // be converted into PostgreSQL's 1-based character position (the
        // wire `P` field): every stage below this one only ever sees the
        // parsed statement, never the text it came from.
        self.dispatch_admitted(parse::statement(sql))
            .map_err(|error| error.resolve_position(sql))
    }

    fn dispatch_admitted(
        &self,
        admitted: Result<Statement, HeadError>,
    ) -> Result<Outcome, HeadError> {
        let failed = matches!(self.state.borrow().transaction, Transaction::Failed { .. });
        match admitted {
            Err(error) if error.state == SqlState::SyntaxError => Err(error),
            Err(_) if failed => Err(aborted()),
            Err(error) => Err(error),
            Ok(statement) => self.statement(statement),
        }
    }

    fn statement(&self, statement: Statement) -> Result<Outcome, HeadError> {
        match statement {
            Statement::Commit => self.finish(Statement::Commit, CommandTag::Commit),
            Statement::Rollback => self.finish(Statement::Rollback, CommandTag::Rollback),
            Statement::Begin { read_only } if self.is_idle() => self.begin(read_only),
            other @ Statement::CreateTable { .. }
            | other @ Statement::Insert { .. }
            | other @ Statement::Select(_)
            | other @ Statement::CreateRole { .. }
            | other @ Statement::Grant { .. }
            | other @ Statement::CreatePolicy { .. }
            | other @ Statement::AlterRowSecurity { .. }
            | other @ Statement::Lock { .. }
            | other @ Statement::Begin { .. }
            | other @ Statement::SetTransaction { .. }
            | other @ Statement::Set(_)
            | other @ Statement::Prepare { .. }
            | other @ Statement::Execute { .. } => self.dispatch(other),
        }
    }

    fn is_idle(&self) -> bool {
        matches!(self.state.borrow().transaction, Transaction::Idle)
    }

    fn dispatch(&self, statement: Statement) -> Result<Outcome, HeadError> {
        if self.is_idle() {
            self.autocommit(statement)
        } else {
            self.in_transaction(statement)
        }
    }

    fn begin(&self, read_only: Option<bool>) -> Result<Outcome, HeadError> {
        let settings = self.state.borrow().settings.clone();
        let (outcome, effects) = self.run(
            &self.connection,
            Statement::Begin { read_only },
            &settings,
            TransactionState::Idle,
        )?;
        let requested = effects.into_iter().find_map(|effect| match effect {
            SessionEffect::Begin { read_only } => Some(read_only),
            SessionEffect::Commit
            | SessionEffect::Rollback
            | SessionEffect::SetTransaction { .. }
            | SessionEffect::Set { .. }
            | SessionEffect::Prepare { .. } => None,
        });
        engine::store::begin(&self.connection)?;
        let mut state = self.state.borrow_mut();
        let saved = state.settings.clone();
        let base = state.settings.clone();
        state.transaction = Transaction::Open {
            read_only: requested.flatten().unwrap_or(false),
            snapshot_taken: false,
            saved,
            base,
        };
        Ok(outcome)
    }

    fn autocommit(&self, statement: Statement) -> Result<Outcome, HeadError> {
        let engine_write_lock = statement.rules().engine_write_lock;
        let settings = self.state.borrow().settings.clone();
        let (outcome, effects) =
            engine::store::in_unit_of_work(&self.connection, engine_write_lock, |connection| {
                self.run(connection, statement, &settings, TransactionState::Idle)
            })?;
        let mut state = self.state.borrow_mut();
        for effect in effects {
            apply_effect(&mut state, effect);
        }
        Ok(outcome)
    }

    fn in_transaction(&self, statement: Statement) -> Result<Outcome, HeadError> {
        let takes_snapshot = statement.rules().takes_snapshot;
        let (settings, transaction_state) = {
            let state = self.state.borrow();
            (state.settings.clone(), state.transaction.state())
        };
        match self.run(&self.connection, statement, &settings, transaction_state) {
            Ok((outcome, effects)) => {
                let mut state = self.state.borrow_mut();
                if takes_snapshot {
                    if let Transaction::Open { snapshot_taken, .. } = &mut state.transaction {
                        *snapshot_taken = true;
                    }
                }
                for effect in effects {
                    apply_effect(&mut state, effect);
                }
                Ok(outcome)
            }
            Err(error) => {
                let mut state = self.state.borrow_mut();
                match saved_settings(&state.transaction) {
                    Ok(saved) => state.transaction = Transaction::Failed { saved },
                    Err(_) => {
                        return Err(HeadError::internal("transaction state invariant violated"))
                    }
                }
                Err(error)
            }
        }
    }

    fn run(
        &self,
        connection: &EngineConnection,
        statement: Statement,
        settings: &Settings,
        state: TransactionState,
    ) -> Result<(Outcome, Vec<SessionEffect>), HeadError> {
        let admitted = pipeline::admit(statement, &state)?;
        let in_transaction = !matches!(state, TransactionState::Idle);
        let catalog = engine::store::catalog(connection, &self.catalog_cache, !in_transaction)?;
        let prepared = self.state.borrow().prepared.clone();
        let context = Context {
            identity: &self.identity,
            settings,
            in_transaction,
            prepared: &prepared,
        };
        let analyzed = pipeline::analyze(admitted, &catalog, &context)?;
        let authorized = pipeline::authorize(analyzed, &catalog)?;
        let enforced = pipeline::enforce(authorized, &catalog)?;
        let lowered = pipeline::lower(enforced, &catalog, &context)?;
        engine::execute(connection, &catalog, lowered)
    }

    fn finish(&self, statement: Statement, tag: CommandTag) -> Result<Outcome, HeadError> {
        let (settings, transaction_state) = {
            let state = self.state.borrow();
            (state.settings.clone(), state.transaction.state())
        };
        self.run(&self.connection, statement, &settings, transaction_state)?;
        let mut state = self.state.borrow_mut();
        match std::mem::replace(&mut state.transaction, Transaction::Idle) {
            Transaction::Idle => Ok(Outcome::Command(tag)),
            Transaction::Open { saved, .. } if tag == CommandTag::Rollback => {
                state.settings = saved;
                engine::store::rollback(&self.connection)?;
                Ok(Outcome::Command(tag))
            }
            Transaction::Open { base, .. } => {
                state.settings = base;
                engine::store::commit(&self.connection)?;
                Ok(Outcome::Command(tag))
            }
            Transaction::Failed { saved } => {
                state.settings = saved;
                engine::store::rollback(&self.connection)?;
                Ok(Outcome::Command(CommandTag::Rollback))
            }
        }
    }
}

/// Only an open transaction has a restore point for `SET` to update and a
/// `read_only` for `SET TRANSACTION` to move.
fn apply_effect(state: &mut State, effect: SessionEffect) {
    let State {
        settings,
        transaction,
        prepared,
    } = state;
    match effect {
        SessionEffect::Set {
            target,
            value,
            local,
        } => match transaction {
            Transaction::Open { base, .. } => {
                settings.apply(target, &value);
                if !local {
                    base.apply(target, &value);
                }
            }
            Transaction::Idle | Transaction::Failed { .. } if !local => {
                settings.apply(target, &value);
            }
            Transaction::Idle | Transaction::Failed { .. } => {}
        },
        SessionEffect::SetTransaction {
            read_only: Some(read_only),
        } => {
            if let Transaction::Open {
                read_only: current, ..
            } = transaction
            {
                *current = read_only;
            }
        }
        SessionEffect::Prepare { name, statement } => {
            prepared.insert(name, *statement);
        }
        SessionEffect::SetTransaction { read_only: None }
        | SessionEffect::Begin { .. }
        | SessionEffect::Commit
        | SessionEffect::Rollback => {}
    }
}

impl Settings {
    fn new() -> Self {
        Settings {
            search_path: vec![SchemaName::literal("$user"), SchemaName::public()],
            row_security: true,
            known_text: BTreeMap::new(),
        }
    }

    pub(crate) fn search_path_includes_public(&self, catalog: &Catalog) -> bool {
        self.search_path.iter().any(|schema| {
            catalog
                .namespace(schema)
                .is_some_and(|ns| ns.oid == PUBLIC_NAMESPACE)
        })
    }

    pub(crate) fn apply(&mut self, target: KnownSetting, value: &SettingValue) {
        match (target, value) {
            (KnownSetting::SearchPath, SettingValue::SearchPath(path)) => {
                self.search_path.clone_from(path);
            }
            (KnownSetting::RowSecurity, SettingValue::RowSecurity(enabled)) => {
                self.row_security = *enabled;
            }
            (known, SettingValue::Text(text)) => {
                self.known_text.insert(known, text.clone());
            }
            _ => {}
        }
    }

    pub(crate) fn read(&self, target: KnownSetting) -> SettingValue {
        match target {
            KnownSetting::SearchPath => SettingValue::SearchPath(self.search_path.clone()),
            KnownSetting::RowSecurity => SettingValue::RowSecurity(self.row_security),
            known @ KnownSetting::StatementTimeout
            | known @ KnownSetting::LockTimeout
            | known @ KnownSetting::IdleInTransactionSessionTimeout
            | known @ KnownSetting::TransactionTimeout
            | known @ KnownSetting::DateStyle
            | known @ KnownSetting::IntervalStyle
            | known @ KnownSetting::ExtraFloatDigits
            | known @ KnownSetting::SynchronizeSeqscans => SettingValue::Text(
                self.known_text
                    .get(&known)
                    .cloned()
                    .unwrap_or_else(|| known.default_value().display()),
            ),
        }
    }
}

fn saved_settings(transaction: &Transaction) -> Result<Settings, HeadError> {
    match transaction {
        Transaction::Open { saved, .. } | Transaction::Failed { saved } => Ok(saved.clone()),
        Transaction::Idle => Err(HeadError::internal("only an open transaction can fail")),
    }
}

fn aborted() -> HeadError {
    HeadError::raise(PgError::TransactionAborted)
}
