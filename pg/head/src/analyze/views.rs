use crate::analyze::types::{TypeHandle, INT4_OID, OID_OID, TEXT_ARRAY_OID, TEXT_OID};
use crate::catalog::pg::views as view_oids;
use crate::catalog::{Oid, POSTGRES_ROLE};
use crate::error::HeadError;
use crate::ident::{ColumnName, FunctionName, TableName};
use crate::parse::expr::{CompareOp, Expr, Literal};
use crate::parse::statement::{
    FromClause, FromItem, Query, RelationName, RelationSchema, SelectItem, SimpleSelect,
};
use crate::session::settings::SettingMeta;

pub(crate) struct ViewDef {
    pub(crate) oid: Oid,
    pub(crate) owner: Oid,
    pub(crate) query: Query,
}

/// Declares `CatalogView` and each variant's own table name from the same
/// list, so a view this head serves is always named in exactly one place;
/// `oid`/`definition` below are separate exhaustive matches (denied
/// `wildcard_enum_match_arm` means adding a variant without an arm in
/// either fails to compile), and `lookup`/`oids` are the only two places
/// that used to duplicate this set (an `if name == ...` chain, and a
/// hand-maintained OID array that could name a different set of views
/// than the chain did).
macro_rules! catalog_views {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum CatalogView {
            $($variant),+
        }

        impl CatalogView {
            pub(crate) const ALL: &'static [CatalogView] = &[$(CatalogView::$variant),+];

            fn table_name(self) -> TableName {
                match self {
                    $(CatalogView::$variant => TableName::literal($name)),+
                }
            }
        }
    };
}

catalog_views!(
    Roles => "pg_roles",
    Settings => "pg_settings",
    Seclabels => "pg_seclabels",
);

impl CatalogView {
    fn oid(self) -> u32 {
        match self {
            CatalogView::Roles => view_oids::PG_ROLES_OID,
            CatalogView::Settings => view_oids::PG_SETTINGS_OID,
            CatalogView::Seclabels => view_oids::PG_SECLABELS_OID,
        }
    }

    fn definition(self) -> Result<ViewDef, HeadError> {
        match self {
            CatalogView::Roles => pg_roles(),
            CatalogView::Settings => pg_settings(),
            CatalogView::Seclabels => pg_seclabels(),
        }
    }
}

pub(crate) fn lookup(name: &TableName) -> Result<Option<ViewDef>, HeadError> {
    match CatalogView::ALL
        .iter()
        .find(|view| view.table_name() == *name)
    {
        Some(view) => Ok(Some(view.definition()?)),
        None => Ok(None),
    }
}

/// Every catalog view's own oid, for `engine::store::load`'s ACL scan:
/// derived from `CatalogView::ALL` rather than a second, hand-maintained
/// OID list, so it can never name a different set of views than `lookup`
/// admits.
pub(crate) fn oids() -> impl Iterator<Item = u32> {
    CatalogView::ALL.iter().map(|view| view.oid())
}

fn column(table: &'static str, name: &'static str) -> Expr {
    Expr::Column(
        Some(TableName::literal(table)),
        ColumnName::literal(name),
        None,
    )
}

fn cast(expr: Expr, oid: i64) -> Result<Expr, HeadError> {
    Ok(Expr::Cast(Box::new(expr), TypeHandle::by_oid(oid)?))
}

fn text_literal(value: impl Into<String>) -> Expr {
    Expr::Literal(Literal::Text(value.into()), None)
}

fn null_of(oid: i64) -> Result<Expr, HeadError> {
    cast(Expr::Literal(Literal::Null, None), oid)
}

fn item(expr: Expr, alias: &'static str) -> SelectItem {
    SelectItem::Expr(expr, Some(ColumnName::literal(alias)))
}

fn simple_select(items: Vec<SelectItem>, from: Option<FromClause>) -> SimpleSelect {
    SimpleSelect {
        distinct: false,
        items,
        from,
        filter: None,
    }
}

fn single_arm_query(first: SimpleSelect) -> Query {
    Query {
        first,
        combined: Vec::new(),
        order_by: Vec::new(),
    }
}

fn pg_catalog_table(name: &'static str) -> FromClause {
    FromClause {
        first: FromItem::Table {
            relation: RelationName {
                name: TableName::literal(name),
                schema: RelationSchema::PgCatalog,
                location: None,
            },
            alias: None,
        },
        joins: Vec::new(),
    }
}

fn pg_roles() -> Result<ViewDef, HeadError> {
    let items = vec![
        item(column("pg_authid", "rolname"), "rolname"),
        item(column("pg_authid", "rolsuper"), "rolsuper"),
        item(column("pg_authid", "rolinherit"), "rolinherit"),
        item(column("pg_authid", "rolcreaterole"), "rolcreaterole"),
        item(column("pg_authid", "rolcreatedb"), "rolcreatedb"),
        item(column("pg_authid", "rolcanlogin"), "rolcanlogin"),
        item(column("pg_authid", "rolreplication"), "rolreplication"),
        item(column("pg_authid", "rolconnlimit"), "rolconnlimit"),
        item(text_literal("********"), "rolpassword"),
        item(column("pg_authid", "rolvaliduntil"), "rolvaliduntil"),
        item(column("pg_authid", "rolbypassrls"), "rolbypassrls"),
        item(null_of(TEXT_ARRAY_OID)?, "rolconfig"),
        item(column("pg_authid", "oid"), "oid"),
    ];
    let query = single_arm_query(simple_select(items, Some(pg_catalog_table("pg_authid"))));
    Ok(ViewDef {
        oid: Oid::new(CatalogView::Roles.oid()),
        owner: POSTGRES_ROLE,
        query,
    })
}

fn pg_seclabels() -> Result<ViewDef, HeadError> {
    let items = vec![
        item(null_of(OID_OID)?, "objoid"),
        item(null_of(OID_OID)?, "classoid"),
        item(null_of(INT4_OID)?, "objsubid"),
        item(null_of(TEXT_OID)?, "objtype"),
        item(null_of(OID_OID)?, "objnamespace"),
        item(null_of(TEXT_OID)?, "objname"),
        item(null_of(TEXT_OID)?, "provider"),
        item(null_of(TEXT_OID)?, "label"),
    ];
    let mut first = simple_select(items, None);
    first.filter = Some(Expr::Literal(Literal::Boolean(false), None));
    let query = single_arm_query(first);
    Ok(ViewDef {
        oid: Oid::new(CatalogView::Seclabels.oid()),
        owner: POSTGRES_ROLE,
        query,
    })
}

fn optional_text(value: Option<&'static str>) -> Result<Expr, HeadError> {
    match value {
        Some(value) => cast(text_literal(value), TEXT_OID),
        None => null_of(TEXT_OID),
    }
}

fn enumvals_array(values: Option<&'static [&'static str]>) -> Result<Expr, HeadError> {
    match values {
        Some(values) => cast(
            text_literal(format!("{{{}}}", values.join(","))),
            TEXT_ARRAY_OID,
        ),
        None => null_of(TEXT_ARRAY_OID),
    }
}

fn current_setting_call(name: &'static str) -> Expr {
    Expr::Call(
        FunctionName::literal("current_setting"),
        vec![text_literal(name)],
        None,
    )
}

fn setting_source(name: &'static str, boot_val: &'static str) -> Expr {
    Expr::Case {
        base: None,
        arms: vec![(
            Expr::Compare(
                CompareOp::Eq,
                Box::new(current_setting_call(name)),
                Box::new(text_literal(boot_val)),
                None,
            ),
            text_literal("default"),
        )],
        otherwise: Some(Box::new(text_literal("session"))),
    }
}

fn setting_row(meta: &'static SettingMeta) -> Result<SimpleSelect, HeadError> {
    let items = vec![
        item(cast(text_literal(meta.display_name), TEXT_OID)?, "name"),
        item(cast(current_setting_call(meta.name), TEXT_OID)?, "setting"),
        item(optional_text(meta.unit)?, "unit"),
        item(cast(text_literal(meta.category), TEXT_OID)?, "category"),
        item(cast(text_literal(meta.short_desc), TEXT_OID)?, "short_desc"),
        item(optional_text(meta.extra_desc)?, "extra_desc"),
        item(cast(text_literal("user"), TEXT_OID)?, "context"),
        item(cast(text_literal(meta.vartype), TEXT_OID)?, "vartype"),
        item(
            cast(setting_source(meta.name, meta.boot_val), TEXT_OID)?,
            "source",
        ),
        item(optional_text(meta.min_val)?, "min_val"),
        item(optional_text(meta.max_val)?, "max_val"),
        item(enumvals_array(meta.enumvals)?, "enumvals"),
        item(cast(text_literal(meta.boot_val), TEXT_OID)?, "boot_val"),
        item(
            cast(current_setting_call(meta.name), TEXT_OID)?,
            "reset_val",
        ),
        item(null_of(TEXT_OID)?, "sourcefile"),
        item(null_of(INT4_OID)?, "sourceline"),
        item(
            Expr::Literal(Literal::Boolean(false), None),
            "pending_restart",
        ),
    ];
    Ok(simple_select(items, None))
}

fn pg_settings() -> Result<ViewDef, HeadError> {
    let (first_meta, rest_meta) = crate::session::settings::SETTINGS
        .split_first()
        .ok_or_else(|| HeadError::internal("pg_settings models at least one setting"))?;
    let first = setting_row(first_meta)?;
    let combined = rest_meta
        .iter()
        .map(setting_row)
        .collect::<Result<Vec<_>, HeadError>>()?;
    let query = Query {
        first,
        combined,
        order_by: Vec::new(),
    };
    Ok(ViewDef {
        oid: Oid::new(CatalogView::Settings.oid()),
        owner: POSTGRES_ROLE,
        query,
    })
}
