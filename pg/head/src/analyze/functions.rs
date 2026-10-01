use crate::analyze::types::{
    TypeHandle, ANYARRAY_OID, ANYCOMPATIBLEARRAY_OID, ANYCOMPATIBLE_OID, ANYNONARRAY_OID, ANY_OID,
    CHAR_OID, INT4_OID, NAME_OID, TEXT_ARRAY_OID, TEXT_OID,
};
use crate::catalog::pg;
use crate::catalog::Oid;
use crate::error::{HeadError, NotSupportedFeature};
use crate::ident::FunctionName;

/// Declares `FunctionHandle` and `FunctionHandle::ALL` from the same list
/// of variant names, so the two cannot drift the way a hand-maintained
/// `ALL` array (a second, independent list of the same names) could: a
/// variant added to one is a variant added to both, in the same edit.
macro_rules! function_handles {
    ($($variant:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum FunctionHandle {
            $($variant),+
        }

        impl FunctionHandle {
            pub(crate) const ALL: &'static [FunctionHandle] = &[
                $(FunctionHandle::$variant),+
            ];
        }
    };
}

function_handles!(
    CurrentDatabase,
    CurrentSchemas,
    CurrentSetting,
    SetConfig,
    CurrentSettingMissingOk,
    PgIsInRecovery,
    Unnest,
    PgOptionsToTable,
    GenerateSeries,
    HeapTableamHandler,
    Bthandler,
    Hashhandler,
    Gisthandler,
    Ginhandler,
    Brinhandler,
    Spghandler,
    ArrayUpper,
    ArrayRemove,
    ArrayToString,
    QuoteIdent,
    QuoteLiteral,
    FormatType,
    AclDefault,
    PgGetTriggerdef,
    PgGetConstraintdef,
    PgGetConstraintdefPretty,
    PgGetIndexdef,
    PgGetIndexdefColumn,
    PgGetExpr,
    ArrayAgg,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Evaluation {
    Engine,
    HeadEngine,
    Aggregate,
    Session,
    CatalogRendered,
    SetReturning,
}

pub(crate) struct TableFunctionShape {
    pub(crate) default_name: &'static str,
    pub(crate) columns: Vec<(&'static str, TypeHandle)>,
}

impl FunctionHandle {
    pub(crate) fn oid(self) -> Oid {
        Oid::new(match self {
            FunctionHandle::CurrentDatabase => 861u32,
            FunctionHandle::CurrentSchemas => 1403,
            FunctionHandle::CurrentSetting => 2077,
            FunctionHandle::SetConfig => 2078,
            FunctionHandle::CurrentSettingMissingOk => 3294,
            FunctionHandle::PgIsInRecovery => 3810,
            FunctionHandle::PgOptionsToTable => 2289,
            FunctionHandle::Unnest => 2331,
            FunctionHandle::GenerateSeries => 1067,
            FunctionHandle::HeapTableamHandler => 3,
            FunctionHandle::Bthandler => 330,
            FunctionHandle::Hashhandler => 331,
            FunctionHandle::Gisthandler => 332,
            FunctionHandle::Ginhandler => 333,
            FunctionHandle::Spghandler => 334,
            FunctionHandle::Brinhandler => 335,
            FunctionHandle::ArrayUpper => 2092,
            FunctionHandle::ArrayRemove => 3167,
            FunctionHandle::ArrayToString => 395,
            FunctionHandle::QuoteIdent => 1282,
            FunctionHandle::QuoteLiteral => 1283,
            FunctionHandle::FormatType => 1081,
            FunctionHandle::AclDefault => 3943,
            FunctionHandle::PgGetTriggerdef => 2730,
            FunctionHandle::PgGetConstraintdef => 1387,
            FunctionHandle::PgGetConstraintdefPretty => 2508,
            FunctionHandle::PgGetIndexdef => 1643,
            FunctionHandle::PgGetIndexdefColumn => 2507,
            FunctionHandle::PgGetExpr => 1716,
            FunctionHandle::ArrayAgg => 2335,
        })
    }

    pub(crate) fn evaluation(self) -> Evaluation {
        match self {
            FunctionHandle::CurrentDatabase
            | FunctionHandle::CurrentSchemas
            | FunctionHandle::CurrentSetting
            | FunctionHandle::SetConfig
            | FunctionHandle::CurrentSettingMissingOk
            | FunctionHandle::PgIsInRecovery => Evaluation::Session,
            FunctionHandle::Unnest
            | FunctionHandle::PgOptionsToTable
            | FunctionHandle::GenerateSeries
            | FunctionHandle::HeapTableamHandler
            | FunctionHandle::Bthandler
            | FunctionHandle::Hashhandler
            | FunctionHandle::Gisthandler
            | FunctionHandle::Ginhandler
            | FunctionHandle::Brinhandler
            | FunctionHandle::Spghandler => Evaluation::SetReturning,
            FunctionHandle::QuoteIdent
            | FunctionHandle::QuoteLiteral
            | FunctionHandle::FormatType
            | FunctionHandle::AclDefault
            | FunctionHandle::PgGetTriggerdef => Evaluation::HeadEngine,
            FunctionHandle::ArrayUpper
            | FunctionHandle::ArrayRemove
            | FunctionHandle::ArrayToString => Evaluation::Engine,
            FunctionHandle::PgGetConstraintdef
            | FunctionHandle::PgGetConstraintdefPretty
            | FunctionHandle::PgGetIndexdef
            | FunctionHandle::PgGetIndexdefColumn
            | FunctionHandle::PgGetExpr => Evaluation::CatalogRendered,
            FunctionHandle::ArrayAgg => Evaluation::Aggregate,
        }
    }

    pub(crate) fn engine_name(self) -> Result<&'static str, HeadError> {
        self.row()?.text(pg::pg_proc::PRONAME)
    }

    pub(crate) fn lookup_engine_scalar(name: &str, arg_count: usize) -> Option<FunctionHandle> {
        let arg_count = i16::try_from(arg_count).ok()?;
        pg::proc_rows()
            .iter()
            .find(|row| {
                row.text(pg::pg_proc::PRONAME) == Ok(name)
                    && row.i16_value(pg::pg_proc::PRONARGS) == Ok(arg_count)
            })
            .and_then(|row| row.oid(pg::pg_proc::OID).ok())
            .and_then(FunctionHandle::from_oid)
            .filter(|handle| handle.evaluation() == Evaluation::HeadEngine)
    }

    pub(crate) fn lookup_table_function(
        name: &FunctionName,
        arg_count: usize,
    ) -> Option<FunctionHandle> {
        let arg_count = i16::try_from(arg_count).ok()?;
        pg::proc_rows()
            .iter()
            .find(|row| {
                row.text(pg::pg_proc::PRONAME) == Ok(name.as_str())
                    && row.bool_value(pg::pg_proc::PRORETSET) == Ok(true)
                    && row.i16_value(pg::pg_proc::PRONARGS) == Ok(arg_count)
            })
            .and_then(|row| row.oid(pg::pg_proc::OID).ok())
            .and_then(FunctionHandle::from_oid)
            .filter(|handle| {
                matches!(
                    handle,
                    FunctionHandle::Unnest
                        | FunctionHandle::PgOptionsToTable
                        | FunctionHandle::GenerateSeries
                )
            })
    }

    pub(crate) fn table_function_shape(
        self,
        args: &[TypeHandle],
    ) -> Result<TableFunctionShape, HeadError> {
        match self {
            FunctionHandle::Unnest => {
                let [array] = args else {
                    return Err(HeadError::internal(
                        "unnest is registered with one argument",
                    ));
                };
                let element = array.element();
                if element == 0 {
                    return Err(crate::analyze::typing::undefined_function("unnest", args));
                }
                let element = TypeHandle::by_oid(i64::from(element))?;
                Ok(TableFunctionShape {
                    default_name: "unnest",
                    columns: vec![("unnest", element)],
                })
            }
            FunctionHandle::PgOptionsToTable => {
                let [options] = args else {
                    return Err(HeadError::internal(
                        "pg_options_to_table is registered with one argument",
                    ));
                };
                if options.oid() != TEXT_ARRAY_OID {
                    return Err(crate::analyze::typing::undefined_function(
                        "pg_options_to_table",
                        args,
                    ));
                }
                let text = TypeHandle::by_oid(TEXT_OID)?;
                Ok(TableFunctionShape {
                    default_name: "pg_options_to_table",
                    columns: vec![("option_name", text), ("option_value", text)],
                })
            }
            FunctionHandle::GenerateSeries => {
                let [start, stop] = args else {
                    return Err(HeadError::internal(
                        "generate_series is registered with two arguments",
                    ));
                };
                if start.oid() != INT4_OID || stop.oid() != INT4_OID {
                    return Err(crate::analyze::typing::undefined_function(
                        "generate_series",
                        args,
                    ));
                }
                let int4 = TypeHandle::by_oid(INT4_OID)?;
                Ok(TableFunctionShape {
                    default_name: "generate_series",
                    columns: vec![("generate_series", int4)],
                })
            }
            FunctionHandle::CurrentDatabase
            | FunctionHandle::CurrentSchemas
            | FunctionHandle::CurrentSetting
            | FunctionHandle::SetConfig
            | FunctionHandle::CurrentSettingMissingOk
            | FunctionHandle::PgIsInRecovery
            | FunctionHandle::HeapTableamHandler
            | FunctionHandle::Bthandler
            | FunctionHandle::Hashhandler
            | FunctionHandle::Gisthandler
            | FunctionHandle::Ginhandler
            | FunctionHandle::Brinhandler
            | FunctionHandle::Spghandler
            | FunctionHandle::ArrayUpper
            | FunctionHandle::ArrayRemove
            | FunctionHandle::ArrayToString
            | FunctionHandle::QuoteIdent
            | FunctionHandle::QuoteLiteral
            | FunctionHandle::FormatType
            | FunctionHandle::AclDefault
            | FunctionHandle::PgGetTriggerdef
            | FunctionHandle::PgGetConstraintdef
            | FunctionHandle::PgGetConstraintdefPretty
            | FunctionHandle::PgGetIndexdef
            | FunctionHandle::PgGetIndexdefColumn
            | FunctionHandle::PgGetExpr
            | FunctionHandle::ArrayAgg => Err(HeadError::internal(
                "this function handle is not registered as a table function",
            )),
        }
    }

    pub(crate) fn from_oid(oid: u32) -> Option<FunctionHandle> {
        FunctionHandle::ALL
            .iter()
            .find(|handle| handle.oid() == Oid::new(oid))
            .copied()
    }

    fn row(self) -> Result<&'static pg::Row, HeadError> {
        let oid = self.oid().get();
        pg::proc_rows()
            .iter()
            .find(|row| row.oid(pg::pg_proc::OID) == Ok(oid))
            .ok_or_else(|| HeadError::internal("every FunctionHandle names a captured pg_proc row"))
    }

    pub(crate) fn result_type(self) -> Result<TypeHandle, HeadError> {
        let rettype = self.row()?.oid(pg::pg_proc::PRORETTYPE)?;
        TypeHandle::by_oid(i64::from(rettype))
    }

    pub(crate) fn arg_count(self) -> Result<usize, HeadError> {
        let nargs = self.row()?.i16_value(pg::pg_proc::PRONARGS)?;
        usize::try_from(nargs)
            .map_err(|_| HeadError::internal(format!("pronargs {nargs} does not fit in usize")))
    }

    pub(crate) fn lookup(name: &FunctionName, args: &[TypeHandle]) -> Option<FunctionHandle> {
        pg::proc_rows()
            .iter()
            .find(|row| {
                row.text(pg::pg_proc::PRONAME) == Ok(name.as_str()) && signature_matches(row, args)
            })
            .and_then(|row| row.oid(pg::pg_proc::OID).ok())
            .and_then(FunctionHandle::from_oid)
    }

    /// PostgreSQL only fixes a literal `NULL` argument's type once a
    /// candidate overload is chosen (`parse_coerce.c`'s `UNKNOWNOID`
    /// handling), not before; `args` names each argument's own type where
    /// it has one, `None` where the caller found a literal `NULL`. Used
    /// only as a fallback once `lookup` (which the vast majority of calls,
    /// having no literal `NULL` argument, resolve through unchanged)
    /// fails to find a match.
    pub(crate) fn lookup_deferring_nulls(
        name: &FunctionName,
        args: &[Option<TypeHandle>],
    ) -> Option<FunctionHandle> {
        pg::proc_rows()
            .iter()
            .find(|row| {
                row.text(pg::pg_proc::PRONAME) == Ok(name.as_str())
                    && signature_matches_deferring_nulls(row, args)
            })
            .and_then(|row| row.oid(pg::pg_proc::OID).ok())
            .and_then(FunctionHandle::from_oid)
    }

    /// This handle's own declared parameter types, to coerce a deferred
    /// `NULL` argument (see `lookup_deferring_nulls`) to once it is known.
    pub(crate) fn declared_arg_types(self) -> Result<Vec<TypeHandle>, HeadError> {
        row_arg_types(self.row()?)
            .ok_or_else(|| {
                HeadError::internal("every FunctionHandle names its own declared argument types")
            })?
            .iter()
            .map(|&oid| TypeHandle::by_oid(i64::from(oid)))
            .collect()
    }
}

fn signature_matches(row: &pg::Row, args: &[TypeHandle]) -> bool {
    let args: Vec<Option<TypeHandle>> = args.iter().map(|&ty| Some(ty)).collect();
    signature_matches_deferring_nulls(row, &args)
}

/// The one argument-matching walk both `lookup` and `lookup_deferring_nulls`
/// share: a `None` argument (a literal `NULL`, not yet coerced to any
/// type) matches any declared parameter, since PostgreSQL's own `UNKNOWN`
/// literal does too.
fn signature_matches_deferring_nulls(row: &pg::Row, args: &[Option<TypeHandle>]) -> bool {
    let Some(declared) = row_arg_types(row) else {
        return false;
    };
    if declared.len() != args.len() {
        return false;
    }
    let mut compatible_group: Option<i64> = None;
    for (&declared_oid, arg) in declared.iter().zip(args) {
        let Some(arg) = arg else {
            continue;
        };
        let declared_oid = i64::from(declared_oid);
        let matches = if declared_oid == ANYARRAY_OID {
            arg.element() != 0
        } else if declared_oid == ANYNONARRAY_OID {
            !(arg.element() != 0 && arg.len() == -1)
        } else if declared_oid == ANY_OID {
            true
        } else if declared_oid == ANYCOMPATIBLEARRAY_OID {
            let element = i64::from(arg.element());
            element != 0 && *compatible_group.get_or_insert(element) == element
        } else if declared_oid == ANYCOMPATIBLE_OID {
            *compatible_group.get_or_insert(arg.oid()) == arg.oid()
        } else if declared_oid == CHAR_OID {
            arg.oid() == CHAR_OID || arg.oid() == TEXT_OID
        } else if declared_oid == TEXT_OID {
            arg.oid() == TEXT_OID || arg.oid() == NAME_OID
        } else {
            u32::try_from(arg.oid()) == u32::try_from(declared_oid)
        };
        if !matches {
            return false;
        }
    }
    true
}

fn row_arg_types(row: &pg::Row) -> Option<Vec<u32>> {
    match row.value(pg::pg_proc::PROARGTYPES).ok()? {
        pg::Value::OidArray(items) => Some(items.to_vec()),
        pg::Value::Null
        | pg::Value::Bool(_)
        | pg::Value::I16(_)
        | pg::Value::I32(_)
        | pg::Value::F32(_)
        | pg::Value::Oid(_)
        | pg::Value::Char(_)
        | pg::Value::Text(_)
        | pg::Value::Name(_)
        | pg::Value::Acl(_)
        | pg::Value::I16Array(_) => None,
    }
}

fn text_argument(value: &turso_core::Value) -> Result<&str, HeadError> {
    value
        .to_text()
        .ok_or_else(|| HeadError::internal("a text-typed function argument was not text"))
}

fn char_argument_byte(value: &turso_core::Value) -> Result<u8, HeadError> {
    match value {
        turso_core::Value::Blob(bytes) => bytes.as_slice().first().copied().ok_or_else(|| {
            HeadError::not_supported(NotSupportedFeature::AclDefaultEmptyObjectType)
        }),
        other @ turso_core::Value::Null
        | other @ turso_core::Value::Numeric(_)
        | other @ turso_core::Value::Text(_) => {
            text_argument(other)?.bytes().next().ok_or_else(|| {
                HeadError::not_supported(NotSupportedFeature::AclDefaultEmptyObjectType)
            })
        }
    }
}

pub(crate) fn exec_pure(
    handle: FunctionHandle,
    args: &[turso_core::Value],
) -> Result<turso_core::Value, HeadError> {
    use turso_core::Value;
    match handle {
        FunctionHandle::QuoteIdent => match args.first() {
            None | Some(Value::Null) => Ok(Value::Null),
            Some(other) => Ok(Value::from_text(
                crate::render::ident::render_identifier(text_argument(other)?)
                    .as_sql()
                    .to_string(),
            )),
        },
        FunctionHandle::QuoteLiteral => match args.first() {
            None | Some(Value::Null) => Ok(Value::Null),
            Some(other) => Ok(Value::from_text(
                crate::render::ident::render_portable_literal(text_argument(other)?)
                    .as_sql()
                    .to_string(),
            )),
        },
        FunctionHandle::FormatType => match args.first() {
            None | Some(Value::Null) => Ok(Value::Null),
            Some(value) => {
                let oid = value
                    .as_int()
                    .ok_or_else(|| HeadError::internal("a type oid argument was not an integer"))?;
                Ok(Value::from_text(crate::analyze::types::format_type_name(
                    oid,
                )))
            }
        },
        FunctionHandle::AclDefault => {
            let (Some(objtype), Some(owner)) = (args.first(), args.get(1)) else {
                return Err(HeadError::internal(
                    "acldefault is registered with two arguments",
                ));
            };
            if matches!(objtype, Value::Null) || matches!(owner, Value::Null) {
                return Ok(Value::Null);
            }
            let letter = char_argument_byte(objtype)?;
            let owner_oid = owner
                .as_int()
                .ok_or_else(|| HeadError::internal("acldefault's owner argument was not an oid"))?;
            let entries =
                crate::security::privileges::default_acl_entries(letter, Oid::from_i64(owner_oid)?)
                    .ok_or_else(|| {
                        HeadError::not_supported(NotSupportedFeature::AclDefaultObjectType)
                    })?;
            let elements = entries
                .into_iter()
                .map(crate::catalog::AclEntry::to_value)
                .collect::<Result<Vec<_>, _>>()?;
            turso_core::encode_array(&elements)
                .map_err(|error| HeadError::internal(error.to_string()))
        }
        FunctionHandle::PgGetTriggerdef => Ok(Value::Null),
        FunctionHandle::PgGetConstraintdef
        | FunctionHandle::PgGetConstraintdefPretty
        | FunctionHandle::PgGetIndexdef
        | FunctionHandle::PgGetIndexdefColumn
        | FunctionHandle::PgGetExpr
        | FunctionHandle::CurrentDatabase
        | FunctionHandle::CurrentSchemas
        | FunctionHandle::CurrentSetting
        | FunctionHandle::SetConfig
        | FunctionHandle::CurrentSettingMissingOk
        | FunctionHandle::PgIsInRecovery
        | FunctionHandle::Unnest
        | FunctionHandle::PgOptionsToTable
        | FunctionHandle::GenerateSeries
        | FunctionHandle::HeapTableamHandler
        | FunctionHandle::Bthandler
        | FunctionHandle::Hashhandler
        | FunctionHandle::Gisthandler
        | FunctionHandle::Ginhandler
        | FunctionHandle::Brinhandler
        | FunctionHandle::Spghandler
        | FunctionHandle::ArrayUpper
        | FunctionHandle::ArrayRemove
        | FunctionHandle::ArrayToString
        | FunctionHandle::ArrayAgg => Err(HeadError::internal(
            "this function handle does not run per row in the engine",
        )),
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
    use super::*;

    #[test]
    fn every_constant_pg_proc_row_has_an_implementation() {
        for row in pg::proc_rows() {
            let oid = row.oid(pg::pg_proc::OID).expect("captured oid");
            assert!(
                FunctionHandle::from_oid(oid).is_some(),
                "pg_proc oid {oid} has no FunctionHandle implementation"
            );
        }
    }

    #[test]
    fn every_implementation_has_a_constant_pg_proc_row() {
        for handle in FunctionHandle::ALL {
            handle.row().unwrap_or_else(|error| {
                panic!("FunctionHandle {handle:?} has no pg_proc row: {error}")
            });
        }
    }

    #[test]
    fn lookup_finds_set_config_by_name_and_argument_types() {
        let text = TypeHandle::by_oid(crate::analyze::types::TEXT_OID).expect("text is captured");
        let boolean = TypeHandle::by_oid(16).expect("bool is captured");
        let handle =
            FunctionHandle::lookup(&FunctionName::literal("set_config"), &[text, text, boolean])
                .expect("set_config(text, text, boolean) is registered");
        assert_eq!(handle, FunctionHandle::SetConfig);
    }

    #[test]
    fn lookup_refuses_an_unregistered_signature() {
        let text = TypeHandle::by_oid(crate::analyze::types::TEXT_OID).expect("text is captured");
        assert!(FunctionHandle::lookup(&FunctionName::literal("set_config"), &[text]).is_none());
        assert!(FunctionHandle::lookup(&FunctionName::literal("no_such_function"), &[]).is_none());
    }
}
