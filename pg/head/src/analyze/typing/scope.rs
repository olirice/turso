use std::rc::Rc;

use crate::analyze::types::TypeHandle;
use crate::catalog::{Attnum, Column, Oid, Table};
use crate::error::{HeadError, PgError};
use crate::ident::{ColumnName, TableName};

use super::RelationSlot;

#[derive(Clone)]
pub(crate) enum RelationShape<'t> {
    Catalog(&'t Table),
    Synthetic(Rc<Vec<Column>>),
}

impl<'t> RelationShape<'t> {
    fn len(&self) -> usize {
        match self {
            RelationShape::Catalog(table) => table.columns.len(),
            RelationShape::Synthetic(columns) => columns.len(),
        }
    }

    pub(crate) fn attnums(&self) -> impl Iterator<Item = Attnum> {
        (1..=self.len()).map(Attnum::new)
    }

    pub(crate) fn column(&self, name: &ColumnName) -> Option<Attnum> {
        match self {
            RelationShape::Catalog(table) => table.column(name),
            RelationShape::Synthetic(columns) => columns
                .iter()
                .position(|column| &column.name == name)
                .map(|index| Attnum::new(index + 1)),
        }
    }

    pub(crate) fn column_at(&self, attnum: Attnum) -> Result<(ColumnName, TypeHandle), HeadError> {
        match self {
            RelationShape::Catalog(table) => {
                let column = table.column_at(attnum)?;
                Ok((column.name.clone(), column.ty))
            }
            RelationShape::Synthetic(columns) => columns
                .get(attnum.get() - 1)
                .map(|column| (column.name.clone(), column.ty))
                .ok_or_else(|| HeadError::internal("an attnum fell outside a synthetic relation")),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ScopeRelation<'a> {
    pub(crate) slot: RelationSlot,
    pub(crate) visible_as: TableName,
    pub(crate) shape: RelationShape<'a>,
}

pub(crate) struct Scope<'t, 's> {
    pub(crate) relations: Vec<ScopeRelation<'t>>,
    pub(crate) outer: Option<&'s Scope<'t, 's>>,
}

impl<'t> Scope<'t, '_> {
    pub(crate) fn empty() -> Self {
        Scope {
            relations: Vec::new(),
            outer: None,
        }
    }

    pub(crate) fn single(slot: RelationSlot, visible_as: TableName, table: &'t Table) -> Self {
        Scope {
            relations: vec![ScopeRelation {
                slot,
                visible_as,
                shape: RelationShape::Catalog(table),
            }],
            outer: None,
        }
    }
}

/// PostgreSQL 18 quotes only the column name for an unqualified reference
/// ("column \"nope\" does not exist") but renders a qualified one raw and
/// dotted, with no quoting at all ("column e.nope does not exist").
fn undefined_column(qualifier: &Option<TableName>, name: &ColumnName) -> HeadError {
    HeadError::raise(PgError::UndefinedColumn {
        qualifier: qualifier.clone(),
        column: name.clone(),
    })
}

fn ambiguous_column(name: &ColumnName) -> HeadError {
    HeadError::raise(PgError::AmbiguousColumnReference(name.clone()))
}

pub(crate) fn missing_from_clause_entry(qualifier: &TableName) -> HeadError {
    HeadError::raise(PgError::MissingFromClauseEntry(qualifier.clone()))
}

pub(super) fn column_lookup(
    qualifier: &Option<TableName>,
    name: &ColumnName,
    scope: &Scope<'_, '_>,
) -> Result<(RelationSlot, Attnum, TypeHandle), HeadError> {
    let candidates: Vec<&ScopeRelation> = match qualifier {
        None => scope.relations.iter().collect(),
        Some(qualifier) => {
            let matches: Vec<&ScopeRelation> = scope
                .relations
                .iter()
                .filter(|relation| &relation.visible_as == qualifier)
                .collect();
            if matches.is_empty() {
                let requalified = Some(qualifier.clone());
                return match scope.outer {
                    Some(outer) => column_lookup(&requalified, name, outer),
                    None => Err(missing_from_clause_entry(qualifier)),
                };
            }
            matches
        }
    };
    let hits: Vec<(&ScopeRelation, Attnum)> = candidates
        .into_iter()
        .filter_map(|relation| relation.shape.column(name).map(|attnum| (relation, attnum)))
        .collect();
    match hits.as_slice() {
        [] => match (qualifier, scope.outer) {
            (None, Some(outer)) => column_lookup(qualifier, name, outer),
            _ => Err(undefined_column(qualifier, name)),
        },
        [(relation, attnum)] => {
            let (_, ty) = relation.shape.column_at(*attnum)?;
            Ok((relation.slot, *attnum, ty))
        }
        _ => Err(ambiguous_column(name)),
    }
}

pub(super) fn tableoid_lookup(
    qualifier: &Option<TableName>,
    scope: &Scope<'_, '_>,
) -> Result<Oid, HeadError> {
    let name = ColumnName::tableoid();
    let candidates: Vec<&ScopeRelation> = match qualifier {
        None => scope.relations.iter().collect(),
        Some(qualifier) => {
            let matches: Vec<&ScopeRelation> = scope
                .relations
                .iter()
                .filter(|relation| &relation.visible_as == qualifier)
                .collect();
            if matches.is_empty() {
                let requalified = Some(qualifier.clone());
                return match scope.outer {
                    Some(outer) => tableoid_lookup(&requalified, outer),
                    None => Err(missing_from_clause_entry(qualifier)),
                };
            }
            matches
        }
    };
    let hits: Vec<(&ScopeRelation, Oid)> = candidates
        .into_iter()
        .filter_map(|relation| match &relation.shape {
            RelationShape::Catalog(table) => Some((relation, table.oid)),
            RelationShape::Synthetic(_) => None,
        })
        .collect();
    match hits.as_slice() {
        [] => match (qualifier, scope.outer) {
            (None, Some(outer)) => tableoid_lookup(qualifier, outer),
            _ => Err(undefined_column(qualifier, &name)),
        },
        [(_, oid)] => Ok(*oid),
        _ => Err(ambiguous_column(&name)),
    }
}
