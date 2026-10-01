use std::collections::BTreeSet;

use crate::catalog::pg;
use crate::catalog::{Attnum, Grantee, Oid};
use crate::engine::constant;
use crate::engine::EngineConnection;
use crate::error::HeadError;
use crate::ident::PolicyName;

use super::super::rows::{insert_row, DependType, PolicyCommand, SharedDependType};
use super::dependencies::{write_depend_row, write_shdepend_row, Dependency};

/// Every `pg_policy` column for a `CREATE POLICY`: this head only ever
/// admits a permissive `SELECT` policy (`ARCH.md`'s scope), so `polcmd`
/// and `polpermissive` never vary; `polwithcheck` is `None` because a
/// `SELECT` policy has no `WITH CHECK` clause of its own in PostgreSQL
/// either.
fn policy_row(
    oid: Oid,
    name: &PolicyName,
    table: Oid,
    roles: &[Grantee],
    using_source: &str,
) -> pg::pg_policy::Row {
    let role_values: Vec<i64> = roles
        .iter()
        .map(|grantee| match grantee {
            Grantee::Public => 0,
            Grantee::Role(oid) => oid.as_i64(),
        })
        .collect();
    pg::pg_policy::Row {
        oid: oid.as_i64(),
        polname: name.as_str().to_string(),
        polrelid: table.as_i64(),
        polcmd: PolicyCommand::Select.code(),
        polpermissive: true,
        polroles: role_values,
        polqual: Some(using_source.to_string()),
        polwithcheck: None,
    }
}

pub(super) fn create_policy(
    connection: &EngineConnection,
    oid: Oid,
    name: &PolicyName,
    table: Oid,
    roles: &[Grantee],
    using_source: &str,
    referenced_columns: &BTreeSet<Attnum>,
) -> Result<(), HeadError> {
    let policy_oid = pg::pg_policy::TABLE.relation_oid();
    let class_oid = pg::pg_class::TABLE.relation_oid();
    let authid_oid = pg::pg_authid::TABLE.relation_oid();
    {
        let object = constant::ProjectObject::new(oid)?;
        insert_row(
            connection,
            object,
            &pg::pg_policy::TABLE,
            policy_row(oid, name, table, roles, using_source).into_cells()?,
        )?;
    }
    write_depend_row(
        connection,
        Dependency {
            classid: policy_oid,
            objid: oid,
            objsubid: 0,
            refclassid: class_oid,
            refobjid: table,
            refobjsubid: 0,
            deptype: DependType::Auto,
        },
    )?;
    for attnum in referenced_columns {
        let attnum_i64 = i64::try_from(attnum.get())
            .map_err(|_| HeadError::internal("an attnum fits in i64"))?;
        write_depend_row(
            connection,
            Dependency {
                classid: policy_oid,
                objid: oid,
                objsubid: 0,
                refclassid: class_oid,
                refobjid: table,
                refobjsubid: attnum_i64,
                deptype: DependType::Normal,
            },
        )?;
    }
    for grantee in roles {
        if let Grantee::Role(role_oid) = grantee {
            write_shdepend_row(
                connection,
                policy_oid.as_i64(),
                oid.as_i64(),
                authid_oid.as_i64(),
                role_oid.as_i64(),
                SharedDependType::Policy,
            )?;
        }
    }
    Ok(())
}
