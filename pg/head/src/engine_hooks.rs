use turso_core::dialect::sqlite;
use turso_core::schema::{BTreeTable, Schema};
use turso_core::{Connection, Dialect, Func, LimboError, Value};
use turso_parser::ast;

use crate::analyze::pg_options_to_table;
use crate::engine::constant_table;

const NAME: &str = "pg_head";

pub(crate) struct EngineHooks;

impl Dialect for EngineHooks {
    fn name(&self) -> &'static str {
        NAME
    }

    fn parse(&self, sql: &str) -> turso_core::Result<(Option<ast::Cmd>, usize)> {
        sqlite::parse(sql)
    }

    fn parse_table_sql(&self, sql: &str, root_page: i64) -> turso_core::Result<BTreeTable> {
        BTreeTable::from_sql(sql, root_page)
    }

    fn parse_table_sql_ast(&self, sql: &str) -> turso_core::Result<ast::Stmt> {
        sqlite::parse_table_sql_ast(sql)
    }

    fn table_sql_for_replay(&self, sql: &str) -> turso_core::Result<String> {
        sqlite::table_sql_for_replay(sql)
    }

    fn format_table_sql(
        &self,
        input: &str,
        _tbl_name: &ast::QualifiedName,
        _body: &ast::CreateTableBody,
    ) -> turso_core::Result<String> {
        Ok(input.to_string())
    }

    fn resolve_function(&self, name: &str, arg_count: usize) -> turso_core::Result<Option<Func>> {
        if crate::analyze::functions::FunctionHandle::lookup_engine_scalar(name, arg_count)
            .is_some()
        {
            return Ok(Some(Func::Dialect(name.to_string())));
        }
        sqlite::resolve_builtin_function(name, arg_count)
    }

    fn exec_scalar_function(
        &self,
        _conn: &Connection,
        name: &str,
        args: &[Value],
    ) -> turso_core::Result<Value> {
        let handle =
            crate::analyze::functions::FunctionHandle::lookup_engine_scalar(name, args.len())
                .ok_or_else(|| LimboError::ParseError(format!("no such function: {name}")))?;
        crate::analyze::functions::exec_pure(handle, args)
            .map_err(|error| LimboError::InternalError(error.to_string()))
    }

    fn requires_custom_types(&self) -> bool {
        true
    }

    fn register_catalog(
        &self,
        schema: &mut Schema,
        enable_custom_types: bool,
    ) -> turso_core::Result<()> {
        sqlite::register_builtin_catalog(schema, enable_custom_types)?;
        constant_table::register_all(schema)?;
        pg_options_to_table::register(schema)?;
        Ok(())
    }
}
