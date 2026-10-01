use std::sync::{Arc, OnceLock};

use turso_core::Value;

use crate::catalog::pg::{self, ColumnId};
use crate::catalog::{
    Namespace, Oid, DATABASE_OID, FIRST_USER_OID, PG_CATALOG_NAMESPACE, PG_DATABASE_OWNER_ROLE,
    POSTGRES_ROLE, PUBLIC_NAMESPACE,
};
use crate::error::HeadError;
use crate::lower::sql;
use crate::security::privileges::ObjectKind;

/// Declares `ConstantRelation` and `ALL` from the same variant list, so a
/// variant can only ever be added to both, in the same edit (replacing a
/// hand-maintained `ALL` array that was a second, independent list of the
/// same variants, in the same order `index` below relied on by
/// convention alone).
macro_rules! constant_relations {
    ($($variant:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum ConstantRelation {
            $($variant),+
        }

        impl ConstantRelation {
            pub(crate) const ALL: [ConstantRelation; 10] = [$(ConstantRelation::$variant),+];
        }
    };
}

constant_relations!(
    Class,
    Attribute,
    Index,
    Namespace,
    Type,
    Am,
    Tablespace,
    Description,
    InitPrivs,
    Proc,
);

impl ConstantRelation {
    /// This variant's own slot in `ROWS` (`relation_rows` below, built as
    /// `ConstantRelation::ALL.map(build_relation_rows)`): its position in
    /// `ALL` itself, rather than a third, hand-assigned list of numbers
    /// that had to be kept in the same order as `ALL` by convention, with
    /// nothing to check that they still lined up.
    fn index(self) -> usize {
        ConstantRelation::ALL
            .iter()
            .position(|&relation| relation == self)
            .unwrap_or(usize::MAX)
    }

    pub(crate) fn table(self) -> &'static pg::Table {
        match self {
            ConstantRelation::Class => &pg::pg_class::TABLE,
            ConstantRelation::Attribute => &pg::pg_attribute::TABLE,
            ConstantRelation::Index => &pg::pg_index::TABLE,
            ConstantRelation::Namespace => &pg::pg_namespace::TABLE,
            ConstantRelation::Type => &pg::pg_type::TABLE,
            ConstantRelation::Am => &pg::pg_am::TABLE,
            ConstantRelation::Tablespace => &pg::pg_tablespace::TABLE,
            ConstantRelation::Description => &pg::pg_description::TABLE,
            ConstantRelation::InitPrivs => &pg::pg_init_privs::TABLE,
            ConstantRelation::Proc => &pg::pg_proc::TABLE,
        }
    }

    fn rows(self) -> Result<Vec<&'static pg::Row>, HeadError> {
        match self {
            ConstantRelation::Class => Ok(pg::class_rows().iter().collect()),
            ConstantRelation::Attribute => Ok(pg::attribute_rows().iter().collect()),
            ConstantRelation::Index => Ok(pg::index_rows().iter().collect()),
            ConstantRelation::Namespace => {
                let row = pg::namespace_rows()
                    .iter()
                    .find(|row| row.text(pg::pg_namespace::NSPNAME).ok() == Some("pg_catalog"))
                    .ok_or_else(|| HeadError::internal("pg_catalog is captured in pg_namespace"))?;
                Ok(vec![row])
            }
            ConstantRelation::Type => Ok(pg::type_rows().iter().collect()),
            ConstantRelation::Am => ["heap", "btree"]
                .iter()
                .map(|name| {
                    pg::am_rows()
                        .iter()
                        .find(|row| row.text(pg::pg_am::AMNAME).ok() == Some(*name))
                        .ok_or_else(|| {
                            HeadError::internal(format!("{name} is a captured access method"))
                        })
                })
                .collect(),
            ConstantRelation::Tablespace => Ok(pg::tablespace_rows().iter().collect()),
            ConstantRelation::Description => {
                let pg_catalog = PG_CATALOG_NAMESPACE.get();
                let row = pg::description_rows()
                    .iter()
                    .find(|row| row.oid(pg::pg_description::OBJOID) == Ok(pg_catalog))
                    .ok_or_else(|| HeadError::internal("pg_catalog's description is captured"))?;
                Ok(vec![row])
            }
            ConstantRelation::InitPrivs => {
                let public = PUBLIC_NAMESPACE.get();
                let namespace_class = ObjectKind::Schema.class_oid();
                let rows: Vec<&pg::Row> = pg::init_privs_rows()
                    .iter()
                    .filter(|row| {
                        !(row.oid(pg::pg_init_privs::OBJOID) == Ok(public)
                            && row
                                .oid(pg::pg_init_privs::CLASSOID)
                                .is_ok_and(|class| i64::from(class) == namespace_class))
                    })
                    .collect();
                if rows.is_empty() {
                    return Err(HeadError::internal(
                        "the catalog's initial privileges are captured",
                    ));
                }
                Ok(rows)
            }
            ConstantRelation::Proc => Ok(pg::proc_rows().iter().collect()),
        }
    }
}

type CachedRows = Result<Arc<Vec<Vec<Value>>>, HeadError>;

static ROWS: OnceLock<[CachedRows; 10]> = OnceLock::new();

pub(crate) fn relation_rows(relation: ConstantRelation) -> Result<Arc<Vec<Vec<Value>>>, HeadError> {
    let cached = ROWS.get_or_init(|| {
        ConstantRelation::ALL.map(|relation| build_relation_rows(relation).map(Arc::new))
    });
    match cached.get(relation.index()) {
        Some(Ok(rows)) => Ok(Arc::clone(rows)),
        Some(Err(error)) => Err(error.clone()),
        None => Err(HeadError::internal("constant relation index out of range")),
    }
}

fn build_relation_rows(relation: ConstantRelation) -> Result<Vec<Vec<Value>>, HeadError> {
    let table = relation.table();
    relation
        .rows()?
        .into_iter()
        .map(|row| {
            let cells = super::store::rows::row_cells(table, row)?;
            let cells = match relation {
                ConstantRelation::Class => class_deviations(row, cells)?,
                ConstantRelation::Attribute
                | ConstantRelation::Index
                | ConstantRelation::Namespace
                | ConstantRelation::Type
                | ConstantRelation::Am
                | ConstantRelation::Tablespace
                | ConstantRelation::Description
                | ConstantRelation::InitPrivs
                | ConstantRelation::Proc => cells,
            };
            realize_row(cells)
        })
        .collect()
}

fn class_deviations(row: &pg::Row, mut cells: Vec<sql::Cell>) -> Result<Vec<sql::Cell>, HeadError> {
    let oid = Oid::new(row.oid(pg::pg_class::OID)?);
    let is_index = row.char_value(pg::pg_class::RELKIND)? == b'i';
    let storage = super::store::rows::class_storage_facts(oid, is_index);
    let mut facts: Vec<(ColumnId, sql::Cell)> = vec![
        (
            pg::pg_class::RELTYPE,
            sql::Cell::Scalar(Value::from_i64(storage.reltype)),
        ),
        (
            pg::pg_class::RELTOASTRELID,
            sql::Cell::Scalar(Value::from_i64(storage.reltoastrelid)),
        ),
        (
            pg::pg_class::RELALLVISIBLE,
            sql::Cell::Scalar(Value::from_i64(storage.relallvisible)),
        ),
        (
            pg::pg_class::RELALLFROZEN,
            sql::Cell::Scalar(Value::from_i64(storage.relallfrozen)),
        ),
        (
            pg::pg_class::RELMINMXID,
            sql::Cell::Scalar(Value::from_i64(storage.relminmxid)),
        ),
        (
            pg::pg_class::RELFROZENXID,
            sql::Cell::Scalar(Value::from_i64(storage.relfrozenxid)),
        ),
        (
            pg::pg_class::RELFILENODE,
            sql::Cell::Scalar(Value::from_i64(storage.relfilenode)),
        ),
        (
            pg::pg_class::RELPAGES,
            sql::Cell::Scalar(Value::from_i64(storage.relpages)),
        ),
        (
            pg::pg_class::RELTUPLES,
            sql::Cell::Scalar(Value::from_f64(storage.reltuples)),
        ),
    ];
    for (column, cell) in pg::pg_class::TABLE
        .columns
        .iter()
        .filter(|column| !column.is_system)
        .zip(cells.iter_mut())
    {
        let id = ColumnId {
            table_oid: pg::pg_class::TABLE.oid,
            attnum: column.attnum,
            type_oid: column.type_oid,
        };
        if let Some(index) = facts.iter().position(|(fact_id, _)| *fact_id == id) {
            *cell = facts.remove(index).1;
        }
    }
    Ok(cells)
}

fn realize_row(cells: Vec<sql::Cell>) -> Result<Vec<Value>, HeadError> {
    cells
        .into_iter()
        .map(|cell| match cell {
            sql::Cell::Scalar(value) => Ok(value),
            sql::Cell::Array(values) => turso_core::encode_array(&values)
                .map_err(|error| HeadError::internal(error.to_string())),
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(super) struct ProjectObject(());

impl ProjectObject {
    pub(super) fn new(oid: Oid) -> Result<ProjectObject, HeadError> {
        if oid.get() >= FIRST_USER_OID || is_project_seeded(oid) {
            return Ok(ProjectObject(()));
        }
        // PostgreSQL allows changing some built-in rows (for example
        // REVOKE on a built-in function, GRANT on a catalog table,
        // COMMENT ON a built-in object) and the head refuses these for
        // now; supporting them would mean a project row that overrides
        // the constant row.
        Err(HeadError::internal(format!(
            "oid {} is part of the constant catalog layer and cannot be written to",
            oid.get()
        )))
    }
}

fn is_project_seeded(oid: Oid) -> bool {
    oid == POSTGRES_ROLE
        || oid == PG_DATABASE_OWNER_ROLE
        || oid == PUBLIC_NAMESPACE
        || oid.get() == DATABASE_OID
}

pub(super) fn namespace() -> Result<(&'static str, Namespace), HeadError> {
    let row = pg::namespace_rows()
        .iter()
        .find(|row| row.text(pg::pg_namespace::NSPNAME) == Ok("pg_catalog"))
        .ok_or_else(|| HeadError::internal("pg_catalog is captured"))?;
    Ok((
        "pg_catalog",
        Namespace {
            oid: Oid::new(row.oid(pg::pg_namespace::OID)?),
            owner: Oid::new(row.oid(pg::pg_namespace::NSPOWNER)?),
        },
    ))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;

    #[test]
    fn a_new_project_object_oid_is_allowed() {
        assert!(ProjectObject::new(Oid::new(FIRST_USER_OID)).is_ok());
        assert!(ProjectObject::new(Oid::new(FIRST_USER_OID + 1)).is_ok());
    }

    #[test]
    fn every_project_seeded_oid_is_allowed() {
        assert!(ProjectObject::new(POSTGRES_ROLE).is_ok());
        assert!(ProjectObject::new(PG_DATABASE_OWNER_ROLE).is_ok());
        assert!(ProjectObject::new(PUBLIC_NAMESPACE).is_ok());
        assert!(ProjectObject::new(Oid::new(DATABASE_OID)).is_ok());
    }

    #[test]
    fn pg_class_own_oid_is_refused() {
        assert!(ProjectObject::new(Oid::new(1259)).is_err());
    }

    #[test]
    fn pg_class_captured_index_oids_are_refused() {
        let pg_class = pg::tables()
            .iter()
            .find(|table| table.name == "pg_class")
            .expect("pg_class is captured");
        let indexes: Vec<_> = pg::indexes_for(pg_class)
            .filter(|index| index.unique)
            .collect();
        assert!(!indexes.is_empty());
        for index in indexes {
            assert!(ProjectObject::new(Oid::new(index.oid)).is_err());
        }
    }

    #[test]
    fn a_predefined_roles_real_oid_is_refused() {
        let pg_monitor = pg::authid_rows()
            .iter()
            .find(|row| row.text(pg::pg_authid::ROLNAME) == Ok("pg_monitor"))
            .expect("pg_monitor is captured");
        let oid = pg_monitor
            .oid(pg::pg_authid::OID)
            .expect("pg_monitor has an oid");
        assert!(ProjectObject::new(Oid::new(oid)).is_err());
    }

    #[test]
    fn pg_catalog_is_the_constant_namespace() {
        let (name, namespace) = namespace().expect("pg_catalog is captured");
        assert_eq!(name, "pg_catalog");
        assert_eq!(namespace.oid, PG_CATALOG_NAMESPACE);
        assert!(ProjectObject::new(PG_CATALOG_NAMESPACE).is_err());
    }

    #[test]
    fn relation_rows_builds_every_constant_relation_with_no_connection_at_all() {
        for relation in ConstantRelation::ALL {
            let rows = relation_rows(relation)
                .unwrap_or_else(|error| panic!("building {relation:?} rows failed: {error}"));
            assert!(
                !rows.is_empty(),
                "{relation:?} has at least one constant row"
            );
        }
    }
}
