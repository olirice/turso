use std::sync::{Arc, Mutex};

use crate::catalog::Catalog;
use crate::error::HeadError;

pub(crate) struct CatalogCache {
    entry: Mutex<Option<(i64, Arc<Catalog>)>>,
}

impl CatalogCache {
    pub(crate) fn new() -> Self {
        CatalogCache {
            entry: Mutex::new(None),
        }
    }

    pub(super) fn lookup(&self, version: i64) -> Result<Option<Arc<Catalog>>, HeadError> {
        let entry = self.entry.lock().map_err(|_| lock_poisoned())?;
        Ok(entry
            .as_ref()
            .filter(|(cached_version, _)| *cached_version == version)
            .map(|(_, catalog)| Arc::clone(catalog)))
    }

    pub(super) fn publish(&self, version: i64, catalog: Arc<Catalog>) -> Result<(), HeadError> {
        let mut entry = self.entry.lock().map_err(|_| lock_poisoned())?;
        *entry = Some((version, catalog));
        Ok(())
    }
}

fn lock_poisoned() -> HeadError {
    HeadError::internal("the catalog cache lock is poisoned")
}
