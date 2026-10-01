#[rustfmt::skip]
// The generated schema names every column of every table it describes,
// read or not, so a column is used by name without touching the generator.
#[allow(dead_code)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/catalog_generated.rs"));
}

use crate::catalog::{Attnum, Oid};
use crate::error::HeadError;
use crate::security::privileges::Privileges;

pub(crate) use generated::{
    casts, pg_am, pg_attribute, pg_authid, pg_class, pg_constraint, pg_database, pg_depend,
    pg_description, pg_index, pg_init_privs, pg_namespace, pg_policy, pg_proc, pg_shdepend,
    pg_tablespace, pg_type, types, views,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Alignment {
    Char,
    Short,
    Int,
    Double,
}

impl Alignment {
    pub(crate) fn code(self) -> char {
        match self {
            Alignment::Char => 'c',
            Alignment::Short => 's',
            Alignment::Int => 'i',
            Alignment::Double => 'd',
        }
    }

    pub(crate) fn from_code(code: u8) -> Result<Alignment, HeadError> {
        match code {
            b'c' => Ok(Alignment::Char),
            b's' => Ok(Alignment::Short),
            b'i' => Ok(Alignment::Int),
            b'd' => Ok(Alignment::Double),
            other => Err(HeadError::internal(format!(
                "unknown typalign code {}",
                char::from(other)
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Storage {
    Plain,
    Main,
    Extended,
}

impl Storage {
    pub(crate) fn code(self) -> char {
        match self {
            Storage::Plain => 'p',
            Storage::Main => 'm',
            Storage::Extended => 'x',
        }
    }

    pub(crate) fn from_code(code: u8) -> Result<Storage, HeadError> {
        match code {
            b'p' => Ok(Storage::Plain),
            b'm' => Ok(Storage::Main),
            b'x' => Ok(Storage::Extended),
            other => Err(HeadError::internal(format!(
                "unknown typstorage code {}",
                char::from(other)
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Column {
    pub(crate) attnum: i16,
    pub(crate) name: &'static str,
    pub(crate) type_oid: u32,
    pub(crate) not_null: bool,
    pub(crate) len: i16,
    pub(crate) align: Alignment,
    pub(crate) storage: Storage,
    pub(crate) collation: u32,
    pub(crate) ndims: i32,
    pub(crate) is_system: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Table {
    pub(crate) oid: u32,
    pub(crate) name: &'static str,
    pub(crate) columns: &'static [Column],
}

impl Table {
    pub(crate) fn relation_oid(&self) -> Oid {
        Oid::new(self.oid)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ColumnId {
    pub(crate) table_oid: u32,
    pub(crate) attnum: i16,
    pub(crate) type_oid: u32,
}

impl ColumnId {
    pub(crate) fn attnum(self) -> Attnum {
        Attnum::new(usize::from(self.attnum.unsigned_abs()))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Index {
    pub(crate) oid: u32,
    pub(crate) table_oid: u32,
    pub(crate) unique: bool,
    pub(crate) key_attnums: &'static [i16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grantee {
    Public,
    Role(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AclItem {
    pub(crate) grantee: Grantee,
    pub(crate) grantor: u32,
    pub(crate) privileges: Privileges,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    I16(i16),
    I32(i32),
    F32(f32),
    Oid(u32),
    Char(u8),
    Text(&'static str),
    Name(&'static str),
    Acl(&'static [AclItem]),
    I16Array(&'static [i16]),
    OidArray(&'static [u32]),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Row {
    pub(crate) columns: &'static [(ColumnId, Value)],
}

impl Row {
    pub(crate) fn oid(&self, column: ColumnId) -> Result<u32, HeadError> {
        match self.value(column)? {
            Value::Oid(oid) => Ok(oid),
            other @ (Value::Null
            | Value::Bool(_)
            | Value::I16(_)
            | Value::I32(_)
            | Value::F32(_)
            | Value::Char(_)
            | Value::Text(_)
            | Value::Name(_)
            | Value::Acl(_)
            | Value::I16Array(_)
            | Value::OidArray(_)) => Err(HeadError::internal(format!(
                "column {column:?} is {other:?}, not an oid"
            ))),
        }
    }

    pub(crate) fn text(&self, column: ColumnId) -> Result<&'static str, HeadError> {
        match self.value(column)? {
            Value::Text(text) | Value::Name(text) => Ok(text),
            other @ (Value::Null
            | Value::Bool(_)
            | Value::I16(_)
            | Value::I32(_)
            | Value::F32(_)
            | Value::Oid(_)
            | Value::Char(_)
            | Value::Acl(_)
            | Value::I16Array(_)
            | Value::OidArray(_)) => Err(HeadError::internal(format!(
                "column {column:?} is {other:?}, not text"
            ))),
        }
    }

    pub(crate) fn bool_value(&self, column: ColumnId) -> Result<bool, HeadError> {
        match self.value(column)? {
            Value::Bool(value) => Ok(value),
            other @ (Value::Null
            | Value::I16(_)
            | Value::I32(_)
            | Value::F32(_)
            | Value::Oid(_)
            | Value::Char(_)
            | Value::Text(_)
            | Value::Name(_)
            | Value::Acl(_)
            | Value::I16Array(_)
            | Value::OidArray(_)) => Err(HeadError::internal(format!(
                "column {column:?} is {other:?}, not a bool"
            ))),
        }
    }

    pub(crate) fn char_value(&self, column: ColumnId) -> Result<u8, HeadError> {
        match self.value(column)? {
            Value::Char(value) => Ok(value),
            other @ (Value::Null
            | Value::Bool(_)
            | Value::I16(_)
            | Value::I32(_)
            | Value::F32(_)
            | Value::Oid(_)
            | Value::Text(_)
            | Value::Name(_)
            | Value::Acl(_)
            | Value::I16Array(_)
            | Value::OidArray(_)) => Err(HeadError::internal(format!(
                "column {column:?} is {other:?}, not a \"char\""
            ))),
        }
    }

    pub(crate) fn i16_value(&self, column: ColumnId) -> Result<i16, HeadError> {
        match self.value(column)? {
            Value::I16(value) => Ok(value),
            other @ (Value::Null
            | Value::Bool(_)
            | Value::I32(_)
            | Value::F32(_)
            | Value::Oid(_)
            | Value::Char(_)
            | Value::Text(_)
            | Value::Name(_)
            | Value::Acl(_)
            | Value::I16Array(_)
            | Value::OidArray(_)) => Err(HeadError::internal(format!(
                "column {column:?} is {other:?}, not an int2"
            ))),
        }
    }

    #[cfg(test)]
    pub(crate) fn i32_value(&self, column: ColumnId) -> Result<i32, HeadError> {
        match self.value(column)? {
            Value::I32(value) => Ok(value),
            other @ (Value::Null
            | Value::Bool(_)
            | Value::I16(_)
            | Value::F32(_)
            | Value::Oid(_)
            | Value::Char(_)
            | Value::Text(_)
            | Value::Name(_)
            | Value::Acl(_)
            | Value::I16Array(_)
            | Value::OidArray(_)) => Err(HeadError::internal(format!(
                "column {column:?} is {other:?}, not an int4"
            ))),
        }
    }

    pub(crate) fn acl(&self, column: ColumnId) -> Result<&'static [AclItem], HeadError> {
        match self.value(column)? {
            Value::Acl(items) => Ok(items),
            Value::Null => Ok(&[]),
            other @ (Value::Bool(_)
            | Value::I16(_)
            | Value::I32(_)
            | Value::F32(_)
            | Value::Oid(_)
            | Value::Char(_)
            | Value::Text(_)
            | Value::Name(_)
            | Value::I16Array(_)
            | Value::OidArray(_)) => Err(HeadError::internal(format!(
                "column {column:?} is {other:?}, not an aclitem array"
            ))),
        }
    }

    pub(crate) fn value(&self, column: ColumnId) -> Result<Value, HeadError> {
        self.columns
            .iter()
            .find(|entry| entry.0 == column)
            .map(|entry| entry.1)
            .ok_or_else(|| HeadError::internal(format!("row has no column {column:?}")))
    }
}

pub(crate) fn indexes_for(table: &Table) -> impl Iterator<Item = &'static Index> + use<> {
    let table_oid = table.oid;
    indexes()
        .iter()
        .filter(move |index| index.table_oid == table_oid)
}

pub(crate) fn tables() -> &'static [Table] {
    generated::TABLES
}

pub(crate) fn indexes() -> &'static [Index] {
    generated::INDEXES
}

pub(crate) fn authid_rows() -> &'static [Row] {
    generated::PG_AUTHID
}

pub(crate) fn namespace_rows() -> &'static [Row] {
    generated::PG_NAMESPACE
}

pub(crate) fn database_rows() -> &'static [Row] {
    generated::PG_DATABASE
}

pub(crate) fn init_privs_rows() -> &'static [Row] {
    generated::PG_INIT_PRIVS
}

pub(crate) fn description_rows() -> &'static [Row] {
    generated::PG_DESCRIPTION
}

pub(crate) fn type_rows() -> &'static [Row] {
    generated::TYPES
}

pub(crate) fn am_rows() -> &'static [Row] {
    generated::PG_AM
}

pub(crate) fn tablespace_rows() -> &'static [Row] {
    generated::PG_TABLESPACE
}

pub(crate) fn class_rows() -> &'static [Row] {
    generated::PG_CLASS_ROWS
}

pub(crate) fn attribute_rows() -> &'static [Row] {
    generated::PG_ATTRIBUTE_ROWS
}

pub(crate) fn index_rows() -> &'static [Row] {
    generated::PG_INDEX_ROWS
}

pub(crate) fn proc_rows() -> &'static [Row] {
    generated::PG_PROC
}

/// PostgreSQL 18's own `pg_cast` fact (`castcontext = 'i'`): whether a
/// value typed `source` implicitly casts to `target`, the same fact
/// `select_common_type` consults. `casts::IMPLICIT` is captured, not
/// hand-ranked (`capture/out/casts.json`).
pub(crate) fn implicit_cast_exists(source: u32, target: u32) -> bool {
    casts::IMPLICIT.contains(&(source, target))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EngineStorageType {
    Boolean,
    SmallInt,
    Integer,
    Real,
    Text,
    Name,
    TimestampTz,
    Blob,
    Char,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EngineType {
    pub(crate) element: EngineStorageType,
    pub(crate) array: bool,
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::generated::{
        INDEXES, PG_AM, PG_AUTHID, PG_DATABASE, PG_DESCRIPTION, PG_INIT_PRIVS, PG_NAMESPACE,
        PG_TABLESPACE, TABLES, TYPES,
    };
    use super::*;

    fn table(name: &str) -> &'static Table {
        TABLES
            .iter()
            .find(|table| table.name == name)
            .unwrap_or_else(|| panic!("no captured table named {name}"))
    }

    #[test]
    fn pg_class_has_postgres_18s_own_column_order() {
        let pg_class = table("pg_class");
        assert_eq!(pg_class.oid, 1259);
        let user_columns: Vec<&str> = pg_class
            .columns
            .iter()
            .filter(|column| !column.is_system)
            .map(|column| column.name)
            .collect();
        assert_eq!(user_columns.len(), 34);
        assert_eq!(
            user_columns,
            vec![
                "oid",
                "relname",
                "relnamespace",
                "reltype",
                "reloftype",
                "relowner",
                "relam",
                "relfilenode",
                "reltablespace",
                "relpages",
                "reltuples",
                "relallvisible",
                "relallfrozen",
                "reltoastrelid",
                "relhasindex",
                "relisshared",
                "relpersistence",
                "relkind",
                "relnatts",
                "relchecks",
                "relhasrules",
                "relhastriggers",
                "relhassubclass",
                "relrowsecurity",
                "relforcerowsecurity",
                "relispopulated",
                "relreplident",
                "relispartition",
                "relrewrite",
                "relfrozenxid",
                "relminmxid",
                "relacl",
                "reloptions",
                "relpartbound",
            ]
        );
        let system_columns: Vec<&str> = pg_class
            .columns
            .iter()
            .filter(|column| column.is_system)
            .map(|column| column.name)
            .collect();
        assert_eq!(
            system_columns,
            vec!["tableoid", "cmax", "xmax", "cmin", "xmin", "ctid"]
        );
        let name_column = pg_class
            .columns
            .iter()
            .find(|column| column.name == "relname")
            .expect("pg_class has a relname column");
        assert_eq!(name_column.attnum, 2);
        assert_eq!(name_column.type_oid, 19);
        assert!(name_column.not_null);
        assert_eq!(name_column.len, 64);
        assert_eq!(name_column.align, Alignment::Char);
        assert_eq!(name_column.storage, Storage::Plain);
        assert_eq!(name_column.collation, 950);
        assert_eq!(name_column.ndims, 0);
    }

    #[test]
    fn pg_attribute_has_postgres_18s_own_column_order() {
        let pg_attribute = table("pg_attribute");
        assert_eq!(pg_attribute.oid, 1249);
        let user_columns: Vec<&str> = pg_attribute
            .columns
            .iter()
            .filter(|column| !column.is_system)
            .map(|column| column.name)
            .collect();
        assert_eq!(
            user_columns,
            vec![
                "attrelid",
                "attname",
                "atttypid",
                "attlen",
                "attnum",
                "atttypmod",
                "attndims",
                "attbyval",
                "attalign",
                "attstorage",
                "attcompression",
                "attnotnull",
                "atthasdef",
                "atthasmissing",
                "attidentity",
                "attgenerated",
                "attisdropped",
                "attislocal",
                "attinhcount",
                "attcollation",
                "attstattarget",
                "attacl",
                "attoptions",
                "attfdwoptions",
                "attmissingval",
            ]
        );
    }

    #[test]
    fn pg_index_has_postgres_18s_own_column_order() {
        let pg_index = table("pg_index");
        assert_eq!(pg_index.oid, 2610);
        let user_columns: Vec<&str> = pg_index
            .columns
            .iter()
            .filter(|column| !column.is_system)
            .map(|column| column.name)
            .collect();
        assert_eq!(
            user_columns,
            vec![
                "indexrelid",
                "indrelid",
                "indnatts",
                "indnkeyatts",
                "indisunique",
                "indnullsnotdistinct",
                "indisprimary",
                "indisexclusion",
                "indimmediate",
                "indisclustered",
                "indisvalid",
                "indcheckxmin",
                "indisready",
                "indislive",
                "indisreplident",
                "indkey",
                "indcollation",
                "indclass",
                "indoption",
                "indexprs",
                "indpred",
            ]
        );
    }

    #[test]
    fn tables_indexes_and_types_match_the_capture_counts() {
        assert_eq!(TABLES.len(), 64);
        assert_eq!(INDEXES.len(), 124);
        assert_eq!(TYPES.len(), 50);
    }

    #[test]
    fn pg_class_indexes_carry_their_key_attnums_and_flags() {
        let pg_class = table("pg_class");
        let mut on_pg_class: Vec<&Index> = INDEXES
            .iter()
            .filter(|index| index.table_oid == pg_class.oid)
            .collect();
        on_pg_class.sort_by_key(|index| index.oid);
        assert_eq!(on_pg_class.len(), 3);
        let oid_index = on_pg_class
            .iter()
            .find(|index| index.key_attnums == [1])
            .expect("pg_class has an index keyed on oid");
        assert!(oid_index.unique);
        let relname_index = on_pg_class
            .iter()
            .find(|index| index.key_attnums == [2, 3])
            .expect("pg_class has an index keyed on (relname, relnamespace)");
        assert!(relname_index.unique);
    }

    #[test]
    fn pg_authid_rows_carry_postgres_as_a_real_superuser() {
        let postgres = PG_AUTHID
            .iter()
            .find(|row| row.text(pg_authid::ROLNAME).unwrap() == "postgres")
            .expect("postgres role is captured");
        assert_eq!(postgres.oid(pg_authid::OID).unwrap(), 10);
        assert!(postgres.bool_value(pg_authid::ROLSUPER).unwrap());
        assert_eq!(postgres.i32_value(pg_authid::ROLCONNLIMIT).unwrap(), -1);
        assert_eq!(PG_AUTHID.len(), 17);
    }

    #[test]
    fn pg_database_row_is_the_postgres_database() {
        let postgres_db = &PG_DATABASE[0];
        assert_eq!(postgres_db.text(pg_database::DATNAME).unwrap(), "postgres");
        assert_eq!(
            postgres_db.char_value(pg_database::DATLOCPROVIDER).unwrap(),
            b'c'
        );
        assert_eq!(
            postgres_db.value(pg_database::DATLOCALE).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn pg_namespace_acl_grants_resolve_role_names_to_oids() {
        let pg_catalog = PG_NAMESPACE
            .iter()
            .find(|row| row.text(pg_namespace::NSPNAME).unwrap() == "pg_catalog")
            .expect("pg_catalog namespace is captured");
        let acl = pg_catalog.acl(pg_namespace::NSPACL).unwrap();
        assert_eq!(acl.len(), 2);
        assert_eq!(
            acl[0],
            AclItem {
                grantee: Grantee::Role(10),
                grantor: 10,
                privileges: Privileges::parse("UC").unwrap(),
            }
        );
        assert_eq!(
            acl[1],
            AclItem {
                grantee: Grantee::Public,
                grantor: 10,
                privileges: Privileges::parse("U").unwrap(),
            }
        );
    }

    #[test]
    fn pg_init_privs_are_captured_for_the_bootstrap_schemas_and_catalog_relations() {
        assert_eq!(PG_INIT_PRIVS.len(), 86);
        for row in PG_INIT_PRIVS {
            assert_eq!(row.char_value(pg_init_privs::PRIVTYPE).unwrap(), b'i');
            assert!(!row.acl(pg_init_privs::INITPRIVS).unwrap().is_empty());
        }
        let namespace_rows = PG_INIT_PRIVS
            .iter()
            .filter(|row| row.oid(pg_init_privs::CLASSOID) == Ok(2615))
            .count();
        assert_eq!(
            namespace_rows, 2,
            "pg_catalog and public are the only namespaces with initial privileges"
        );
    }

    #[test]
    fn pg_description_rows_describe_the_bootstrap_schemas() {
        assert_eq!(PG_DESCRIPTION.len(), 2);
        for row in PG_DESCRIPTION {
            assert_eq!(row.i32_value(pg_description::OBJSUBID).unwrap(), 0);
            assert!(!row.text(pg_description::DESCRIPTION).unwrap().is_empty());
        }
    }

    #[test]
    fn pg_am_carries_the_heap_access_method() {
        let heap = PG_AM
            .iter()
            .find(|row| row.text(generated::pg_am::AMNAME).unwrap() == "heap")
            .expect("heap access method is captured");
        assert_eq!(heap.char_value(generated::pg_am::AMTYPE).unwrap(), b't');
        assert_eq!(heap.oid(generated::pg_am::AMHANDLER).unwrap(), 3);
    }

    #[test]
    fn pg_tablespace_carries_the_default_tablespace() {
        let pg_default = PG_TABLESPACE
            .iter()
            .find(|row| row.text(generated::pg_tablespace::SPCNAME).unwrap() == "pg_default")
            .expect("pg_default tablespace is captured");
        assert_eq!(pg_default.oid(generated::pg_tablespace::OID).unwrap(), 1663);
    }

    #[test]
    fn pg_type_rows_carry_every_column_typed() {
        let bool_type = TYPES
            .iter()
            .find(|row| row.text(generated::pg_type::TYPNAME).unwrap() == "bool")
            .expect("bool type is captured");
        assert_eq!(bool_type.oid(generated::pg_type::OID).unwrap(), 16);
        assert_eq!(bool_type.i16_value(generated::pg_type::TYPLEN).unwrap(), 1);
        assert_eq!(
            bool_type.char_value(generated::pg_type::TYPTYPE).unwrap(),
            b'b'
        );
        assert_eq!(
            bool_type
                .char_value(generated::pg_type::TYPCATEGORY)
                .unwrap(),
            b'B'
        );
        assert!(bool_type
            .bool_value(generated::pg_type::TYPISPREFERRED)
            .unwrap());
        assert_eq!(bool_type.oid(generated::pg_type::TYPINPUT).unwrap(), 1242);
        assert_eq!(
            bool_type.value(generated::pg_type::TYPDEFAULT).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn pg_class_row_describes_itself_in_the_bootstrap_data() {
        let pg_class_row = class_rows()
            .iter()
            .find(|row| row.text(generated::pg_class::RELNAME).unwrap() == "pg_class")
            .expect("pg_class's own row is captured");
        assert_eq!(pg_class_row.oid(generated::pg_class::OID).unwrap(), 1259);
        assert_eq!(
            pg_class_row
                .char_value(generated::pg_class::RELKIND)
                .unwrap(),
            b'r'
        );
        assert_eq!(
            pg_class_row
                .i16_value(generated::pg_class::RELNATTS)
                .unwrap(),
            34
        );
        assert!(pg_class_row
            .bool_value(generated::pg_class::RELHASINDEX)
            .unwrap());
        assert!(!pg_class_row
            .bool_value(generated::pg_class::RELISSHARED)
            .unwrap());
        let acl = pg_class_row.acl(generated::pg_class::RELACL).unwrap();
        assert_eq!(
            acl[0],
            AclItem {
                grantee: Grantee::Role(10),
                grantor: 10,
                privileges: Privileges::parse("arwdDxtm").unwrap(),
            }
        );
    }

    #[test]
    fn pg_proc_row_captures_the_heap_tableam_handler() {
        let heap_tableam_handler = proc_rows()
            .iter()
            .find(|row| row.text(generated::pg_proc::PRONAME).unwrap() == "heap_tableam_handler")
            .expect("heap_tableam_handler is captured");
        assert_eq!(
            heap_tableam_handler.oid(generated::pg_proc::OID).unwrap(),
            3
        );
        assert_eq!(
            heap_tableam_handler
                .oid(generated::pg_proc::PRONAMESPACE)
                .unwrap(),
            11
        );
        assert_eq!(
            heap_tableam_handler
                .oid(generated::pg_proc::PRORETTYPE)
                .unwrap(),
            269
        );
    }
}
