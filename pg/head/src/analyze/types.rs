use crate::catalog::pg;
use crate::error::HeadError;
use crate::ident::TypeName;

pub(crate) use pg::types::{
    ACLITEM_ARRAY_OID, ANYARRAY_OID, ANYCOMPATIBLEARRAY_OID, ANYCOMPATIBLE_OID, ANYNONARRAY_OID,
    ANY_OID, BOOL_OID, CHAR_OID, FLOAT4_OID, INT2VECTOR_OID, INT2_ARRAY_OID, INT2_OID, INT4_OID,
    INT8_OID, NAME_OID, OIDVECTOR_OID, OID_ARRAY_OID, OID_OID, REGCLASS_OID, REGPROC_OID,
    TEXT_ARRAY_OID, TEXT_OID, XID_OID,
};

/// Every type the head builds a value of, declares a column with, casts to,
/// or renders specially: one variant per type, `facts()` exhaustive over
/// all of them so a new type cannot be added without deciding every fact
/// below for it. Replaces `DECLARABLE_COLUMN_TYPES`, `VALUE_CAST_TYPES`,
/// `VALUE_CAST_ARRAY_TYPES`, the raw-oid `format_type_name`/`btree_opclass`
/// lists and `catalog::pg::engine_type`'s flat match, all of which named
/// the same handful of types independently and could drift.
///
/// The enum, `ALL` and `oid` are declared together from one variant/oid
/// list (`supported_types!` below): `facts()` further down is its own
/// separate exhaustive match (denied `wildcard_enum_match_arm` already
/// forces it to grow with the enum), but nothing forced `ALL` to grow the
/// same way before this, since it was a hand-maintained second list of the
/// same variants that existed only for `TypeKind::of_oid`'s reverse
/// lookup.
macro_rules! supported_types {
    ($($variant:ident => $oid:expr),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum SupportedType {
            $($variant),+
        }

        impl SupportedType {
            const ALL: &'static [SupportedType] = &[$(SupportedType::$variant),+];

            pub(crate) fn oid(self) -> i64 {
                match self {
                    $(SupportedType::$variant => $oid),+
                }
            }
        }
    };
}

supported_types!(
    Bool => BOOL_OID,
    Int2 => INT2_OID,
    Int4 => INT4_OID,
    Int8 => INT8_OID,
    Oid => OID_OID,
    Xid => XID_OID,
    Char => CHAR_OID,
    Name => NAME_OID,
    Text => TEXT_OID,
    RegClass => REGCLASS_OID,
    RegProc => REGPROC_OID,
    Float4 => FLOAT4_OID,
    Int2Vector => INT2VECTOR_OID,
    OidVector => OIDVECTOR_OID,
    Int2Array => INT2_ARRAY_OID,
    Int4Array => pg::types::INT4_ARRAY_OID,
    OidArray => OID_ARRAY_OID,
    TextArray => TEXT_ARRAY_OID,
    CharArray => pg::types::CHAR_ARRAY_OID,
    NameArray => pg::types::NAME_ARRAY_OID,
    AclItemArray => ACLITEM_ARRAY_OID,
    AnyArray => ANYARRAY_OID,
);

/// A type the capture exposes but the head never produces a value of on
/// its own account: never declarable, castable or selectable.
/// `PgNodeTree`/`TimestampTz` are real columns of a served catalog table
/// (`pg_constraint.conbin`, `pg_authid.rolvaliduntil`) that exist only so
/// those tables can be materialized. The rest are never a served table's
/// own column, only ones `pg_attribute`'s bootstrap data describes for
/// every relation PostgreSQL 18's catalog carries, served or not
/// (`engine/store/load.rs`'s `load_columns` reads every captured
/// `pg_attribute` row, not only the ones this head materializes, so a
/// type used by an unserved system relation's column still needs a
/// handle to describe it): `Bytea`/`Tid`/`Cid` (`pg_largeobject`,
/// `pg_trigger`'s system columns), `Float4Array` (`pg_statistic`),
/// `Cstring` (`pg_proc`'s own bootstrap-only columns), `PgLsn`
/// (`pg_subscription`), `PgNdistinct`/`PgDependencies`/`PgMcvList`/
/// `PgStatisticArray` (`pg_statistic_ext_data`). `AclItem` is
/// `AclItemArray`'s own element type, needed only so
/// `parse::expr::array_cast`'s empty-array-literal fold
/// (`'{}'::aclitem[]`, `pg_dump`'s own `pg_default_acl` query) can name
/// it, never a column anywhere on its own. The four `Any*` variants are
/// `pg_proc.proargtypes` pseudo-type placeholders
/// (`analyze/functions.rs`'s polymorphic signature matching), never a
/// real column's type at all.
/// Declares `CatalogOnlyType`, `CatalogOnlyType::ALL` and
/// `CatalogOnlyType::oid` from the same variant/oid list, so a variant can
/// only ever be added with its own oid, in the same edit, to both the enum
/// and `ALL` (a hand-maintained second list of the variants, which existed
/// only to answer `of_oid`'s reverse lookup, could otherwise fall out of
/// sync with the enum silently: `wildcard_enum_match_arm` forces `oid`'s
/// own match to grow, but nothing forced `ALL` to).
macro_rules! catalog_only_types {
    ($($variant:ident => $oid:expr),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum CatalogOnlyType {
            $($variant),+
        }

        impl CatalogOnlyType {
            const ALL: &'static [CatalogOnlyType] = &[$(CatalogOnlyType::$variant),+];

            fn oid(self) -> i64 {
                match self {
                    $(CatalogOnlyType::$variant => $oid),+
                }
            }
        }
    };
}

catalog_only_types!(
    PgNodeTree => pg::types::PG_NODE_TREE_OID,
    TimestampTz => pg::types::TIMESTAMPTZ_OID,
    Bytea => pg::types::BYTEA_OID,
    Tid => pg::types::TID_OID,
    Cid => pg::types::CID_OID,
    Float4Array => pg::types::FLOAT4_ARRAY_OID,
    AclItem => pg::types::ACLITEM_OID,
    Cstring => pg::types::CSTRING_OID,
    PgLsn => pg::types::PG_LSN_OID,
    PgNdistinct => pg::types::PG_NDISTINCT_OID,
    PgDependencies => pg::types::PG_DEPENDENCIES_OID,
    PgMcvList => pg::types::PG_MCV_LIST_OID,
    PgStatisticArray => pg::types::PG_STATISTIC_ARRAY_OID,
    Any => ANY_OID,
    AnyNonArray => ANYNONARRAY_OID,
    AnyCompatible => ANYCOMPATIBLE_OID,
    AnyCompatibleArray => ANYCOMPATIBLEARRAY_OID,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeKind {
    Supported(SupportedType),
    CatalogOnly(CatalogOnlyType),
}

impl TypeKind {
    /// A reverse lookup over `SupportedType::ALL`/`CatalogOnlyType::ALL`
    /// rather than a second oid match: each variant's own oid
    /// (`SupportedType::oid`/`CatalogOnlyType::oid`) is the only place
    /// that number is written, so this cannot drift from it the way two
    /// independent oid lists could.
    fn of_oid(oid: u32) -> Option<TypeKind> {
        let oid = i64::from(oid);
        if let Some(supported) = SupportedType::ALL.iter().find(|s| s.oid() == oid).copied() {
            return Some(TypeKind::Supported(supported));
        }
        CatalogOnlyType::ALL
            .iter()
            .find(|c| c.oid() == oid)
            .copied()
            .map(TypeKind::CatalogOnly)
    }

    fn engine(self) -> Result<pg::EngineType, HeadError> {
        match self {
            TypeKind::Supported(supported) => Ok(supported.facts().engine),
            TypeKind::CatalogOnly(catalog_only) => catalog_only.engine(),
        }
    }
}

impl CatalogOnlyType {
    /// Only `PgNodeTree`/`TimestampTz` back a served table's own column
    /// and so ever need an engine storage mapping; every other
    /// catalog-only type describes an unserved relation purely as
    /// `pg_attribute` row data (`ColumnId`'s own captured fields), never
    /// as a table this head materializes, so asking one for engine
    /// storage is a bug in the caller, matching this crate's own
    /// `catalog::pg::engine_type` before this table replaced it (which
    /// had no mapping for any of these oids either).
    fn engine(self) -> Result<pg::EngineType, HeadError> {
        use pg::EngineStorageType::{Text, TimestampTz};
        let (element, array) = match self {
            CatalogOnlyType::PgNodeTree => (Text, false),
            CatalogOnlyType::TimestampTz => (TimestampTz, false),
            CatalogOnlyType::Bytea
            | CatalogOnlyType::Tid
            | CatalogOnlyType::Cid
            | CatalogOnlyType::Float4Array
            | CatalogOnlyType::AclItem
            | CatalogOnlyType::Cstring
            | CatalogOnlyType::PgLsn
            | CatalogOnlyType::PgNdistinct
            | CatalogOnlyType::PgDependencies
            | CatalogOnlyType::PgMcvList
            | CatalogOnlyType::PgStatisticArray
            | CatalogOnlyType::Any
            | CatalogOnlyType::AnyNonArray
            | CatalogOnlyType::AnyCompatible
            | CatalogOnlyType::AnyCompatibleArray => {
                return Err(HeadError::internal(format!(
                    "no engine type mapping for pg_type oid {}",
                    self.oid()
                )))
            }
        };
        Ok(pg::EngineType { element, array })
    }
}

/// How a text literal cast directly to this type folds at analysis
/// (`analyze/typing/coerce.rs`'s `coerce_scalar`), the one decision
/// `"char"` used to be excluded from by an `oid` comparison in
/// `analyze/typing/check.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiteralFold {
    /// Kept as text, validated as an integer literal of this type's own
    /// width (`coerce::parse_integer_text`): PostgreSQL keeps a
    /// numeric-targeted text literal as text too, probed (`pg_get_expr`
    /// shows `'22'::bigint`, never a bare `22`).
    Numeric,
    /// Kept as text, unchanged.
    Text,
    /// Truncated to its first byte, matching `"char"`'s own encoding
    /// (`core/schema.rs`'s `CREATE TYPE "char" ... ENCODE`), probed
    /// against PostgreSQL 18: `''::"char"` truncates to empty text (a
    /// leading NUL byte is `charout`'s own C-string terminator), any
    /// other byte truncates to a one-character string.
    Char,
    /// This type's own value never comes from folding a literal cast in
    /// this crate's admitted SQL (`RegClass`/`RegProc` text literals are
    /// resolved to a catalog oid or refused before reaching this fold,
    /// `analyze/walk/subquery.rs`'s `presolve_identifier_cast`; every
    /// other type here is never a value-cast target at all). Reaching
    /// this is a bug in the caller, not a value this crate's SQL surface
    /// can produce.
    NotFoldable,
}

/// Which `OutputSpec` a column or expression of this type renders through
/// (`analyze/select.rs`'s `output_spec`), replacing that function's own
/// flat oid match (including its `_ if ty.element() != 0 => Column`
/// catch-all, now every array variant's own fact).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextOutput {
    /// The physical engine value is already this column's final wire
    /// text (an integer rendered as digits, or already-formatted text).
    Column,
    Bool,
    Float4,
    RegClass,
    RegProc,
    /// `oidvector`/`int2vector`: PostgreSQL's own space-separated form.
    Vector,
    AclArray,
}

/// Every per-type decision `SupportedType` used to scatter across
/// `DECLARABLE_COLUMN_TYPES`, `VALUE_CAST_TYPES`, `VALUE_CAST_ARRAY_TYPES`,
/// `format_type_name`, `btree_opclass` and `catalog::pg::engine_type`, now
/// one struct literal per variant with no field defaulted (`facts()` never
/// uses `..`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TypeFacts {
    pub(crate) declarable_column: bool,
    pub(crate) value_castable: bool,
    pub(crate) btree_opclass: Option<i64>,
    pub(crate) engine: pg::EngineType,
    pub(crate) literal_fold: LiteralFold,
    pub(crate) text_output: TextOutput,
}

impl SupportedType {
    pub(crate) fn facts(self) -> TypeFacts {
        use pg::EngineStorageType::{Blob, Boolean, Char, Integer, Name, Real, SmallInt, Text};
        fn engine(element: pg::EngineStorageType, array: bool) -> pg::EngineType {
            pg::EngineType { element, array }
        }
        match self {
            SupportedType::Bool => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(Boolean, false),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Bool,
            },
            SupportedType::Int2 => TypeFacts {
                declarable_column: false,
                value_castable: true,
                btree_opclass: None,
                engine: engine(SmallInt, false),
                literal_fold: LiteralFold::Numeric,
                text_output: TextOutput::Column,
            },
            SupportedType::Int4 => TypeFacts {
                declarable_column: true,
                value_castable: true,
                btree_opclass: Some(1978),
                engine: engine(Integer, false),
                literal_fold: LiteralFold::Numeric,
                text_output: TextOutput::Column,
            },
            SupportedType::Int8 => TypeFacts {
                declarable_column: true,
                value_castable: true,
                btree_opclass: Some(3124),
                engine: engine(Integer, false),
                literal_fold: LiteralFold::Numeric,
                text_output: TextOutput::Column,
            },
            SupportedType::Oid => TypeFacts {
                declarable_column: false,
                value_castable: true,
                btree_opclass: None,
                engine: engine(Integer, false),
                literal_fold: LiteralFold::Numeric,
                text_output: TextOutput::Column,
            },
            SupportedType::Xid => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(Integer, false),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Column,
            },
            SupportedType::Char => TypeFacts {
                declarable_column: false,
                value_castable: true,
                btree_opclass: None,
                engine: engine(Char, false),
                literal_fold: LiteralFold::Char,
                text_output: TextOutput::Column,
            },
            SupportedType::Name => TypeFacts {
                declarable_column: false,
                value_castable: true,
                btree_opclass: None,
                engine: engine(Name, false),
                literal_fold: LiteralFold::Text,
                text_output: TextOutput::Column,
            },
            SupportedType::Text => TypeFacts {
                declarable_column: true,
                value_castable: true,
                btree_opclass: Some(3126),
                engine: engine(Text, false),
                literal_fold: LiteralFold::Text,
                text_output: TextOutput::Column,
            },
            SupportedType::RegClass => TypeFacts {
                declarable_column: false,
                value_castable: true,
                btree_opclass: None,
                engine: engine(Integer, false),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::RegClass,
            },
            SupportedType::RegProc => TypeFacts {
                declarable_column: false,
                value_castable: true,
                btree_opclass: None,
                engine: engine(Integer, false),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::RegProc,
            },
            SupportedType::Float4 => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(Real, false),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Float4,
            },
            SupportedType::Int2Vector => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(SmallInt, true),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Vector,
            },
            SupportedType::OidVector => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(Integer, true),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Vector,
            },
            SupportedType::Int2Array => TypeFacts {
                declarable_column: false,
                value_castable: true,
                btree_opclass: None,
                engine: engine(SmallInt, true),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Column,
            },
            SupportedType::Int4Array => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(Integer, true),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Column,
            },
            SupportedType::OidArray => TypeFacts {
                declarable_column: false,
                value_castable: true,
                btree_opclass: None,
                engine: engine(Integer, true),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Column,
            },
            SupportedType::TextArray => TypeFacts {
                declarable_column: false,
                value_castable: true,
                btree_opclass: None,
                engine: engine(Text, true),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Column,
            },
            SupportedType::CharArray => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(Char, true),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Column,
            },
            SupportedType::NameArray => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(Name, true),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Column,
            },
            SupportedType::AclItemArray => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(Blob, true),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::AclArray,
            },
            SupportedType::AnyArray => TypeFacts {
                declarable_column: false,
                value_castable: false,
                btree_opclass: None,
                engine: engine(Blob, false),
                literal_fold: LiteralFold::NotFoldable,
                text_output: TextOutput::Column,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TypeHandle {
    oid: u32,
    name: &'static str,
    len: i16,
    align: pg::Alignment,
    storage: pg::Storage,
    category: u8,
    by_val: bool,
    collation: u32,
    element: u32,
    array: u32,
    kind: TypeKind,
}

impl TypeHandle {
    pub(crate) fn by_oid(oid: i64) -> Result<TypeHandle, HeadError> {
        let oid = u32::try_from(oid)
            .map_err(|_| HeadError::internal(format!("type oid {oid} does not fit in u32")))?;
        let kind = TypeKind::of_oid(oid).ok_or_else(|| {
            HeadError::internal(format!(
                "type oid {oid} is not a type this head supports or reads from its own catalog"
            ))
        })?;
        pg::type_rows()
            .iter()
            .find(|row| row.oid(pg::pg_type::OID) == Ok(oid))
            .ok_or_else(|| HeadError::internal(format!("no pg_type row for oid {oid}")))
            .and_then(|row| from_row(row, kind))
    }

    pub(crate) fn by_name(name: &TypeName) -> Option<TypeHandle> {
        let row = pg::type_rows()
            .iter()
            .find(|row| row.text(pg::pg_type::TYPNAME) == Ok(name.as_str()))?;
        let oid = row.oid(pg::pg_type::OID).ok()?;
        let kind = TypeKind::of_oid(oid)?;
        from_row(row, kind).ok()
    }

    pub(crate) fn oid(self) -> i64 {
        i64::from(self.oid)
    }

    pub(crate) fn name(self) -> &'static str {
        self.name
    }

    pub(crate) fn len(self) -> i64 {
        i64::from(self.len)
    }

    pub(crate) fn align(self) -> pg::Alignment {
        self.align
    }

    pub(crate) fn storage(self) -> pg::Storage {
        self.storage
    }

    pub(crate) fn category(self) -> u8 {
        self.category
    }

    pub(crate) fn by_val(self) -> bool {
        self.by_val
    }

    pub(crate) fn collation(self) -> i64 {
        i64::from(self.collation)
    }

    pub(crate) fn element(self) -> u32 {
        self.element
    }

    pub(crate) fn array_type(self) -> u32 {
        self.array
    }

    pub(crate) fn engine(self) -> Result<pg::EngineType, HeadError> {
        self.kind.engine()
    }

    /// `None` for a catalog-only type: it is never declarable, castable or
    /// selectable, so every fact-driven predicate below answers "no"
    /// rather than asking `SupportedType` a question it has no variant
    /// for.
    fn facts(self) -> Option<TypeFacts> {
        match self.kind {
            TypeKind::Supported(supported) => Some(supported.facts()),
            TypeKind::CatalogOnly(_) => None,
        }
    }

    pub(crate) fn is_declarable_column(self) -> bool {
        self.facts().is_some_and(|facts| facts.declarable_column)
    }

    pub(crate) fn is_value_castable(self) -> bool {
        self.facts().is_some_and(|facts| facts.value_castable)
    }

    pub(crate) fn btree_opclass(self) -> Option<i64> {
        self.facts().and_then(|facts| facts.btree_opclass)
    }

    pub(crate) fn literal_fold(self) -> LiteralFold {
        self.facts()
            .map_or(LiteralFold::NotFoldable, |facts| facts.literal_fold)
    }

    pub(crate) fn text_output(self) -> Option<TextOutput> {
        self.facts().map(|facts| facts.text_output)
    }

    /// Whether PostgreSQL 18 implicitly casts a value of this type to
    /// `target` (`pg_cast.castcontext = 'i'`, captured in
    /// `capture/out/casts.json`): the fact `analyze/typing/check.rs`'s
    /// common-type merges (`case_anchor_merge`, `array_literal_element_type`)
    /// read to order same-category types, replacing a hand-ranked tier.
    pub(crate) fn implicitly_casts_to(self, target: TypeHandle) -> bool {
        pg::implicit_cast_exists(self.oid, target.oid)
    }

    pub(crate) fn display_name(self) -> &'static str {
        match self.oid() {
            INT4_OID => "integer",
            INT8_OID => "bigint",
            INT2_OID => "smallint",
            TEXT_OID => "text",
            _ => self.name,
        }
    }
}

fn from_row(row: &pg::Row, kind: TypeKind) -> Result<TypeHandle, HeadError> {
    Ok(TypeHandle {
        oid: row.oid(pg::pg_type::OID)?,
        name: row.text(pg::pg_type::TYPNAME)?,
        len: row.i16_value(pg::pg_type::TYPLEN)?,
        align: pg::Alignment::from_code(row.char_value(pg::pg_type::TYPALIGN)?)?,
        storage: pg::Storage::from_code(row.char_value(pg::pg_type::TYPSTORAGE)?)?,
        category: row.char_value(pg::pg_type::TYPCATEGORY)?,
        by_val: row.bool_value(pg::pg_type::TYPBYVAL)?,
        collation: row.oid(pg::pg_type::TYPCOLLATION)?,
        element: row.oid(pg::pg_type::TYPELEM)?,
        array: row.oid(pg::pg_type::TYPARRAY)?,
        kind,
    })
}

/// PostgreSQL's own `format_type()` synonym table: a fixed, small set of
/// base types `format_type_internal` (probed on a live PostgreSQL 18)
/// prints under a standard SQL name rather than their catalog `typname`
/// (`bool` as `boolean`, `"char"` always quoted since it collides with the
/// `char`/`character` keyword, and so on). Every other captured type
/// prints its own `typname` unchanged, so only this handful is listed;
/// `format_type_name` below never repeats a `typname` the row already
/// carries.
fn format_type_synonym(oid: i64) -> Option<&'static str> {
    match oid {
        BOOL_OID => Some("boolean"),
        INT2_OID => Some("smallint"),
        INT4_OID => Some("integer"),
        INT8_OID => Some("bigint"),
        CHAR_OID => Some("\"char\""),
        FLOAT4_OID => Some("real"),
        ANY_OID => Some("\"any\""),
        _ if oid == pg::types::FLOAT8_OID => Some("double precision"),
        _ if oid == pg::types::TIMESTAMPTZ_OID => Some("timestamp with time zone"),
        _ => None,
    }
}

/// Any captured `pg_type` row, supported or not: `format_type()` (the SQL
/// builtin, `FunctionHandle::FormatType`) is handed an arbitrary runtime
/// oid value from row data, which PostgreSQL answers for any type it
/// knows about, not only the ones this head declares, casts or selects.
/// This is the one place that reads a `pg_type` row without going through
/// `TypeHandle`'s closed, supported-or-catalog-only gate.
fn any_captured_type_row(oid: i64) -> Option<&'static pg::Row> {
    let oid = u32::try_from(oid).ok()?;
    pg::type_rows()
        .iter()
        .find(|row| row.oid(pg::pg_type::OID) == Ok(oid))
}

pub(crate) fn format_type_name(oid: i64) -> String {
    let Some(row) = any_captured_type_row(oid) else {
        return "???".to_string();
    };
    // A nonzero `typelem` alone does not mean "prints as `element[]`":
    // `name`, `int2vector` and `oidvector` all point `typelem` at their
    // own storage element too, but PostgreSQL prints each of those under
    // its own name, never bracketed (probed: `format_type` on all three
    // is bare). The bracketed, true array types are exactly the ones
    // PostgreSQL stores `TOAST`-able (`typstorage = 'x'`, probed
    // alongside every other captured type); `name`/`int2vector`/
    // `oidvector` are all fixed, non-`TOAST`-able (`typstorage = 'p'`).
    let element = row.oid(pg::pg_type::TYPELEM).unwrap_or(0);
    let storage = row.char_value(pg::pg_type::TYPSTORAGE).unwrap_or(0);
    if element != 0 && char::from(storage) == pg::Storage::Extended.code() {
        return format!("{}[]", format_type_name(i64::from(element)));
    }
    format_type_synonym(oid)
        .map(str::to_string)
        .unwrap_or_else(|| {
            row.text(pg::pg_type::TYPNAME)
                .map_or_else(|_| "???".to_string(), str::to_string)
        })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;

    #[test]
    fn each_accepted_alias_resolves_to_its_pg_type_oid() {
        for (alias, oid) in [("int4", INT4_OID), ("int8", INT8_OID), ("text", TEXT_OID)] {
            let handle = TypeHandle::by_name(&TypeName::literal(alias))
                .expect("the alias is a captured pg_type row");
            assert_eq!(handle.oid(), oid);
            assert!(handle.is_declarable_column());
        }
    }

    #[test]
    fn a_catalog_only_type_has_a_handle_but_is_not_declarable() {
        let name = TypeHandle::by_name(&TypeName::literal("name"))
            .expect("\"name\" is a captured pg_type row");
        assert_eq!(name.oid(), 19);
        assert!(!name.is_declarable_column());
    }

    #[test]
    fn an_unregistered_type_name_resolves_to_no_handle() {
        assert!(TypeHandle::by_name(&TypeName::literal("frobozz")).is_none());
    }

    #[test]
    fn a_captured_but_unsupported_type_oid_has_no_handle() {
        // `numeric` (1700) is captured (it is a real column of some served
        // relation's dependency chain in `types.json`) but this head never
        // declares, casts or selects it; only `format_type_name` (which
        // reads the row directly) still names it.
        assert!(TypeHandle::by_oid(1700).is_err());
        assert_eq!(format_type_name(1700), "numeric");
    }

    #[test]
    fn format_type_name_matches_postgres_18() {
        assert_eq!(format_type_name(INT4_OID), "integer");
        assert_eq!(format_type_name(INT8_OID), "bigint");
        assert_eq!(format_type_name(TEXT_OID), "text");
        assert_eq!(format_type_name(NAME_OID), "name");
        assert_eq!(format_type_name(CHAR_OID), "\"char\"");
        assert_eq!(format_type_name(BOOL_OID), "boolean");
        assert_eq!(format_type_name(pg::types::FLOAT8_OID), "double precision");
        assert_eq!(format_type_name(TEXT_ARRAY_OID), "text[]");
        assert_eq!(format_type_name(999_999), "???");
    }

    #[test]
    fn every_supported_type_s_own_oid_resolves_back_to_it() {
        // Guards `SupportedType::oid()` and `TypeKind::of_oid` against
        // drifting apart: each variant's own oid must resolve back to the
        // same variant, the self-consistency a single scattered oid list
        // could never check itself against.
        for &supported in SupportedType::ALL {
            let oid = u32::try_from(supported.oid()).expect("every supported oid fits in u32");
            assert_eq!(TypeKind::of_oid(oid), Some(TypeKind::Supported(supported)));
        }
    }

    #[test]
    fn by_oid_finds_the_same_row_as_by_name() {
        let by_name = TypeHandle::by_name(&TypeName::literal("int4")).expect("int4 is captured");
        let by_oid = TypeHandle::by_oid(INT4_OID).expect("23 is int4's oid");
        assert_eq!(by_name.oid(), by_oid.oid());
        assert_eq!(by_name.name(), by_oid.name());
    }
}
