use std::collections::BTreeSet;

use turso_core::Value;
use turso_parser::ast;

use crate::analyze::types::TypeHandle;
use crate::catalog::pg::{self, ColumnId};
use crate::catalog::{AclEntry, Attnum, Oid};
use crate::engine::constant;
use crate::engine::EngineConnection;
use crate::error::HeadError;
use crate::lower::sql::{self, Relation};

pub(super) enum RelKind {
    Relation,
    Index,
}

impl RelKind {
    pub(super) fn code(self) -> char {
        match self {
            RelKind::Relation => 'r',
            RelKind::Index => 'i',
        }
    }
}

pub(super) enum ReplicaIdentity {
    Default,
    Nothing,
}

impl ReplicaIdentity {
    pub(super) fn code(self) -> char {
        match self {
            ReplicaIdentity::Default => 'd',
            ReplicaIdentity::Nothing => 'n',
        }
    }
}

pub(super) enum ConstraintType {
    PrimaryKey,
    NotNull,
}

impl ConstraintType {
    pub(super) fn code(self) -> char {
        match self {
            ConstraintType::PrimaryKey => 'p',
            ConstraintType::NotNull => 'n',
        }
    }
}

pub(super) enum ReferentialAction {
    NotApplicable,
}

impl ReferentialAction {
    pub(super) fn code(self) -> char {
        match self {
            ReferentialAction::NotApplicable => ' ',
        }
    }
}

pub(super) enum DependType {
    Normal,
    Auto,
    Internal,
}

impl DependType {
    pub(super) fn code(self) -> char {
        match self {
            DependType::Normal => 'n',
            DependType::Auto => 'a',
            DependType::Internal => 'i',
        }
    }
}

pub(super) enum SharedDependType {
    Owner,
    Acl,
    Policy,
}

impl SharedDependType {
    pub(super) fn code(self) -> char {
        match self {
            SharedDependType::Owner => 'o',
            SharedDependType::Acl => 'a',
            SharedDependType::Policy => 'r',
        }
    }
}

pub(super) enum PolicyCommand {
    Select,
}

impl PolicyCommand {
    pub(super) fn code(self) -> char {
        match self {
            PolicyCommand::Select => 'r',
        }
    }
}

pub(super) fn table_exists(
    connection: &EngineConnection,
    table: &pg::Table,
) -> Result<bool, HeadError> {
    let mut params = Vec::new();
    let filter = sql::equals(
        sql::Column::EngineSchemaName,
        &mut params,
        text_value(&sql::engine_name(Relation::Table(table.relation_oid()))),
    )?;
    let rows = query(
        connection,
        sql::select(
            Relation::EngineSchema,
            &[sql::Column::EngineSchemaName],
            Some(filter),
            &[],
        ),
        params,
    )?;
    Ok(!rows.is_empty())
}

pub(super) fn stored_catalog_tables(
    connection: &EngineConnection,
) -> Result<BTreeSet<u32>, HeadError> {
    let engine_names = query(
        connection,
        sql::select(
            Relation::EngineSchema,
            &[sql::Column::EngineSchemaName],
            None,
            &[],
        ),
        Vec::new(),
    )?
    .iter()
    .map(|row| match row.as_slice() {
        [name] => text(name),
        _ => Err(malformed("the engine schema")),
    })
    .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(pg::tables()
        .iter()
        .filter(|table| {
            engine_names.contains(&sql::engine_name(Relation::Table(table.relation_oid())))
        })
        .map(|table| table.oid)
        .collect())
}

pub(crate) fn declared_columns(table: &pg::Table) -> Result<Vec<ast::ColumnDefinition>, HeadError> {
    table
        .columns
        .iter()
        .filter(|column| !column.is_system)
        .map(|column| {
            let engine = TypeHandle::by_oid(i64::from(column.type_oid))?.engine()?;
            let array = engine.array && i64::from(column.type_oid) != pg::types::ACLITEM_ARRAY_OID;
            Ok(sql::array_column(
                Attnum::try_from(column.attnum)?,
                engine.element,
                array,
                sql::ColumnConstraints {
                    primary_key: false,
                    not_null: column.not_null,
                    unique: false,
                },
            ))
        })
        .collect()
}

pub(super) fn ensure_table(
    connection: &EngineConnection,
    table: &pg::Table,
) -> Result<(), HeadError> {
    if table_exists(connection, table)? {
        return Ok(());
    }
    let columns = declared_columns(table)?;
    connection.write(
        sql::create_table(
            Relation::Table(table.relation_oid()),
            true,
            columns,
            Vec::new(),
        ),
        Vec::new(),
    )?;
    for index in pg::indexes_for(table).filter(|index| index.unique) {
        let columns: Vec<sql::Column> = index
            .key_attnums
            .iter()
            .map(|attnum| Ok(Attnum::try_from(*attnum)?.into()))
            .collect::<Result<Vec<_>, HeadError>>()?;
        connection.write(
            sql::create_index(
                Relation::Index(Oid::new(index.oid)),
                Relation::Table(table.relation_oid()),
                true,
                &columns,
            ),
            Vec::new(),
        )?;
    }
    Ok(())
}

fn column_names(table: &pg::Table) -> Result<Vec<sql::Column>, HeadError> {
    table
        .columns
        .iter()
        .filter(|column| !column.is_system)
        .map(|column| Ok(Attnum::try_from(column.attnum)?.into()))
        .collect()
}

pub(in crate::engine) fn row_cells(
    table: &pg::Table,
    row: &pg::Row,
) -> Result<Vec<sql::Cell>, HeadError> {
    table
        .columns
        .iter()
        .filter(|column| !column.is_system)
        .map(|column| {
            cell_from_pg_value(row.value(ColumnId {
                table_oid: table.oid,
                attnum: column.attnum,
                type_oid: column.type_oid,
            })?)
        })
        .collect()
}

fn cell_from_pg_value(value: pg::Value) -> Result<sql::Cell, HeadError> {
    match value {
        pg::Value::Null => Ok(sql::Cell::Scalar(Value::Null)),
        pg::Value::Bool(value) => Ok(sql::Cell::Scalar(Value::from_i64(i64::from(value)))),
        pg::Value::I16(value) => Ok(sql::Cell::Scalar(Value::from_i64(i64::from(value)))),
        pg::Value::I32(value) => Ok(sql::Cell::Scalar(Value::from_i64(i64::from(value)))),
        pg::Value::F32(value) => Ok(sql::Cell::Scalar(Value::from_f64(f64::from(value)))),
        pg::Value::Oid(value) => Ok(sql::Cell::Scalar(Value::from_i64(i64::from(value)))),
        pg::Value::Char(value) => Ok(sql::Cell::Scalar(code_value(char::from(value)))),
        pg::Value::Text(value) | pg::Value::Name(value) => Ok(sql::Cell::Scalar(text_value(value))),
        pg::Value::Acl(items) => {
            let elements: Result<Vec<_>, _> = items
                .iter()
                .map(|item| acl_item_value(AclEntry::from(*item)))
                .collect();
            Ok(sql::Cell::Array(elements?))
        }
        pg::Value::I16Array(items) => Ok(sql::Cell::Array(
            items
                .iter()
                .map(|item| Value::from_i64(i64::from(*item)))
                .collect(),
        )),
        pg::Value::OidArray(items) => Ok(sql::Cell::Array(
            items
                .iter()
                .map(|item| Value::from_i64(i64::from(*item)))
                .collect(),
        )),
    }
}

pub(super) fn insert_rows(
    connection: &EngineConnection,
    object: constant::ProjectObject,
    table: &pg::Table,
    or_ignore: bool,
    rows: Vec<Vec<sql::Cell>>,
) -> Result<(), HeadError> {
    let columns = column_names(table)?;
    insert_columns(connection, object, table, or_ignore, &columns, rows)
}

fn insert_columns(
    connection: &EngineConnection,
    object: constant::ProjectObject,
    table: &pg::Table,
    or_ignore: bool,
    columns: &[sql::Column],
    rows: Vec<Vec<sql::Cell>>,
) -> Result<(), HeadError> {
    ensure_table(connection, table)?;
    let (cmd, params) = sql::insert_cells(
        Relation::Table(table.relation_oid()),
        or_ignore,
        columns,
        rows,
    )?;
    write_project_row(connection, object, cmd, params)
}

pub(super) fn write_project_row(
    connection: &EngineConnection,
    object: constant::ProjectObject,
    cmd: ast::Cmd,
    params: Vec<Value>,
) -> Result<(), HeadError> {
    let _ = object;
    connection.write(cmd, params)
}

/// What every `pg_class` row, constant or project, says about storage the
/// head does not have: shared by `write::relations::class_row` (a project
/// row's own `Row` literal) and `engine::constant::class_deviations` (the
/// same nine columns patched onto a compiled-in constant row at read
/// time), so the two cannot drift apart into disagreeing about a built-in
/// object's own storage facts.
pub(in crate::engine) struct ClassStorageFacts {
    pub(in crate::engine) reltype: i64,
    pub(in crate::engine) reltoastrelid: i64,
    pub(in crate::engine) relallvisible: i64,
    pub(in crate::engine) relallfrozen: i64,
    pub(in crate::engine) relminmxid: i64,
    pub(in crate::engine) relfrozenxid: i64,
    pub(in crate::engine) relfilenode: i64,
    pub(in crate::engine) relpages: i64,
    pub(in crate::engine) reltuples: f64,
}

pub(in crate::engine) fn class_storage_facts(oid: Oid, is_index: bool) -> ClassStorageFacts {
    ClassStorageFacts {
        reltype: 0,
        reltoastrelid: 0,
        relallvisible: 0,
        relallfrozen: 0,
        relminmxid: 1,
        relfrozenxid: super::write::FIRST_NORMAL_TRANSACTION_ID,
        relfilenode: oid.as_i64(),
        relpages: i64::from(is_index),
        reltuples: if is_index { 0.0 } else { -1.0 },
    }
}

/// Every column of `table`'s own row, named once by the generated `Row`
/// struct `row` was built from (`Row::into_cells`): building that struct
/// is the only way to reach this function, so a column a caller forgot
/// fails to compile there rather than being silently left out of the
/// INSERT (and so out of the row, as NULL) here.
pub(super) fn insert_row(
    connection: &EngineConnection,
    object: constant::ProjectObject,
    table: &pg::Table,
    row: Vec<(ColumnId, sql::Cell)>,
) -> Result<(), HeadError> {
    let (columns, cells): (Vec<sql::Column>, Vec<sql::Cell>) = row
        .into_iter()
        .map(|(id, cell)| (sql::Column::from(id), cell))
        .unzip();
    insert_columns(connection, object, table, false, &columns, vec![cells])
}

pub(super) fn select_rows(
    connection: &EngineConnection,
    table: &pg::Table,
    columns: &[sql::Column],
    filter: Option<Box<ast::Expr>>,
    params: Vec<Value>,
    order_by: &[sql::Column],
) -> Result<Vec<Vec<Value>>, HeadError> {
    if !table_exists(connection, table)? {
        return Ok(Vec::new());
    }
    query(
        connection,
        sql::select(
            Relation::Table(table.relation_oid()),
            columns,
            filter,
            order_by,
        ),
        params,
    )
}

pub(super) fn read_array(
    connection: &EngineConnection,
    table: &pg::Table,
    column: ColumnId,
    key: &[(ColumnId, Value)],
) -> Result<Option<Vec<Value>>, HeadError> {
    if !table_exists(connection, table)? {
        return Ok(None);
    }
    let mut params = Vec::new();
    let conditions: Vec<ast::Expr> = key
        .iter()
        .map(|(id, value)| sql::equals(*id, &mut params, value.clone()).map(|expr| *expr))
        .collect::<Result<Vec<_>, _>>()?;
    let filter = sql::and_all(conditions);
    let relation = Relation::Table(table.relation_oid());

    let length_rows = query(
        connection,
        sql::select_exprs(
            relation,
            vec![*sql::array_length(column)],
            filter.clone(),
            &[],
        ),
        params.clone(),
    )?;
    let length = match length_rows.as_slice() {
        [row] => match row.as_slice() {
            [Value::Null] => return Ok(None),
            [value] => integer(value)?,
            _ => return Err(malformed(table.name)),
        },
        [] => return Ok(None),
        _ => return Err(malformed(table.name)),
    };

    let mut elements = Vec::with_capacity(usize::try_from(length.max(0)).unwrap_or(0));
    for index in 1..=length {
        let rows = query(
            connection,
            sql::select_exprs(
                relation,
                vec![*sql::array_element(column, index)],
                filter.clone(),
                &[],
            ),
            params.clone(),
        )?;
        match rows.as_slice() {
            [row] => match row.as_slice() {
                [value] => elements.push(value.clone()),
                _ => return Err(malformed(table.name)),
            },
            _ => return Err(malformed(table.name)),
        }
    }
    Ok(Some(elements))
}

pub(super) fn acl_item_value(entry: AclEntry) -> Result<Value, HeadError> {
    entry.to_value()
}

pub(super) fn decode_acl_element(value: &Value) -> Result<AclEntry, HeadError> {
    AclEntry::from_value(value).map_err(|_| malformed("an aclitem"))
}

pub(super) fn query(
    connection: &EngineConnection,
    cmd: ast::Cmd,
    params: Vec<Value>,
) -> Result<Vec<Vec<Value>>, HeadError> {
    connection.query(cmd, params)
}

pub(super) fn text_value(text: &str) -> Value {
    Value::from_text(text.to_string())
}

pub(super) fn code_value(code: char) -> Value {
    text_value(&code.to_string())
}

pub(super) fn text(value: &Value) -> Result<String, HeadError> {
    value
        .to_text()
        .map(str::to_string)
        .ok_or_else(|| HeadError::internal("a catalog text column held a non-text value"))
}

pub(super) fn integer(value: &Value) -> Result<i64, HeadError> {
    value
        .as_int()
        .ok_or_else(|| HeadError::internal("a catalog integer column held a non-integer value"))
}

pub(super) fn malformed(table: &str) -> HeadError {
    HeadError::internal(format!("a row in {table} has the wrong shape"))
}
