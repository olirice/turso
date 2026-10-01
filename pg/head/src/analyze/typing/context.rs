//! One structural classification gate for expressions, modeled on
//! PostgreSQL's own `ParseExprKind` (`parse_node.h`). `Position` names every
//! admitted expression position; `Feature` names the limitation classes
//! they differ on; `Position::rules()` pins, per feature, PostgreSQL's own
//! behavior (`Verdict::Allowed`/`Refused`) or this head's own gap
//! (`Verdict::NotBuilt`, always `0A000`).
#![deny(clippy::wildcard_enum_match_arm)]

use crate::analyze::functions::{Evaluation, FunctionHandle};
use crate::analyze::types::BOOL_OID;
use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::parse::expr::Expr;
use crate::parse::Location;

use super::Typed;

/// Every expression position the head admits, named as PostgreSQL 18 names
/// them (probed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Position {
    Where,
    JoinOn,
    SelectTarget,
    OrderBy,
    FromFunction,
    InsertValue,
    ExecuteParameter,
    Policy,
}

/// PostgreSQL's own name for a position: singular for a subquery message
/// ("cannot use subquery in EXECUTE parameter") and for `coerce_to_boolean`
/// ("argument of WHERE must be type boolean"); plural for an aggregate or
/// set-returning-function message ("... not allowed in JOIN conditions").
/// `NotBuilt` messages, having no PostgreSQL text to match, use `singular`.
#[derive(Clone, Copy)]
pub(crate) struct Noun {
    pub(crate) singular: &'static str,
    pub(crate) plural: &'static str,
}

impl Position {
    fn noun(self) -> Noun {
        match self {
            Position::Where => Noun {
                singular: "WHERE",
                plural: "WHERE",
            },
            Position::JoinOn => Noun {
                singular: "JOIN/ON",
                plural: "JOIN conditions",
            },
            Position::SelectTarget => Noun {
                singular: "SELECT",
                plural: "SELECT",
            },
            Position::OrderBy => Noun {
                singular: "ORDER BY",
                plural: "ORDER BY",
            },
            Position::FromFunction => Noun {
                singular: "a function in FROM",
                plural: "functions in FROM",
            },
            Position::InsertValue => Noun {
                singular: "VALUES",
                plural: "VALUES",
            },
            Position::ExecuteParameter => Noun {
                singular: "EXECUTE parameter",
                plural: "EXECUTE parameters",
            },
            Position::Policy => Noun {
                singular: "POLICY",
                plural: "policy expressions",
            },
        }
    }

    /// This position's rules: one `Verdict` per feature, plus its own
    /// boolean-result requirement. Every arm is a full struct literal (no
    /// `Default`, no `..`), so a new feature forces a decision everywhere.
    pub(crate) fn rules(self) -> Rules {
        match self {
            Position::Where => Rules {
                subquery: Verdict::Allowed,
                aggregate: Verdict::Refused(Wording::Template),
                row_computed: Verdict::NotBuilt,
                set_returning: Verdict::Refused(Wording::Template),
                require_boolean: true,
                reports_position: true,
            },
            Position::JoinOn => Rules {
                subquery: Verdict::Allowed,
                aggregate: Verdict::Refused(Wording::Template),
                row_computed: Verdict::NotBuilt,
                set_returning: Verdict::Refused(Wording::Template),
                require_boolean: true,
                reports_position: true,
            },
            Position::SelectTarget => Rules {
                subquery: Verdict::Allowed,
                aggregate: Verdict::Allowed,
                row_computed: Verdict::Allowed,
                set_returning: Verdict::NotBuilt,
                require_boolean: false,
                reports_position: true,
            },
            Position::OrderBy => Rules {
                subquery: Verdict::Allowed,
                aggregate: Verdict::NotBuilt,
                row_computed: Verdict::NotBuilt,
                set_returning: Verdict::NotBuilt,
                require_boolean: false,
                reports_position: true,
            },
            Position::FromFunction => Rules {
                subquery: Verdict::Allowed,
                aggregate: Verdict::Refused(Wording::Template),
                row_computed: Verdict::NotBuilt,
                set_returning: Verdict::Refused(Wording::Exact),
                require_boolean: false,
                reports_position: true,
            },
            Position::InsertValue => Rules {
                subquery: Verdict::NotBuilt,
                aggregate: Verdict::Refused(Wording::Template),
                row_computed: Verdict::NotBuilt,
                set_returning: Verdict::NotBuilt,
                require_boolean: false,
                reports_position: true,
            },
            Position::ExecuteParameter => Rules {
                subquery: Verdict::Refused(Wording::Template),
                aggregate: Verdict::Refused(Wording::Template),
                row_computed: Verdict::NotBuilt,
                set_returning: Verdict::Refused(Wording::Template),
                require_boolean: false,
                reports_position: true,
            },
            // PostgreSQL 18 attaches no LINE to any error raised while
            // type-checking a policy's USING expression (probed live),
            // unlike the identical check inside a SELECT.
            Position::Policy => Rules {
                subquery: Verdict::NotBuilt,
                aggregate: Verdict::Refused(Wording::Template),
                row_computed: Verdict::NotBuilt,
                set_returning: Verdict::Refused(Wording::Template),
                require_boolean: true,
                reports_position: false,
            },
        }
    }
}

/// A `Verdict::Refused` message: PostgreSQL's own template for the feature,
/// filled in with the position's noun (`Wording::Template`), or, for the
/// one position/feature pair where PostgreSQL uses a wholly different
/// template (a set-returning function nested in a FROM-function argument),
/// PostgreSQL's exact text (`Wording::Exact`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wording {
    Template,
    Exact,
}

/// A verdict for one (position, feature) pair. `Allowed` and `Refused` are
/// PostgreSQL's own behavior; `NotBuilt` is what PostgreSQL allows but this
/// head does not implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Allowed,
    Refused(Wording),
    NotBuilt,
}

/// One position's full rule set: one `Verdict` per feature, plus its own
/// boolean-result requirement.
pub(crate) struct Rules {
    pub(crate) subquery: Verdict,
    pub(crate) aggregate: Verdict,
    pub(crate) row_computed: Verdict,
    pub(crate) set_returning: Verdict,
    pub(crate) require_boolean: bool,
    /// Whether this position's errors may carry a source position at all.
    pub(crate) reports_position: bool,
}

impl Rules {
    fn rule(&self, feature: Feature) -> Verdict {
        match feature {
            Feature::Subquery => self.subquery,
            Feature::Aggregate => self.aggregate,
            Feature::RowComputed(_) => self.row_computed,
            Feature::SetReturning => self.set_returning,
        }
    }
}

/// The limitation classes the positions differ on. There is no `Plain`
/// variant: an expression that no position restricts classifies as
/// `None` (see `Feature::of`, `Feature::of_call`), so a refusal can never
/// be asked for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Feature {
    Subquery,
    Aggregate,
    /// Carries the call so its own message can name it: a catalog-rendered
    /// function, always; or a session function fed a non-constant (column)
    /// argument (only `check::call_feature`, not `Feature::of_call`, can
    /// see that distinction). Both need the per-row output hook that only a
    /// literal top-level `SELECT` target item gets (see
    /// `analyze/select.rs`'s `OutputSpec`, `ARCH.md`'s "Function
    /// evaluation"); a session function fed only constant arguments is
    /// folded away before lowering wherever it appears, so it classifies
    /// as `None` regardless of position.
    RowComputed(FunctionHandle),
    SetReturning,
}

impl Feature {
    /// Classifies a not-yet-resolved parse expression. Exhaustive over
    /// every `Expr` variant: only the four subquery forms are restricted by
    /// position before they resolve to a call; everything else defers to
    /// `Feature::of_call` once (and if) it turns out to be one.
    pub(crate) fn of(expr: &Expr) -> Option<Feature> {
        match expr {
            Expr::Subquery(_, _)
            | Expr::Exists(_, _)
            | Expr::InSelect(..)
            | Expr::ArrayFromQuery(..) => Some(Feature::Subquery),
            Expr::Column(..)
            | Expr::CurrentUser
            | Expr::Literal(..)
            | Expr::Param(..)
            | Expr::Cast(..)
            | Expr::Not(_)
            | Expr::And(_)
            | Expr::Or(_)
            | Expr::Compare(..)
            | Expr::IsNull(..)
            | Expr::Is(..)
            | Expr::DistinctFrom(..)
            | Expr::In(..)
            | Expr::Concat(..)
            | Expr::Case { .. }
            | Expr::Call(..)
            | Expr::Subscript(..)
            | Expr::AnyEq(..)
            | Expr::ArrayLiteral(_)
            | Expr::Resolved(_) => None,
        }
    }

    /// The location to report for a refusal of `Feature::of`'s own
    /// classification, read straight off the same node `of` matched
    /// (`None` for every non-subquery shape, since only those carry one).
    pub(crate) fn location(expr: &Expr) -> Option<Location> {
        match expr {
            Expr::Subquery(_, location) | Expr::Exists(_, location) => *location,
            Expr::InSelect(_, _, _, location) => *location,
            Expr::ArrayFromQuery(_, _, location) => *location,
            Expr::Column(..)
            | Expr::CurrentUser
            | Expr::Literal(..)
            | Expr::Param(..)
            | Expr::Cast(..)
            | Expr::Not(_)
            | Expr::And(_)
            | Expr::Or(_)
            | Expr::Compare(..)
            | Expr::IsNull(..)
            | Expr::Is(..)
            | Expr::DistinctFrom(..)
            | Expr::In(..)
            | Expr::Concat(..)
            | Expr::Case { .. }
            | Expr::Call(..)
            | Expr::Subscript(..)
            | Expr::AnyEq(..)
            | Expr::ArrayLiteral(_)
            | Expr::Resolved(_) => None,
        }
    }

    /// Classifies a resolved call by evaluation kind alone; see
    /// `Feature::RowComputed`'s doc comment for what this cannot see.
    pub(crate) fn of_call(handle: FunctionHandle) -> Option<Feature> {
        match handle.evaluation() {
            Evaluation::Engine | Evaluation::HeadEngine | Evaluation::Session => None,
            Evaluation::Aggregate => Some(Feature::Aggregate),
            Evaluation::CatalogRendered => Some(Feature::RowComputed(handle)),
            Evaluation::SetReturning => Some(Feature::SetReturning),
        }
    }

    /// The one refusal path: PostgreSQL 18's own `(SQLSTATE, message)`
    /// (probed) when this position's verdict is `Refused`, this head's own
    /// `0A000` when it is `NotBuilt`. Never called for `Verdict::Allowed`.
    pub(crate) fn refusal(self, position: Position) -> HeadError {
        let noun = position.noun();
        match self {
            Feature::RowComputed(handle) => {
                let function = handle.engine_name().unwrap_or("a function");
                match handle.evaluation() {
                    Evaluation::Session => HeadError::not_supported(
                        NotSupportedFeature::NonConstantArgumentOutsideSelectItem { function },
                    ),
                    Evaluation::Engine
                    | Evaluation::HeadEngine
                    | Evaluation::Aggregate
                    | Evaluation::CatalogRendered
                    | Evaluation::SetReturning => {
                        HeadError::not_supported(NotSupportedFeature::FunctionInPosition {
                            function,
                            position: noun.singular,
                        })
                    }
                }
            }
            Feature::Subquery => match position.rules().rule(self) {
                Verdict::Allowed => {
                    HeadError::internal("Context::permit never refuses Verdict::Allowed")
                }
                Verdict::Refused(Wording::Template) => {
                    HeadError::raise(PgError::CannotUseSubqueryInPosition {
                        position: noun.singular,
                    })
                }
                Verdict::Refused(Wording::Exact) => {
                    HeadError::internal("a subquery refusal has no exact wording")
                }
                Verdict::NotBuilt => {
                    HeadError::not_supported(NotSupportedFeature::SubqueryInPosition(noun.singular))
                }
            },
            Feature::Aggregate => match position.rules().rule(self) {
                Verdict::Allowed => {
                    HeadError::internal("Context::permit never refuses Verdict::Allowed")
                }
                Verdict::Refused(Wording::Template) => {
                    HeadError::raise(PgError::AggregateNotAllowedInPosition {
                        position: noun.plural,
                    })
                }
                Verdict::Refused(Wording::Exact) => {
                    HeadError::internal("an aggregate refusal has no exact wording")
                }
                Verdict::NotBuilt => HeadError::not_supported(
                    NotSupportedFeature::AggregateInPosition(noun.singular),
                ),
            },
            Feature::SetReturning => match position.rules().rule(self) {
                Verdict::Allowed => {
                    HeadError::internal("Context::permit never refuses Verdict::Allowed")
                }
                Verdict::Refused(Wording::Template) => {
                    HeadError::raise(PgError::SetReturningNotAllowedInPosition {
                        position: noun.plural,
                    })
                }
                Verdict::Refused(Wording::Exact) => {
                    HeadError::raise(PgError::SetReturningMustAppearAtTopLevelOfFrom)
                }
                Verdict::NotBuilt => HeadError::not_supported(
                    NotSupportedFeature::SetReturningInPosition(noun.singular),
                ),
            },
        }
    }
}

/// One admitted expression's classification context: which position it is
/// in. Built only from a `Position`.
pub(crate) struct Context(Position);

impl Context {
    pub(crate) fn new(position: Position) -> Context {
        Context(position)
    }

    /// The one gate every expression node passes through.
    pub(crate) fn permit(&self, feature: Feature) -> Result<(), HeadError> {
        match self.0.rules().rule(feature) {
            Verdict::Allowed => Ok(()),
            Verdict::Refused(_) | Verdict::NotBuilt => Err(feature.refusal(self.0)),
        }
    }

    /// The one place a node's location becomes (or does not become) an
    /// error's position: this position's own `reports_position` rule, not
    /// a strip applied after the fact by whoever called into this context.
    pub(crate) fn locate(&self, location: Option<Location>) -> Option<Location> {
        location.filter(|_| self.0.rules().reports_position)
    }

    /// The position's own boolean-result requirement, applied once the
    /// top-level expression of a position has finished type checking.
    pub(crate) fn require_boolean(&self, typed: &Typed) -> Result<(), HeadError> {
        if !self.0.rules().require_boolean {
            return Ok(());
        }
        let ty = typed.result_type()?;
        if ty.oid() != BOOL_OID {
            return Err(HeadError::raise(PgError::ArgumentMustBeBoolean {
                what: self.0.noun().singular,
                actual: ty,
            }));
        }
        Ok(())
    }
}
