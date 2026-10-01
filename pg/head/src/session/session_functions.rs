use crate::analyze::functions::FunctionHandle;
use crate::analyze::plan::{
    ResolvedFrom, ResolvedFromItem, ResolvedJoin, ResolvedOrderItem, ResolvedOrderTarget,
    ResolvedQuery, ResolvedSimpleSelect, RowSecurityFact,
};
use crate::analyze::typing::{walk_typed_map, Scalar, Typed, TypedMap};
use crate::error::{HeadError, NotSupportedFeature};
use crate::ident::SchemaName;
use crate::session::settings::{KnownSetting, SettingName};
use crate::session::{SessionEffect, Settings};

struct Fold<'a> {
    settings: &'a mut Settings,
    effects: &'a mut Vec<SessionEffect>,
}

impl TypedMap for Fold<'_> {
    type Error = HeadError;

    fn enter(&mut self, typed: Typed) -> Result<Typed, HeadError> {
        match typed {
            Typed::Call(handle, args) => {
                let args = args
                    .into_iter()
                    .map(|arg| self.enter(arg))
                    .collect::<Result<Vec<_>, HeadError>>()?;
                match handle.evaluation() {
                    crate::analyze::functions::Evaluation::SetReturning => {
                        Err(HeadError::internal(
                            "a table function or access method handler reached scalar evaluation",
                        ))
                    }
                    crate::analyze::functions::Evaluation::Session => {
                        match literal_arguments(&args) {
                            Some(scalars) => {
                                evaluate(handle, &scalars, self.settings, self.effects)
                            }
                            None => Ok(Typed::Call(handle, args)),
                        }
                    }
                    crate::analyze::functions::Evaluation::Engine
                    | crate::analyze::functions::Evaluation::HeadEngine
                    | crate::analyze::functions::Evaluation::CatalogRendered
                    | crate::analyze::functions::Evaluation::Aggregate => {
                        Ok(Typed::Call(handle, args))
                    }
                }
            }
            Typed::Subquery(query) => Ok(Typed::Subquery(Box::new(self.fold_query(*query)?))),
            Typed::Exists(query) => Ok(Typed::Exists(Box::new(self.fold_query(*query)?))),
            Typed::InSelect(needle, query, negated) => Ok(Typed::InSelect(
                Box::new(self.enter(*needle)?),
                Box::new(self.fold_query(*query)?),
                negated,
            )),
            Typed::ArrayAgg(query, ty, empty_result) => Ok(Typed::ArrayAgg(
                Box::new(self.fold_query(*query)?),
                ty,
                empty_result,
            )),
            other @ Typed::Column(..)
            | other @ Typed::TableOid(_)
            | other @ Typed::Value(..)
            | other @ Typed::CurrentUser
            | other @ Typed::Cast(..)
            | other @ Typed::Not(_)
            | other @ Typed::And(_)
            | other @ Typed::Or(_)
            | other @ Typed::Compare(..)
            | other @ Typed::IsNull(..)
            | other @ Typed::Is(..)
            | other @ Typed::DistinctFrom(..)
            | other @ Typed::In(..)
            | other @ Typed::Concat(..)
            | other @ Typed::AlwaysFalse
            | other @ Typed::Case { .. }
            | other @ Typed::Subscript(..)
            | other @ Typed::AnyEq(..)
            | other @ Typed::ArrayLiteral(..) => walk_typed_map(self, other),
        }
    }
}

impl Fold<'_> {
    fn fold_query(&mut self, query: ResolvedQuery) -> Result<ResolvedQuery, HeadError> {
        Ok(ResolvedQuery {
            first: self.fold_simple_select(query.first)?,
            combined: query
                .combined
                .into_iter()
                .map(|simple| self.fold_simple_select(simple))
                .collect::<Result<_, HeadError>>()?,
            order_by: query
                .order_by
                .into_iter()
                .map(|item| {
                    let target = match item.target {
                        ResolvedOrderTarget::Expr(typed) => {
                            ResolvedOrderTarget::Expr(self.enter(typed)?)
                        }
                        position @ ResolvedOrderTarget::OutputPosition(_) => position,
                    };
                    Ok(ResolvedOrderItem {
                        target,
                        desc: item.desc,
                        nulls_first: item.nulls_first,
                    })
                })
                .collect::<Result<_, HeadError>>()?,
        })
    }

    fn fold_simple_select(
        &mut self,
        simple: ResolvedSimpleSelect,
    ) -> Result<ResolvedSimpleSelect, HeadError> {
        Ok(ResolvedSimpleSelect {
            distinct: simple.distinct,
            output: simple
                .output
                .into_iter()
                .map(|item| self.enter(item))
                .collect::<Result<_, HeadError>>()?,
            from: simple.from.map(|from| self.fold_from(from)).transpose()?,
            filter: simple.filter.map(|filter| self.enter(filter)).transpose()?,
        })
    }

    fn fold_from(&mut self, from: ResolvedFrom) -> Result<ResolvedFrom, HeadError> {
        Ok(ResolvedFrom {
            first: self.fold_from_item(from.first)?,
            joins: from
                .joins
                .into_iter()
                .map(|join| {
                    Ok(ResolvedJoin {
                        kind: join.kind,
                        item: self.fold_from_item(join.item)?,
                        on: join.on.map(|on| self.enter(on)).transpose()?,
                    })
                })
                .collect::<Result<_, HeadError>>()?,
        })
    }

    fn fold_from_item(&mut self, item: ResolvedFromItem) -> Result<ResolvedFromItem, HeadError> {
        Ok(match item {
            ResolvedFromItem::Relation {
                slot,
                oid,
                backing,
                security,
            } => ResolvedFromItem::Relation {
                slot,
                oid,
                backing,
                security: match security {
                    RowSecurityFact::Enforced(typed) => {
                        RowSecurityFact::Enforced(self.enter(typed)?)
                    }
                    other @ RowSecurityFact::Unfiltered | other @ RowSecurityFact::Refused => other,
                },
            },
            ResolvedFromItem::Derived { slot, query } => ResolvedFromItem::Derived {
                slot,
                query: Box::new(self.fold_query(*query)?),
            },
            ResolvedFromItem::Function { slot, handle, args } => ResolvedFromItem::Function {
                slot,
                handle,
                args: args
                    .into_iter()
                    .map(|arg| self.enter(arg))
                    .collect::<Result<_, HeadError>>()?,
            },
        })
    }
}

pub(crate) fn fold_query(
    query: ResolvedQuery,
    settings: &mut Settings,
    effects: &mut Vec<SessionEffect>,
) -> Result<ResolvedQuery, HeadError> {
    Fold { settings, effects }.fold_query(query)
}

fn literal_arguments(args: &[Typed]) -> Option<Vec<Scalar>> {
    args.iter()
        .map(|arg| match arg {
            Typed::Value(scalar, _) => Some(scalar.clone()),
            Typed::Column(..)
            | Typed::TableOid(_)
            | Typed::CurrentUser
            | Typed::Cast(..)
            | Typed::Not(_)
            | Typed::And(_)
            | Typed::Or(_)
            | Typed::Compare(..)
            | Typed::IsNull(..)
            | Typed::Is(..)
            | Typed::DistinctFrom(..)
            | Typed::In(..)
            | Typed::Concat(..)
            | Typed::AlwaysFalse
            | Typed::Case { .. }
            | Typed::Subquery(_)
            | Typed::Exists(_)
            | Typed::InSelect(..)
            | Typed::Call(..)
            | Typed::Subscript(..)
            | Typed::AnyEq(..)
            | Typed::ArrayLiteral(..)
            | Typed::ArrayAgg(..) => None,
        })
        .collect()
}

fn scalar_at(scalars: &[Scalar], index: usize) -> Result<&Scalar, HeadError> {
    scalars.get(index).ok_or_else(|| {
        HeadError::internal("a registered function was called with too few arguments")
    })
}

fn text_at(scalars: &[Scalar], index: usize) -> Result<String, HeadError> {
    match scalar_at(scalars, index)? {
        Scalar::Text(text) => Ok(text.clone()),
        Scalar::Integer(value) => Ok(value.to_string()),
        Scalar::Null => Err(HeadError::not_supported(
            NotSupportedFeature::NullArgumentToRegisteredFunction,
        )),
        Scalar::Array(_) => Err(HeadError::internal(
            "an array literal reached a registered scalar function",
        )),
    }
}

fn truthy(scalar: &Scalar) -> bool {
    matches!(scalar, Scalar::Integer(value) if *value != 0)
}

fn evaluate(
    handle: FunctionHandle,
    scalars: &[Scalar],
    settings: &mut Settings,
    effects: &mut Vec<SessionEffect>,
) -> Result<Typed, HeadError> {
    let result_type = handle.result_type()?;
    let value = evaluate_scalars(handle, scalars, settings, effects)?;
    Ok(Typed::Value(value, result_type))
}

pub(crate) fn evaluate_over_row(
    handle: FunctionHandle,
    raw: &[turso_core::Value],
    settings: &mut Settings,
    effects: &mut Vec<SessionEffect>,
) -> Result<turso_core::Value, HeadError> {
    let scalars = raw
        .iter()
        .cloned()
        .map(scalar_from_engine_value)
        .collect::<Result<Vec<_>, HeadError>>()?;
    let result_type = handle.result_type()?;
    let value = evaluate_scalars(handle, &scalars, settings, effects)?;
    crate::lower::scalar_value(&value, result_type)
}

fn scalar_from_engine_value(value: turso_core::Value) -> Result<Scalar, HeadError> {
    match value {
        turso_core::Value::Null => Ok(Scalar::Null),
        other @ turso_core::Value::Numeric(_)
        | other @ turso_core::Value::Text(_)
        | other @ turso_core::Value::Blob(_) => match other.as_int() {
            Some(integer) => Ok(Scalar::Integer(integer)),
            None => other
                .to_text()
                .map(|text| Scalar::Text(text.to_string()))
                .ok_or_else(|| {
                    HeadError::internal(
                        "a session function argument held an unsupported engine value",
                    )
                }),
        },
    }
}

fn evaluate_scalars(
    handle: FunctionHandle,
    scalars: &[Scalar],
    settings: &mut Settings,
    effects: &mut Vec<SessionEffect>,
) -> Result<Scalar, HeadError> {
    Ok(match handle {
        FunctionHandle::CurrentDatabase => Scalar::Text("postgres".to_string()),
        FunctionHandle::PgIsInRecovery => Scalar::Integer(0),
        FunctionHandle::CurrentSchemas => {
            let include_implicit = truthy(scalar_at(scalars, 0)?);
            let mut schemas: Vec<String> = settings
                .search_path
                .iter()
                .map(|schema| schema.as_str().to_string())
                .collect();
            if include_implicit
                && !settings
                    .search_path
                    .iter()
                    .any(|schema| *schema == SchemaName::pg_catalog())
            {
                schemas.insert(0, "pg_catalog".to_string());
            }
            Scalar::Text(format!("{{{}}}", schemas.join(",")))
        }
        FunctionHandle::CurrentSetting => {
            let name = text_at(scalars, 0)?;
            current_setting(settings, &name, false)?
        }
        FunctionHandle::CurrentSettingMissingOk => {
            let name = text_at(scalars, 0)?;
            let missing_ok = truthy(scalar_at(scalars, 1)?);
            current_setting(settings, &name, missing_ok)?
        }
        FunctionHandle::SetConfig => {
            let name = text_at(scalars, 0)?;
            let raw_value = text_at(scalars, 1)?;
            let local = truthy(scalar_at(scalars, 2)?);
            let target = KnownSetting::lookup(&SettingName::from_text(&name))?;
            let parsed = target.parse(std::slice::from_ref(&raw_value))?;
            settings.apply(target, &parsed);
            let display = parsed.display();
            effects.push(SessionEffect::Set {
                target,
                value: parsed,
                local,
            });
            Scalar::Text(display)
        }
        FunctionHandle::Unnest
        | FunctionHandle::PgOptionsToTable
        | FunctionHandle::GenerateSeries => {
            return Err(HeadError::internal(
                "a table function reached scalar evaluation",
            ))
        }
        FunctionHandle::HeapTableamHandler
        | FunctionHandle::Bthandler
        | FunctionHandle::Hashhandler
        | FunctionHandle::Gisthandler
        | FunctionHandle::Ginhandler
        | FunctionHandle::Brinhandler
        | FunctionHandle::Spghandler => {
            return Err(HeadError::internal(
                "an access method handler reached scalar evaluation",
            ))
        }
        FunctionHandle::QuoteIdent
        | FunctionHandle::QuoteLiteral
        | FunctionHandle::FormatType
        | FunctionHandle::AclDefault
        | FunctionHandle::PgGetTriggerdef
        | FunctionHandle::PgGetConstraintdef
        | FunctionHandle::PgGetConstraintdefPretty
        | FunctionHandle::PgGetIndexdef
        | FunctionHandle::PgGetIndexdefColumn
        | FunctionHandle::PgGetExpr
        | FunctionHandle::ArrayUpper
        | FunctionHandle::ArrayRemove
        | FunctionHandle::ArrayToString
        | FunctionHandle::ArrayAgg => {
            return Err(HeadError::internal(
                "an engine, catalog-rendered or aggregate function is not a session function",
            ))
        }
    })
}

fn current_setting(settings: &Settings, name: &str, missing_ok: bool) -> Result<Scalar, HeadError> {
    match KnownSetting::lookup(&SettingName::from_text(name)) {
        Ok(known) => Ok(Scalar::Text(settings.read(known).display())),
        Err(_) if missing_ok => Ok(Scalar::Null),
        Err(error) => Err(error),
    }
}
