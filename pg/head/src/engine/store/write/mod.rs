use turso_core::Value;

use crate::catalog::CatalogWrite;
use crate::engine::EngineConnection;
use crate::error::HeadError;
use crate::lower::sql;

use super::STATE_TABLE;

mod acls;
mod dependencies;
mod indexes;
mod policies;
mod relations;
mod roles;

pub(in crate::engine) const FIRST_NORMAL_TRANSACTION_ID: i64 = 3;

pub(crate) fn apply(connection: &EngineConnection, write: CatalogWrite) -> Result<(), HeadError> {
    match write {
        CatalogWrite::CreateTable {
            oid,
            name,
            owner,
            columns,
            primary_key,
            not_null_constraints,
        } => relations::create_table(
            connection,
            oid,
            &name,
            owner,
            &columns,
            primary_key,
            &not_null_constraints,
        ),
        CatalogWrite::CreateRole {
            oid,
            name,
            can_login,
        } => roles::create_role(connection, oid, &name, can_login),
        CatalogWrite::CreatePolicy {
            oid,
            name,
            table,
            roles,
            using,
            referenced_columns,
        } => policies::create_policy(
            connection,
            oid,
            &name,
            table,
            &roles,
            &using,
            &referenced_columns,
        ),
        CatalogWrite::SetRowSecurity {
            table,
            enabled,
            forced,
        } => relations::set_row_security(connection, table, enabled, forced),
        CatalogWrite::ReplaceAcl {
            kind,
            object,
            owner,
            entries,
        } => acls::replace_acl(connection, kind, object, owner, &entries),
        CatalogWrite::SetNextOid(next) => {
            let mut params = Vec::new();
            let sets = vec![(
                sql::Column::HeadState(sql::HeadStateColumn::NextOid),
                sql::bind(&mut params, Value::from_i64(next.as_i64()))?,
            )];
            let filter = sql::equals(
                sql::Column::HeadState(sql::HeadStateColumn::Id),
                &mut params,
                Value::from_i64(1),
            )?;
            connection.write(sql::update(STATE_TABLE, sets, filter), params)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use std::collections::BTreeSet;

    use turso_core::Value;

    use super::super::rows::{decode_acl_element, integer, read_array, text};
    use super::super::tests::{
        notes_columns, one_row, rows_matching, test_connection, text_handle,
    };
    use super::super::PROOF;
    use super::*;
    use crate::catalog::pg;
    use crate::catalog::{AclEntry, Grantee, NewNotNullConstraint, NewPrimaryKey, Oid};
    use crate::ident::{ColumnName, ConstraintName, PolicyName, TableName};
    use crate::parse::statement::ColumnDef;

    #[test]
    fn create_table_by_a_superuser_matches_the_postgres_18_capture() {
        let connection = test_connection();
        let oid = Oid::new(16386);
        let owner = Oid::new(10);
        let primary_key = Some(NewPrimaryKey {
            name: ConstraintName::from_catalog(PROOF, "notes_pkey"),
            index_oid: Oid::new(16391),
            constraint_oid: Oid::new(16392),
            attnum: 1,
        });
        // Oids 16387/16388 and every column below are a live PostgreSQL
        // 18.6's own `SELECT * FROM pg_constraint WHERE contype = 'n'`
        // after this same `CREATE TABLE`.
        let not_null_constraints = vec![
            NewNotNullConstraint {
                name: ConstraintName::from_catalog(PROOF, "notes_id_not_null"),
                constraint_oid: Oid::new(16387),
                attnum: 1,
            },
            NewNotNullConstraint {
                name: ConstraintName::from_catalog(PROOF, "notes_owner_not_null"),
                constraint_oid: Oid::new(16388),
                attnum: 2,
            },
        ];
        apply(
            &connection,
            CatalogWrite::CreateTable {
                oid,
                name: TableName::from_catalog(PROOF, "notes"),
                owner,
                columns: notes_columns(),
                primary_key,
                not_null_constraints,
            },
        )
        .expect("creating the table succeeds");

        let class = &pg::pg_class::TABLE;
        let rows = rows_matching(
            &connection,
            class,
            &[
                pg::pg_class::RELNAME.into(),
                pg::pg_class::RELNAMESPACE.into(),
                pg::pg_class::RELKIND.into(),
                pg::pg_class::RELNATTS.into(),
                pg::pg_class::RELHASINDEX.into(),
                pg::pg_class::RELAM.into(),
                pg::pg_class::RELPERSISTENCE.into(),
                pg::pg_class::RELISSHARED.into(),
                pg::pg_class::RELTYPE.into(),
                pg::pg_class::RELTOASTRELID.into(),
                pg::pg_class::RELACL.into(),
                pg::pg_class::RELREPLIDENT.into(),
            ],
            pg::pg_class::OID,
            Value::from_i64(oid.as_i64()),
        );
        let row = match rows.as_slice() {
            [row] => row.clone(),
            other => panic!("expected exactly one pg_class row, found {}", other.len()),
        };
        assert_eq!(text(&row[0]).unwrap(), "notes");
        assert_eq!(integer(&row[1]).unwrap(), 2200);
        assert_eq!(text(&row[2]).unwrap(), "r");
        assert_eq!(integer(&row[3]).unwrap(), 3);
        assert_eq!(integer(&row[4]).unwrap(), 1);
        assert_eq!(integer(&row[5]).unwrap(), 2);
        assert_eq!(text(&row[6]).unwrap(), "p");
        assert_eq!(integer(&row[7]).unwrap(), 0);
        assert_eq!(
            integer(&row[8]).unwrap(),
            0,
            "no composite row type: reltype 0"
        );
        assert_eq!(
            integer(&row[9]).unwrap(),
            0,
            "no TOAST table: reltoastrelid 0"
        );
        assert!(
            matches!(row[10], Value::Null),
            "relacl is NULL until a GRANT"
        );
        assert_eq!(text(&row[11]).unwrap(), "d");

        let attribute = &pg::pg_attribute::TABLE;
        let mut attributes = rows_matching(
            &connection,
            attribute,
            &[
                pg::pg_attribute::ATTNAME.into(),
                pg::pg_attribute::ATTTYPID.into(),
                pg::pg_attribute::ATTLEN.into(),
                pg::pg_attribute::ATTALIGN.into(),
                pg::pg_attribute::ATTSTORAGE.into(),
                pg::pg_attribute::ATTCOLLATION.into(),
            ],
            pg::pg_attribute::ATTRELID,
            Value::from_i64(oid.as_i64()),
        );
        attributes.sort_by(|a, b| text(&a[0]).unwrap().cmp(&text(&b[0]).unwrap()));
        let id = attributes
            .iter()
            .find(|row| text(&row[0]).unwrap() == "id")
            .expect("the id column is described");
        assert_eq!(integer(&id[1]).unwrap(), 23);
        assert_eq!(integer(&id[2]).unwrap(), 4);
        assert_eq!(text(&id[3]).unwrap(), "i");
        assert_eq!(text(&id[4]).unwrap(), "p");
        assert_eq!(integer(&id[5]).unwrap(), 0);
        let body = attributes
            .iter()
            .find(|row| text(&row[0]).unwrap() == "body")
            .expect("the body column is described");
        assert_eq!(integer(&body[1]).unwrap(), 25);
        assert_eq!(integer(&body[2]).unwrap(), -1);
        assert_eq!(text(&body[3]).unwrap(), "i");
        assert_eq!(text(&body[4]).unwrap(), "x");
        assert_eq!(integer(&body[5]).unwrap(), 100);

        let index = &pg::pg_index::TABLE;
        let index_row = one_row(
            &connection,
            index,
            &[
                pg::pg_index::INDEXRELID.into(),
                pg::pg_index::INDRELID.into(),
                pg::pg_index::INDISUNIQUE.into(),
                pg::pg_index::INDISPRIMARY.into(),
            ],
        );
        assert_eq!(integer(&index_row[0]).unwrap(), 16391);
        assert_eq!(integer(&index_row[1]).unwrap(), oid.as_i64());
        assert_eq!(integer(&index_row[2]).unwrap(), 1);
        assert_eq!(integer(&index_row[3]).unwrap(), 1);
        let indkey = read_array(
            &connection,
            index,
            pg::pg_index::INDKEY,
            &[(pg::pg_index::INDEXRELID, Value::from_i64(16391))],
        )
        .expect("reading indkey succeeds")
        .expect("indkey is set");
        assert_eq!(
            indkey
                .iter()
                .map(|value| integer(value).unwrap())
                .collect::<Vec<_>>(),
            vec![1]
        );
        let indclass = read_array(
            &connection,
            index,
            pg::pg_index::INDCLASS,
            &[(pg::pg_index::INDEXRELID, Value::from_i64(16391))],
        )
        .expect("reading indclass succeeds")
        .expect("indclass is set");
        assert_eq!(
            indclass
                .iter()
                .map(|value| integer(value).unwrap())
                .collect::<Vec<_>>(),
            vec![1978]
        );

        let constraint = &pg::pg_constraint::TABLE;
        let pkey_rows = rows_matching(
            &connection,
            constraint,
            &[
                pg::pg_constraint::CONNAME.into(),
                pg::pg_constraint::CONTYPE.into(),
                pg::pg_constraint::CONRELID.into(),
                pg::pg_constraint::CONINDID.into(),
            ],
            pg::pg_constraint::OID,
            Value::from_i64(16392),
        );
        let constraint_row = match pkey_rows.as_slice() {
            [row] => row.clone(),
            other => panic!(
                "expected exactly one pg_constraint row, found {}",
                other.len()
            ),
        };
        assert_eq!(text(&constraint_row[0]).unwrap(), "notes_pkey");
        assert_eq!(text(&constraint_row[1]).unwrap(), "p");
        assert_eq!(integer(&constraint_row[2]).unwrap(), oid.as_i64());
        assert_eq!(integer(&constraint_row[3]).unwrap(), 16391);
        let conkey = read_array(
            &connection,
            constraint,
            pg::pg_constraint::CONKEY,
            &[(pg::pg_constraint::OID, Value::from_i64(16392))],
        )
        .expect("reading conkey succeeds")
        .expect("conkey is set");
        assert_eq!(
            conkey
                .iter()
                .map(|value| integer(value).unwrap())
                .collect::<Vec<_>>(),
            vec![1]
        );

        let depend = &pg::pg_depend::TABLE;
        let namespace_oid = pg::pg_namespace::TABLE.relation_oid().as_i64();
        let class_oid = pg::pg_class::TABLE.relation_oid().as_i64();
        let constraint_oid_class = pg::pg_constraint::TABLE.relation_oid().as_i64();
        let table_depends = rows_matching(
            &connection,
            depend,
            &[
                pg::pg_depend::CLASSID.into(),
                pg::pg_depend::OBJID.into(),
                pg::pg_depend::REFCLASSID.into(),
                pg::pg_depend::REFOBJID.into(),
                pg::pg_depend::REFOBJSUBID.into(),
                pg::pg_depend::DEPTYPE.into(),
            ],
            pg::pg_depend::OBJID,
            Value::from_i64(oid.as_i64()),
        );
        assert!(table_depends.iter().any(|row| {
            integer(&row[0]).unwrap() == class_oid
                && integer(&row[2]).unwrap() == namespace_oid
                && integer(&row[3]).unwrap() == 2200
                && text(&row[5]).unwrap() == "n"
        }));
        let constraint_depends = rows_matching(
            &connection,
            depend,
            &[
                pg::pg_depend::CLASSID.into(),
                pg::pg_depend::OBJID.into(),
                pg::pg_depend::REFCLASSID.into(),
                pg::pg_depend::REFOBJID.into(),
                pg::pg_depend::REFOBJSUBID.into(),
                pg::pg_depend::DEPTYPE.into(),
            ],
            pg::pg_depend::OBJID,
            Value::from_i64(16392),
        );
        assert!(constraint_depends.iter().any(|row| {
            integer(&row[0]).unwrap() == constraint_oid_class
                && integer(&row[2]).unwrap() == class_oid
                && integer(&row[3]).unwrap() == oid.as_i64()
                && integer(&row[4]).unwrap() == 1
                && text(&row[5]).unwrap() == "a"
        }));
        for (constraint_oid, name, attnum) in [
            (16387, "notes_id_not_null", 1),
            (16388, "notes_owner_not_null", 2),
        ] {
            let rows = rows_matching(
                &connection,
                constraint,
                &[
                    pg::pg_constraint::CONNAME.into(),
                    pg::pg_constraint::CONNAMESPACE.into(),
                    pg::pg_constraint::CONTYPE.into(),
                    pg::pg_constraint::CONRELID.into(),
                    pg::pg_constraint::CONINDID.into(),
                    pg::pg_constraint::CONENFORCED.into(),
                    pg::pg_constraint::CONVALIDATED.into(),
                    pg::pg_constraint::CONISLOCAL.into(),
                    pg::pg_constraint::CONNOINHERIT.into(),
                ],
                pg::pg_constraint::OID,
                Value::from_i64(constraint_oid),
            );
            let row = match rows.as_slice() {
                [row] => row,
                other => panic!(
                    "expected exactly one pg_constraint row for {name}, found {}",
                    other.len()
                ),
            };
            assert_eq!(text(&row[0]).unwrap(), name);
            assert_eq!(integer(&row[1]).unwrap(), 2200, "connamespace: public");
            assert_eq!(text(&row[2]).unwrap(), "n", "contype: not-null");
            assert_eq!(integer(&row[3]).unwrap(), oid.as_i64());
            assert_eq!(integer(&row[4]).unwrap(), 0, "conindid: no backing index");
            assert_eq!(integer(&row[5]).unwrap(), 1, "conenforced");
            assert_eq!(integer(&row[6]).unwrap(), 1, "convalidated");
            assert_eq!(integer(&row[7]).unwrap(), 1, "conislocal");
            assert_eq!(
                integer(&row[8]).unwrap(),
                0,
                "connoinherit: false, unlike a primary key's"
            );
            let conkey = read_array(
                &connection,
                constraint,
                pg::pg_constraint::CONKEY,
                &[(pg::pg_constraint::OID, Value::from_i64(constraint_oid))],
            )
            .expect("reading conkey succeeds")
            .expect("conkey is set");
            assert_eq!(
                conkey
                    .iter()
                    .map(|value| integer(value).unwrap())
                    .collect::<Vec<_>>(),
                vec![attnum]
            );
            let depends = rows_matching(
                &connection,
                depend,
                &[
                    pg::pg_depend::CLASSID.into(),
                    pg::pg_depend::OBJID.into(),
                    pg::pg_depend::OBJSUBID.into(),
                    pg::pg_depend::REFCLASSID.into(),
                    pg::pg_depend::REFOBJID.into(),
                    pg::pg_depend::REFOBJSUBID.into(),
                    pg::pg_depend::DEPTYPE.into(),
                ],
                pg::pg_depend::OBJID,
                Value::from_i64(constraint_oid),
            );
            assert!(depends.iter().any(|row| {
                integer(&row[0]).unwrap() == constraint_oid_class
                    && integer(&row[2]).unwrap() == 0
                    && integer(&row[3]).unwrap() == class_oid
                    && integer(&row[4]).unwrap() == oid.as_i64()
                    && integer(&row[5]).unwrap() == attnum
                    && text(&row[6]).unwrap() == "a"
            }));
        }

        let index_depends = rows_matching(
            &connection,
            depend,
            &[
                pg::pg_depend::CLASSID.into(),
                pg::pg_depend::OBJID.into(),
                pg::pg_depend::REFCLASSID.into(),
                pg::pg_depend::REFOBJID.into(),
                pg::pg_depend::DEPTYPE.into(),
            ],
            pg::pg_depend::OBJID,
            Value::from_i64(16391),
        );
        assert!(index_depends.iter().any(|row| {
            integer(&row[0]).unwrap() == class_oid
                && integer(&row[2]).unwrap() == constraint_oid_class
                && integer(&row[3]).unwrap() == 16392
                && text(&row[4]).unwrap() == "i"
        }));

        let shdepend = &pg::pg_shdepend::TABLE;
        let owner_depends = rows_matching(
            &connection,
            shdepend,
            &[pg::pg_shdepend::OBJID.into()],
            pg::pg_shdepend::OBJID,
            Value::from_i64(oid.as_i64()),
        );
        assert!(
            owner_depends.is_empty(),
            "postgres (oid 10) is pinned: no owner shdepend row"
        );
    }

    #[test]
    fn create_table_by_a_non_superuser_writes_an_owner_shdepend_row() {
        let connection = test_connection();
        let oid = Oid::new(16400);
        let owner = Oid::new(16384);
        apply(
            &connection,
            CatalogWrite::CreateTable {
                oid,
                name: TableName::from_catalog(PROOF, "plain"),
                owner,
                columns: vec![ColumnDef {
                    name: ColumnName::from_catalog(PROOF, "a"),
                    ty: text_handle(),
                    primary_key: None,
                    not_null: false,
                }],
                primary_key: None,
                not_null_constraints: Vec::new(),
            },
        )
        .expect("creating the table succeeds");

        let shdepend = &pg::pg_shdepend::TABLE;
        let authid_oid = pg::pg_authid::TABLE.relation_oid().as_i64();
        let class_oid = pg::pg_class::TABLE.relation_oid().as_i64();
        let rows = rows_matching(
            &connection,
            shdepend,
            &[
                pg::pg_shdepend::CLASSID.into(),
                pg::pg_shdepend::REFCLASSID.into(),
                pg::pg_shdepend::REFOBJID.into(),
                pg::pg_shdepend::DEPTYPE.into(),
            ],
            pg::pg_shdepend::OBJID,
            Value::from_i64(oid.as_i64()),
        );
        assert!(rows.iter().any(|row| {
            integer(&row[0]).unwrap() == class_oid
                && integer(&row[1]).unwrap() == authid_oid
                && integer(&row[2]).unwrap() == owner.as_i64()
                && text(&row[3]).unwrap() == "o"
        }));
    }

    #[test]
    fn grant_writes_relacl() {
        let connection = test_connection();
        let oid = Oid::new(16386);
        let owner = Oid::new(10);
        apply(
            &connection,
            CatalogWrite::CreateTable {
                oid,
                name: TableName::from_catalog(PROOF, "notes"),
                owner,
                columns: notes_columns(),
                primary_key: None,
                not_null_constraints: Vec::new(),
            },
        )
        .expect("creating the table succeeds");

        let granted_role = Oid::new(16384);
        apply(
            &connection,
            CatalogWrite::ReplaceAcl {
                kind: crate::security::privileges::ObjectKind::Table,
                object: oid,
                owner,
                entries: vec![
                    AclEntry {
                        grantee: Grantee::Role(owner),
                        grantor: owner,
                        privileges: crate::security::privileges::Privileges::all_on(
                            crate::security::privileges::ObjectKind::Table,
                        ),
                    },
                    AclEntry {
                        grantee: Grantee::Role(granted_role),
                        grantor: owner,
                        privileges: crate::security::privileges::Privileges::from_bits(1 << 1),
                    },
                ],
            },
        )
        .expect("granting succeeds");

        let entries = read_array(
            &connection,
            &pg::pg_class::TABLE,
            pg::pg_class::RELACL,
            &[(pg::pg_class::OID, Value::from_i64(oid.as_i64()))],
        )
        .expect("reading relacl succeeds")
        .expect("relacl is now set")
        .iter()
        .map(decode_acl_element)
        .collect::<Result<Vec<_>, _>>()
        .expect("every element decodes");
        assert!(entries
            .iter()
            .any(|entry| entry.grantee == Grantee::Role(granted_role)
                && entry.grantor == owner
                && entry.privileges == crate::security::privileges::Privileges::from_bits(1 << 1)));
    }

    #[test]
    fn create_policy_writes_polroles_with_zero_for_public() {
        let connection = test_connection();
        let oid = Oid::new(16386);
        let owner = Oid::new(10);
        apply(
            &connection,
            CatalogWrite::CreateTable {
                oid,
                name: TableName::from_catalog(PROOF, "notes"),
                owner,
                columns: notes_columns(),
                primary_key: None,
                not_null_constraints: Vec::new(),
            },
        )
        .expect("creating the table succeeds");

        apply(
            &connection,
            CatalogWrite::CreatePolicy {
                oid: Oid::new(16398),
                name: PolicyName::from_catalog(PROOF, "p2"),
                table: oid,
                roles: vec![Grantee::Public],
                using: "false".to_string(),
                referenced_columns: BTreeSet::new(),
            },
        )
        .expect("creating the policy succeeds");

        let roles = read_array(
            &connection,
            &pg::pg_policy::TABLE,
            pg::pg_policy::POLROLES,
            &[(pg::pg_policy::OID, Value::from_i64(16398))],
        )
        .expect("reading polroles succeeds")
        .expect("polroles is set");
        assert_eq!(
            roles
                .iter()
                .map(|value| integer(value).unwrap())
                .collect::<Vec<_>>(),
            vec![0]
        );
    }

    #[test]
    fn an_insert_aimed_at_a_constant_object_is_refused() {
        let connection = test_connection();
        let result = apply(
            &connection,
            CatalogWrite::CreateTable {
                oid: Oid::new(1259),
                name: TableName::from_catalog(PROOF, "evil"),
                owner: Oid::new(10),
                columns: notes_columns(),
                primary_key: None,
                not_null_constraints: Vec::new(),
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn an_update_aimed_at_a_constant_object_is_refused() {
        let connection = test_connection();
        let result = apply(
            &connection,
            CatalogWrite::SetRowSecurity {
                table: Oid::new(1259),
                enabled: true,
                forced: false,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn an_update_and_delete_aimed_at_a_constant_object_are_refused() {
        let connection = test_connection();
        let result = apply(
            &connection,
            CatalogWrite::ReplaceAcl {
                kind: crate::security::privileges::ObjectKind::Table,
                object: Oid::new(1259),
                owner: Oid::new(10),
                entries: Vec::new(),
            },
        );
        assert!(result.is_err());
    }
}
