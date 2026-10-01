use std::collections::BTreeSet;

use turso_core::Value;

use crate::analyze::typing;
use crate::catalog::{AclEntry, Attnum, Grantee, NewNotNullConstraint, NewPrimaryKey, Oid};
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::{ColumnName, GeneratedLabel, MessageName};
use crate::parse::expr::Expr;
use crate::parse::statement::{
    ColumnDef, CommandTag, GrantObjects, GrantedPrivileges, GranteeName, RelationName,
    RelationSchema,
};
use crate::parse::Location;
use crate::security::privileges::{ObjectKind, Privileges};

use super::walk;
use super::{Lookup, Resolved};

pub(crate) struct GrantTarget {
    pub(crate) kind: ObjectKind,
    pub(crate) oid: Oid,
    pub(crate) name: MessageName,
    pub(crate) owner: Oid,
    pub(crate) new_acl: Vec<AclEntry>,
}

pub(crate) struct InsertColumn {
    pub(crate) name: ColumnName,
    pub(crate) not_null: bool,
}

pub(super) fn grant(
    privileges: GrantedPrivileges,
    objects: GrantObjects,
    grantees: &[GranteeName],
    lookup: &Lookup,
) -> Result<Resolved, HeadError> {
    let catalog = lookup.catalog;
    let (kind, objects) = match objects {
        GrantObjects::Tables(names) => (
            ObjectKind::Table,
            names
                .iter()
                .map(|name| {
                    let found = walk::table(name, lookup)?;
                    if found.backing.is_catalog() {
                        return Err(HeadError::not_supported(
                            NotSupportedFeature::CatalogRelationAction {
                                action: CommandTag::Grant,
                                table: name.name.clone(),
                            },
                        ));
                    }
                    Ok((found.oid, name.name.render_message(), found.owner))
                })
                .collect::<Result<Vec<_>, HeadError>>()?,
        ),
        GrantObjects::Schemas(names) => (
            ObjectKind::Schema,
            names
                .iter()
                .map(|name| {
                    let found = catalog.namespace(name).ok_or_else(|| {
                        HeadError::raise(PgError::SchemaDoesNotExist(name.clone()))
                    })?;
                    Ok((found.oid, name.render_message(), found.owner))
                })
                .collect::<Result<Vec<_>, HeadError>>()?,
        ),
    };
    let grantees = grantees
        .iter()
        .map(|grantee| match grantee {
            GranteeName::Public => Ok(Grantee::Public),
            GranteeName::Role(name) => catalog
                .role(name)
                .map(|role| Grantee::Role(role.oid))
                .ok_or_else(|| HeadError::raise(PgError::RoleDoesNotExist(name.clone()))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let granted = match privileges {
        GrantedPrivileges::All => Privileges::all_on(kind),
        GrantedPrivileges::Named(names) => {
            names
                .iter()
                .try_fold(Privileges::default(), |granted, name| {
                    Privileges::from_keyword(name, kind)
                        .map(|privilege| granted.union(privilege))
                        .ok_or_else(|| {
                            HeadError::raise(PgError::InvalidPrivilegeType {
                                privilege: name.as_str().to_uppercase(),
                                kind: kind.name(),
                            })
                        })
                })?
        }
    };
    let role = lookup.role;
    let targets = objects
        .into_iter()
        .map(|(oid, name, owner)| {
            let mut acl = catalog.acl(kind, oid, owner);
            if role.superuser || role.oid == owner {
                let grantor = owner;
                for grantee in &grantees {
                    match acl
                        .iter_mut()
                        .find(|entry| entry.grantee == *grantee && entry.grantor == grantor)
                    {
                        Some(entry) => entry.privileges = entry.privileges.union(granted),
                        None => acl.push(AclEntry {
                            grantee: *grantee,
                            grantor,
                            privileges: granted,
                        }),
                    }
                }
            }
            GrantTarget {
                kind,
                oid,
                name,
                owner,
                new_acl: acl,
            }
        })
        .collect();
    Ok(Resolved::Grant { targets })
}

pub(super) fn create_table(
    relation: RelationName,
    columns: Vec<ColumnDef>,
    lookup: &Lookup,
) -> Result<Resolved, HeadError> {
    match relation.schema {
        RelationSchema::PgCatalog => {
            return Err(HeadError::raise(
                PgError::PermissionDeniedToCreateInPgCatalog(relation.name),
            ))
        }
        RelationSchema::Unqualified if !lookup.search_path_includes_public => {
            return Err(HeadError::raise(PgError::NoSchemaSelectedToCreateIn).at(relation.location));
        }
        RelationSchema::Unqualified | RelationSchema::Public => {}
    }
    // `CREATE TABLE`'s relation always comes from a real, parsed `RangeVar`
    // (`parse::ddl::create_table` builds it through `table_name`), so it
    // always carries a location.
    let location = relation
        .location
        .ok_or_else(|| HeadError::internal("a CREATE TABLE relation has no location"))?;
    let catalog = lookup.catalog;
    let name = relation.name;
    let mut oids = catalog.oids();
    let oid = oids.allocate()?;
    // PostgreSQL 18 gives every NOT NULL column (explicit, or implied by
    // PRIMARY KEY, matching `ATTNOTNULL` below) its own `pg_constraint` row,
    // allocated (like PostgreSQL's own) before the primary key's index and
    // constraint.
    let not_null_constraints = (1..)
        .zip(&columns)
        .filter(|(_, column)| column.primary_key.is_some() || column.not_null)
        .map(|(attnum, column)| {
            Ok(NewNotNullConstraint {
                name: catalog.choose_relation_name(
                    &name,
                    Some(column.name.as_str()),
                    GeneratedLabel::NotNull,
                )?,
                constraint_oid: oids.allocate()?,
                attnum,
            })
        })
        .collect::<Result<Vec<_>, HeadError>>()?;
    let primary_key = match (1..)
        .zip(&columns)
        .find(|(_, column)| column.primary_key.is_some())
    {
        Some((attnum, _)) => Some(NewPrimaryKey {
            name: catalog.choose_relation_name(&name, None, GeneratedLabel::PrimaryKey)?,
            index_oid: oids.allocate()?,
            constraint_oid: oids.allocate()?,
            attnum,
        }),
        None => None,
    };
    Ok(Resolved::CreateTable {
        oid,
        name,
        columns,
        primary_key,
        not_null_constraints,
        next_oid: oids.next(),
        location,
    })
}

/// An expression's own location, for the handful of `Expr` shapes that
/// already carry one (a column reference, a function call, a literal); not
/// a general expression-position accessor, which the head does not have
/// (see `postgres/conformance/head/queries.sql`'s note on the UNION
/// `ORDER BY` gap).
fn shallow_location(expr: &Expr) -> Option<Location> {
    match expr {
        Expr::Column(_, _, location) | Expr::Call(_, _, location) => *location,
        Expr::Literal(_, location) => *location,
        Expr::CurrentUser
        | Expr::Param(..)
        | Expr::Cast(..)
        | Expr::Not(_)
        | Expr::And(_)
        | Expr::Or(_)
        | Expr::Compare(..)
        | Expr::IsNull(..)
        | Expr::Is(..)
        | Expr::DistinctFrom(..)
        | Expr::In(..)
        | Expr::Concat(..)
        | Expr::Case { .. }
        | Expr::Subquery(..)
        | Expr::Exists(..)
        | Expr::InSelect(..)
        | Expr::ArrayFromQuery(..)
        | Expr::ArrayLiteral(_)
        | Expr::Subscript(..)
        | Expr::AnyEq(..)
        | Expr::Resolved(_) => None,
    }
}

pub(super) fn insert(
    relation: &RelationName,
    columns: Option<Vec<(ColumnName, Option<Location>)>>,
    rows: Vec<Vec<Expr>>,
    lookup: &Lookup,
) -> Result<Resolved, HeadError> {
    // PostgreSQL 18's query analyzer attaches an INSERT target's own
    // position to a "relation does not exist" error, unlike a utility
    // command's relation lookup.
    let table = match walk::table(relation, lookup) {
        Ok(table) => table,
        Err(error) => match walk::view_for(relation)? {
            // A name that resolves to one of the head's own views is found,
            // not undefined; PostgreSQL itself refuses the write with its
            // own diagnostic rather than "relation does not exist".
            Some(_) => return Err(cannot_insert_into_view(&relation.name)),
            None => return Err(error.at(relation.location)),
        },
    };
    let name = &relation.name;
    let explicit_columns = columns.is_some();
    // `target_locations` parallels `targets` one for one only when
    // `explicit_columns`: PostgreSQL 18 points "INSERT has more target
    // columns than expressions" at the extra target's own name (probed
    // live), which only an explicit column list supplies.
    let (targets, target_locations): (Vec<Attnum>, Vec<Option<Location>>) = match columns {
        Some(names) => {
            let mut seen = BTreeSet::new();
            names
                .iter()
                .map(|(column, location)| {
                    if !seen.insert(column) {
                        return Err(HeadError::raise(PgError::ColumnSpecifiedMoreThanOnce(
                            column.clone(),
                        )));
                    }
                    let attnum = table.column(column).ok_or_else(|| {
                        HeadError::raise(PgError::ColumnOfRelationDoesNotExist {
                            column: column.clone(),
                            table: name.clone(),
                        })
                        .at(*location)
                    })?;
                    Ok((attnum, *location))
                })
                .collect::<Result<Vec<_>, HeadError>>()?
                .into_iter()
                .unzip()
        }
        None => (table.attnums().collect(), Vec::new()),
    };
    let width = rows.first().map_or(0, Vec::len);
    if rows.iter().any(|row| row.len() != width) {
        return Err(HeadError::raise(PgError::ValuesListsMustBeSameLength));
    }
    if width > targets.len() {
        // PostgreSQL 18 points at the first extra value past the target
        // list (probed live), read straight off whichever row supplied it
        // (every row has the same width, checked above); `shallow_location`
        // is `None` for an expression shape that carries no location of its
        // own, matching PostgreSQL's own silence for those.
        let location = rows
            .first()
            .and_then(|row| row.get(targets.len()))
            .and_then(shallow_location);
        return Err(HeadError::raise(PgError::InsertMoreExpressionsThanTargets).at(location));
    }
    if explicit_columns && width < targets.len() {
        let location = target_locations.get(width).copied().flatten();
        return Err(HeadError::raise(PgError::InsertMoreTargetsThanExpressions).at(location));
    }
    let cx = typing::Context::new(typing::Position::InsertValue);
    let rows = rows
        .into_iter()
        .map(|row| {
            let mut values = vec![Value::Null; table.columns.len()];
            for (attnum, expr) in targets.iter().zip(row) {
                let value = typing::assign(expr, table.column_at(*attnum)?.ty, &cx)?;
                let slot = values.get_mut(attnum.get() - 1).ok_or_else(|| {
                    HeadError::internal("an insert target column falls outside the row")
                })?;
                *slot = value;
            }
            Ok(values)
        })
        .collect::<Result<Vec<_>, HeadError>>()?;
    Ok(Resolved::Insert {
        table: walk::table_ref(relation, table),
        columns: table
            .columns
            .iter()
            .map(|column| InsertColumn {
                name: column.name.clone(),
                not_null: column.not_null,
            })
            .collect(),
        rows,
    })
}

/// PostgreSQL 18's own diagnostic for `INSERT` targeting a view without an
/// `INSTEAD OF INSERT` trigger or `ON INSERT DO INSTEAD` rule: every view
/// the head has is such a view, so this never needs to distinguish an
/// auto-updatable one.
fn cannot_insert_into_view(name: &crate::ident::TableName) -> HeadError {
    HeadError::raise(PgError::CannotInsertIntoView(name.clone()))
}
