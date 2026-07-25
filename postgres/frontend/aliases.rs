use chrono::Utc;
use std::sync::OnceLock;
use turso_core::Connection;
use turso_ext::{scalar, ExtensionApi, ScalarFunction, Value as ExtValue};

pub fn install(conn: &Connection) {
    conn.register_static_extension(register_pg_functions);
}

fn register_pg_functions(ext_api: &mut ExtensionApi) {
    unsafe {
        register_alias(ext_api, c"now".as_ptr(), postgres_frontend_now);
        register_alias(ext_api, c"clock_timestamp".as_ptr(), postgres_frontend_now);
        register_alias(
            ext_api,
            c"transaction_timestamp".as_ptr(),
            postgres_frontend_now,
        );
        register_alias(
            ext_api,
            c"statement_timestamp".as_ptr(),
            postgres_frontend_now,
        );
        register_alias(ext_api, c"version".as_ptr(), postgres_frontend_version);
    }
}

unsafe fn register_alias(
    ext_api: &mut ExtensionApi,
    name: *const std::ffi::c_char,
    func: ScalarFunction,
) {
    (ext_api.register_scalar_function)(ext_api.ctx, name, -1, false, 0, func, None, None);
}

#[scalar(name = "now")]
fn postgres_frontend_now(_args: &[ExtValue]) -> ExtValue {
    let now = Utc::now();
    let formatted = now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
    ExtValue::from_text(formatted)
}

/// The (major, minor) PostgreSQL version the embedded `pg_query`
/// (libpg_query) grammar implements, decoded from `PG_VERSION_NUM`
/// (https://www.postgresql.org/docs/current/libpq-status.html#LIBPQ-PQSERVERVERSION).
/// Read from a throwaway parse rather than a literal, so `version()` always
/// reflects whatever PostgreSQL grammar `pg_query` is actually built
/// against instead of a number we'd have to remember to update by hand.
fn embedded_postgres_version() -> (i32, i32) {
    static VERSION: OnceLock<(i32, i32)> = OnceLock::new();
    *VERSION.get_or_init(|| {
        let version_num = turso_pg_parser::parse("SELECT 1")
            .map(|result| result.protobuf.version)
            .unwrap_or(0);
        (version_num / 10000, version_num % 10000)
    })
}

#[scalar(name = "version")]
fn postgres_frontend_version(_args: &[ExtValue]) -> ExtValue {
    let (major, minor) = embedded_postgres_version();
    ExtValue::from_text(format!(
        "PostgreSQL {major}.{minor} on Turso {}",
        env!("CARGO_PKG_VERSION")
    ))
}
