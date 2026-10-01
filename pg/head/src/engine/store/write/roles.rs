use crate::catalog::pg;
use crate::catalog::Oid;
use crate::engine::constant;
use crate::engine::EngineConnection;
use crate::error::HeadError;
use crate::ident::RoleName;

use super::super::rows::insert_row;

/// Every `pg_authid` column for a `CREATE ROLE`: only `LOGIN` is an
/// admitted role attribute (`ARCH.md`'s scope), so every other privilege
/// bit is always off and there is never a password or expiry to store.
fn authid_row(oid: Oid, name: &RoleName, can_login: bool) -> pg::pg_authid::Row {
    pg::pg_authid::Row {
        oid: oid.as_i64(),
        rolname: name.as_str().to_string(),
        rolsuper: false,
        rolinherit: true,
        rolcreaterole: false,
        rolcreatedb: false,
        rolcanlogin: can_login,
        rolreplication: false,
        rolbypassrls: false,
        rolconnlimit: -1,
        rolpassword: None,
        rolvaliduntil: None,
    }
}

pub(super) fn create_role(
    connection: &EngineConnection,
    oid: Oid,
    name: &RoleName,
    can_login: bool,
) -> Result<(), HeadError> {
    let object = constant::ProjectObject::new(oid)?;
    insert_row(
        connection,
        object,
        &pg::pg_authid::TABLE,
        authid_row(oid, name, can_login).into_cells()?,
    )
}
