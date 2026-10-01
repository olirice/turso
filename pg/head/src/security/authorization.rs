use crate::analyze::{GrantTarget, RelationFact, Resolved, TableRef};
use crate::catalog::{Catalog, Oid, Role};
use crate::error::{HeadError, PgError};
use crate::ident::TableName;
use crate::security::privileges::{ObjectKind, PrivilegeKeyword, Privileges};

pub(crate) fn check(resolved: &Resolved, role: Role, catalog: &Catalog) -> Result<(), HeadError> {
    if role.superuser {
        return Ok(());
    }
    match resolved {
        Resolved::Begin { .. }
        | Resolved::Commit
        | Resolved::Rollback
        | Resolved::SetTransaction { .. }
        | Resolved::Set { .. }
        | Resolved::Prepare { .. } => Ok(()),
        Resolved::CreateTable { location, .. } => {
            let public = catalog.public_namespace()?;
            let create = Privileges::from_keyword(&PrivilegeKeyword::Create, ObjectKind::Schema)
                .ok_or_else(|| HeadError::internal("CREATE is a schema privilege"))?;
            if catalog.holds(
                ObjectKind::Schema,
                public.oid,
                public.owner,
                role.oid,
                create,
            ) {
                return Ok(());
            }
            Err(HeadError::raise(PgError::PermissionDeniedForSchema(
                crate::ident::SchemaName::public(),
            ))
            .at(*location))
        }
        Resolved::Insert { table, .. } => {
            table_access(table, &PrivilegeKeyword::Insert, role, catalog)
        }
        Resolved::Select(plan) => plan
            .references
            .iter()
            .try_for_each(|reference| reference_access(reference, catalog)),
        Resolved::Lock { tables } => tables
            .iter()
            .try_for_each(|table| table_access(table, &PrivilegeKeyword::Select, role, catalog)),
        Resolved::CreateRole { .. } => Err(HeadError::raise(PgError::PermissionDeniedToCreateRole)),
        Resolved::Grant { targets } => targets
            .iter()
            .try_for_each(|target| grant_on(target, role, catalog)),
        Resolved::CreatePolicy { table, .. } | Resolved::AlterRowSecurity { table, .. } => {
            if table.owner == role.oid {
                return Ok(());
            }
            Err(HeadError::raise(PgError::MustBeOwnerOfTable(
                table.name.clone(),
            )))
        }
    }
}

fn table_access(
    table: &TableRef,
    keyword: &PrivilegeKeyword,
    role: Role,
    catalog: &Catalog,
) -> Result<(), HeadError> {
    table_privilege(
        table.oid,
        table.owner,
        &table.name,
        keyword,
        role.oid,
        catalog,
    )
}

fn reference_access(reference: &RelationFact, catalog: &Catalog) -> Result<(), HeadError> {
    if let Some(actor) = catalog.role_by_oid(reference.checked_as) {
        if actor.superuser {
            return Ok(());
        }
    }
    table_privilege(
        reference.oid,
        reference.owner,
        &reference.name,
        &PrivilegeKeyword::Select,
        reference.checked_as,
        catalog,
    )
}

fn table_privilege(
    oid: Oid,
    owner: Oid,
    name: &TableName,
    keyword: &PrivilegeKeyword,
    actor: Oid,
    catalog: &Catalog,
) -> Result<(), HeadError> {
    let needed = Privileges::from_keyword(keyword, ObjectKind::Table)
        .ok_or_else(|| HeadError::internal("the keyword is a table privilege"))?;
    if catalog.holds(ObjectKind::Table, oid, owner, actor, needed) {
        return Ok(());
    }
    Err(HeadError::raise(PgError::PermissionDeniedForTable(
        name.clone(),
    )))
}

fn grant_on(target: &GrantTarget, role: Role, catalog: &Catalog) -> Result<(), HeadError> {
    if target.owner == role.oid {
        return Ok(());
    }
    if catalog.holds_any(target.kind, target.oid, target.owner, role.oid) {
        return Ok(());
    }
    Err(HeadError::raise(PgError::PermissionDeniedForObject {
        kind: target.kind,
        name: target.name.clone(),
    }))
}
