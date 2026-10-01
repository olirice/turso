use std::sync::Arc;

use turso_core::{Database, OpenOptions, IO};

use crate::engine;
use crate::engine_hooks;
use crate::error::{HeadError, PgError};
use crate::ident::RoleName;
use crate::session::Session;

pub(crate) struct FromStartup(());

pub struct Head {
    db: Arc<Database>,
    catalog_cache: Arc<engine::store::CatalogCache>,
}

impl Head {
    pub fn open(io: Arc<dyn IO>, path: &str) -> Result<Head, HeadError> {
        let options = OpenOptions::new(Arc::new(engine_hooks::EngineHooks));
        let db = Database::open(io, path, options)?;
        engine::store::bootstrap(&engine::open(db.connect()?))?;
        let catalog_cache = Arc::new(engine::store::CatalogCache::new());
        Ok(Head { db, catalog_cache })
    }

    pub fn connect(&self, role: &str) -> Result<Session, HeadError> {
        let connection = engine::open(self.db.connect()?);
        let catalog = engine::store::catalog(&connection, &self.catalog_cache, true)?;
        let identity = RoleName::from_startup_user(FromStartup(()), role);
        match catalog.role(&identity) {
            None => Err(HeadError::raise(PgError::AuthRoleDoesNotExist(
                identity.clone(),
            ))),
            Some(found) if !found.can_login => Err(HeadError::raise(
                PgError::AuthRoleNotPermittedToLogin(identity.clone()),
            )),
            Some(found) => Ok(Session::new(
                connection,
                identity,
                found.superuser,
                Arc::clone(&self.catalog_cache),
            )),
        }
    }
}
