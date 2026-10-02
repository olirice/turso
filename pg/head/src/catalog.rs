use crate::ident::{GeneratedLabel, Ident};
use std::collections::{BTreeMap, BTreeSet};

use crate::analyze::types::TypeHandle;
use crate::error::HeadError;
use crate::ident::{ColumnName, ConstraintName, PolicyName, RoleName, SchemaName, TableName};
use crate::parse::statement::ColumnDef;
use crate::security::privileges::{ObjectKind, Privileges};

pub(crate) mod pg;

const ACL_ITEM_BYTES: usize = 10;

pub(crate) const POSTGRES_ROLE: Oid = Oid(10);
pub(crate) const PG_DATABASE_OWNER_ROLE: Oid = Oid(6171);
pub(crate) const PUBLIC_NAMESPACE: Oid = Oid(2200);
pub(crate) const PG_CATALOG_NAMESPACE: Oid = Oid(11);
pub(crate) const FIRST_USER_OID: u32 = 16384;
pub(crate) const DATABASE_OID: u32 = 5;

pub struct Catalog {
    pub(crate) roles: BTreeMap<RoleName, Role>,
    pub(crate) relations: BTreeSet<Ident>,
    pub(crate) tables: BTreeMap<TableName, Table>,
    pub(crate) catalog_relations: BTreeMap<TableName, Table>,
    pub(crate) acls: BTreeMap<(ObjectKind, Oid), Vec<AclEntry>>,
    pub(crate) namespaces: BTreeMap<SchemaName, Namespace>,
    pub(crate) next_oid: Oid,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Backing {
    User,
    Catalog { constant: bool, project: bool },
}

impl Backing {
    pub(crate) fn is_catalog(self) -> bool {
        matches!(self, Backing::Catalog { .. })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Namespace {
    pub(crate) oid: Oid,
    pub(crate) owner: Oid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AclEntry {
    pub(crate) grantee: Grantee,
    pub(crate) grantor: Oid,
    pub(crate) privileges: Privileges,
}

impl AclEntry {
    pub(crate) fn to_bytes(self) -> Vec<u8> {
        let grantee: u32 = match self.grantee {
            Grantee::Public => 0,
            Grantee::Role(oid) => oid.get(),
        };
        let mut bytes = Vec::with_capacity(ACL_ITEM_BYTES);
        bytes.extend_from_slice(&grantee.to_le_bytes());
        bytes.extend_from_slice(&self.grantor.get().to_le_bytes());
        bytes.extend_from_slice(&self.privileges.bits().to_le_bytes());
        bytes
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, HeadError> {
        let &[g0, g1, g2, g3, r0, r1, r2, r3, p0, p1] = bytes else {
            return Err(HeadError::internal(format!(
                "an aclitem must be {ACL_ITEM_BYTES} bytes, found {}",
                bytes.len()
            )));
        };
        let grantee_oid = u32::from_le_bytes([g0, g1, g2, g3]);
        let grantor_oid = u32::from_le_bytes([r0, r1, r2, r3]);
        let bits = u16::from_le_bytes([p0, p1]);
        Ok(AclEntry {
            grantee: if grantee_oid == 0 {
                Grantee::Public
            } else {
                Grantee::Role(Oid::new(grantee_oid))
            },
            grantor: Oid::new(grantor_oid),
            privileges: Privileges::from_bits(bits),
        })
    }

    pub(crate) fn to_value(self) -> Result<turso_core::Value, HeadError> {
        turso_core::Value::from_slice(&self.to_bytes())
            .map_err(|_| HeadError::internal("a 10-byte aclitem allocation succeeds"))
    }

    pub(crate) fn from_value(value: &turso_core::Value) -> Result<Self, HeadError> {
        let bytes = value
            .to_blob()
            .ok_or_else(|| HeadError::internal("an aclitem array element was not a blob"))?;
        AclEntry::from_bytes(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Grantee {
    Public,
    Role(Oid),
}

impl From<pg::AclItem> for AclEntry {
    fn from(item: pg::AclItem) -> Self {
        AclEntry {
            grantee: match item.grantee {
                pg::Grantee::Public => Grantee::Public,
                pg::Grantee::Role(oid) => Grantee::Role(Oid::new(oid)),
            },
            grantor: Oid::new(item.grantor),
            privileges: item.privileges,
        }
    }
}

pub(crate) struct Table {
    pub(crate) oid: Oid,
    pub(crate) owner: Oid,
    pub(crate) columns: Vec<Column>,
    pub(crate) primary_key: Option<PrimaryKey>,
    pub(crate) not_null_constraints: Vec<NotNullConstraint>,
    pub(crate) rls_enabled: bool,
    pub(crate) rls_forced: bool,
    pub(crate) policies: Vec<Policy>,
    pub(crate) backing: Backing,
}

pub(crate) struct PrimaryKey {
    pub(crate) name: ConstraintName,
    pub(crate) index_oid: Oid,
    pub(crate) constraint_oid: Oid,
    pub(crate) attnum: Attnum,
}

/// One of PostgreSQL 18's `pg_constraint` rows for a NOT NULL column
/// (`contype = 'n'`), loaded back so `pg_get_constraintdef` can render it.
pub(crate) struct NotNullConstraint {
    pub(crate) constraint_oid: Oid,
    pub(crate) attnum: Attnum,
}

pub(crate) struct Policy {
    pub(crate) name: PolicyName,
    pub(crate) roles: Vec<Grantee>,
    pub(crate) command: crate::security::row_security::PolicyCommand,
    pub(crate) using: crate::security::row_security::PolicyPredicate,
}

#[derive(Clone)]
pub(crate) struct Column {
    pub(crate) name: ColumnName,
    pub(crate) ty: TypeHandle,
    pub(crate) not_null: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Attnum(usize);

#[derive(Clone, Copy)]
pub(crate) struct Role {
    pub(crate) oid: Oid,
    pub(crate) superuser: bool,
    pub(crate) can_login: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Oid(u32);

pub(crate) struct NewPrimaryKey {
    pub(crate) name: ConstraintName,
    pub(crate) index_oid: Oid,
    pub(crate) constraint_oid: Oid,
    pub(crate) attnum: usize,
}

/// PostgreSQL 18 gives every NOT NULL column (explicit, or implied by
/// PRIMARY KEY) its own `pg_constraint` row; one of these per such column,
/// allocated alongside the primary key's own objects.
pub(crate) struct NewNotNullConstraint {
    pub(crate) name: ConstraintName,
    pub(crate) constraint_oid: Oid,
    pub(crate) attnum: usize,
}

pub(crate) enum CatalogWrite {
    CreateTable {
        oid: Oid,
        name: TableName,
        owner: Oid,
        columns: Vec<ColumnDef>,
        primary_key: Option<NewPrimaryKey>,
        not_null_constraints: Vec<NewNotNullConstraint>,
    },
    CreateRole {
        oid: Oid,
        name: RoleName,
        can_login: bool,
    },
    CreatePolicy {
        oid: Oid,
        name: PolicyName,
        table: Oid,
        roles: Vec<Grantee>,
        command: crate::security::row_security::PolicyCommand,
        using: String,
        referenced_columns: BTreeSet<Attnum>,
    },
    SetRowSecurity {
        table: Oid,
        enabled: bool,
        forced: bool,
    },
    ReplaceAcl {
        kind: ObjectKind,
        object: Oid,
        owner: Oid,
        entries: Vec<AclEntry>,
    },
    SetNextOid(Oid),
}

pub(crate) struct OidAllocator {
    next: Oid,
}

impl Catalog {
    pub(crate) fn namespace(&self, name: &SchemaName) -> Option<Namespace> {
        self.namespaces.get(name).copied()
    }

    pub(crate) fn public_namespace(&self) -> Result<Namespace, HeadError> {
        self.namespace(&SchemaName::public())
            .ok_or_else(|| HeadError::internal("bootstrap always creates the public schema"))
    }

    pub(crate) fn holds(
        &self,
        kind: ObjectKind,
        object: Oid,
        owner: Oid,
        role: Oid,
        needed: Privileges,
    ) -> bool {
        self.held(kind, object, owner, role).contains(needed)
    }

    pub(crate) fn holds_any(&self, kind: ObjectKind, object: Oid, owner: Oid, role: Oid) -> bool {
        !self.held(kind, object, owner, role).is_empty()
    }

    pub(crate) fn acl(&self, kind: ObjectKind, object: Oid, owner: Oid) -> Vec<AclEntry> {
        self.acls.get(&(kind, object)).cloned().unwrap_or_else(|| {
            vec![AclEntry {
                grantee: Grantee::Role(owner),
                grantor: owner,
                privileges: Privileges::all_on(kind),
            }]
        })
    }

    fn held(&self, kind: ObjectKind, object: Oid, owner: Oid, role: Oid) -> Privileges {
        self.acl(kind, object, owner)
            .iter()
            .filter(|entry| {
                matches!(entry.grantee, Grantee::Public) || entry.grantee == Grantee::Role(role)
            })
            .fold(Privileges::default(), |held, entry| {
                held.union(entry.privileges)
            })
    }

    pub(crate) fn table(&self, name: &TableName) -> Option<&Table> {
        self.tables.get(name)
    }

    pub(crate) fn catalog_relation(&self, name: &TableName) -> Option<&Table> {
        self.catalog_relations.get(name)
    }

    pub(crate) fn catalog_relation_by_oid(&self, oid: Oid) -> Option<&Table> {
        self.catalog_relations
            .values()
            .find(|table| table.oid == oid)
    }

    pub(crate) fn relation_name_at(&self, oid: Oid) -> Option<&TableName> {
        self.catalog_relations
            .iter()
            .find(|(_, table)| table.oid == oid)
            .or_else(|| self.tables.iter().find(|(_, table)| table.oid == oid))
            .map(|(name, _)| name)
    }

    pub(crate) fn tables_by_oid(&self) -> Vec<(&TableName, &Table)> {
        let mut tables = self.tables.iter().collect::<Vec<_>>();
        tables.sort_by_key(|(_, table)| table.oid);
        tables
    }

    #[cfg(test)]
    pub(crate) fn roles_by_oid(&self) -> Vec<(&RoleName, &Role)> {
        let mut roles = self.roles.iter().collect::<Vec<_>>();
        roles.sort_by_key(|(_, role)| role.oid);
        roles
    }

    pub(crate) fn role_name(&self, oid: Oid) -> Option<&RoleName> {
        self.roles
            .iter()
            .find(|(_, role)| role.oid == oid)
            .map(|(name, _)| name)
    }

    pub(crate) fn role_by_oid(&self, oid: Oid) -> Option<&Role> {
        self.roles.values().find(|role| role.oid == oid)
    }

    #[cfg(test)]
    pub(crate) fn namespaces_by_oid(&self) -> Vec<(&SchemaName, &Namespace)> {
        let mut namespaces = self.namespaces.iter().collect::<Vec<_>>();
        namespaces.sort_by_key(|(_, namespace)| namespace.oid);
        namespaces
    }

    pub(crate) fn role(&self, name: &RoleName) -> Option<&Role> {
        self.roles.get(name)
    }

    pub(crate) fn relation_exists(&self, name: &Ident) -> bool {
        self.relations.contains(name)
    }

    pub(crate) fn oids(&self) -> OidAllocator {
        OidAllocator {
            next: self.next_oid,
        }
    }

    pub(crate) fn choose_relation_name(
        &self,
        table: &TableName,
        middle: Option<&str>,
        label: GeneratedLabel,
    ) -> Result<ConstraintName, HeadError> {
        let mut attempt = 0usize;
        loop {
            let candidate = ConstraintName::generated(table, middle, label, attempt)?;
            if !self.relation_exists(candidate.ident()) {
                return Ok(candidate);
            }
            attempt += 1;
        }
    }
}

impl OidAllocator {
    pub(crate) fn allocate(&mut self) -> Result<Oid, HeadError> {
        let oid = self.next;
        self.next = Oid(oid
            .0
            .checked_add(1)
            .ok_or_else(|| HeadError::internal("the oid allocator has exhausted u32"))?);
        Ok(oid)
    }

    pub(crate) fn next(&self) -> Oid {
        self.next
    }
}

impl Oid {
    pub(crate) const fn new(oid: u32) -> Self {
        Oid(oid)
    }

    pub(crate) fn from_i64(value: i64) -> Result<Self, HeadError> {
        u32::try_from(value)
            .map(Oid)
            .map_err(|_| HeadError::internal("oid does not fit in u32"))
    }

    pub(crate) const fn get(self) -> u32 {
        self.0
    }

    pub(crate) fn as_i64(self) -> i64 {
        i64::from(self.0)
    }
}

impl Table {
    pub(crate) fn applies_to(policy: &Policy, role: Oid) -> bool {
        policy
            .roles
            .iter()
            .any(|grantee| matches!(grantee, Grantee::Public) || *grantee == Grantee::Role(role))
    }

    pub(crate) fn attnums(&self) -> impl Iterator<Item = Attnum> {
        (1..=self.columns.len()).map(Attnum)
    }

    pub(crate) fn column(&self, name: &ColumnName) -> Option<Attnum> {
        self.columns
            .iter()
            .position(|column| &column.name == name)
            .map(|index| Attnum(index + 1))
    }

    pub(crate) fn column_at(&self, attnum: Attnum) -> Result<&Column, HeadError> {
        self.columns
            .get(attnum.0.wrapping_sub(1))
            .ok_or_else(|| HeadError::internal("an attnum fell outside a table's columns"))
    }
}

impl Attnum {
    pub(crate) const fn new(attnum: usize) -> Self {
        Attnum(attnum)
    }

    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

impl TryFrom<i16> for Attnum {
    type Error = HeadError;

    fn try_from(attnum: i16) -> Result<Self, HeadError> {
        usize::try_from(attnum)
            .map(Attnum)
            .map_err(|_| HeadError::internal("a catalog attnum was negative"))
    }
}
