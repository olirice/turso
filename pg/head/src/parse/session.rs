use pg_query::protobuf::{
    a_const::Val, node::Node as PgNode, DefElem, DefElemAction, ExecuteStmt, LockStmt, Node,
    PrepareStmt, TransactionStmt, TransactionStmtKind, VariableSetKind, VariableSetStmt,
};

use super::{node, table_name, PROOF};
use crate::error::{HeadError, NotSupportedFeature};
use crate::ident::PreparedName;
use crate::parse::expr;
use crate::parse::statement::{SetStatement, Statement};
use crate::session::settings::{KnownSetting, SettingName};

pub(super) fn prepare_statement(prepare: &PrepareStmt) -> Result<Statement, HeadError> {
    let PrepareStmt {
        name,
        argtypes,
        query,
    } = prepare;
    let param_types = argtypes
        .iter()
        .map(|arg| {
            let PgNode::TypeName(type_name) = node(Some(arg))? else {
                return Err(HeadError::internal(
                    "a PREPARE parameter type that is not a TypeName",
                ));
            };
            expr::parameter_type(PROOF, type_name)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let inner = query
        .as_deref()
        .ok_or_else(|| HeadError::internal("PREPARE without a query"))?;
    let PgNode::SelectStmt(select) = node(Some(inner))? else {
        return Err(HeadError::not_supported(
            NotSupportedFeature::PrepareNonSelect,
        ));
    };
    let query = super::query::query(select)?;
    Ok(Statement::Prepare {
        name: PreparedName::from_parse_tree(PROOF, name.clone())?,
        param_types,
        query,
    })
}

pub(super) fn execute_statement(execute: &ExecuteStmt) -> Result<Statement, HeadError> {
    let ExecuteStmt { name, params } = execute;
    let args = params
        .iter()
        .map(|arg| expr::admit(PROOF, node(Some(arg))?))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Statement::Execute {
        name: PreparedName::from_parse_tree(PROOF, name.clone())?,
        args,
    })
}

const ACCESS_SHARE_LOCK: i32 = 1;

pub(super) fn lock_tables(lock: &LockStmt) -> Result<Statement, HeadError> {
    let LockStmt {
        relations,
        mode,
        nowait,
    } = lock;
    if *mode != ACCESS_SHARE_LOCK {
        return Err(HeadError::not_supported(NotSupportedFeature::LockTableMode));
    }
    if *nowait {
        return Err(HeadError::not_supported(
            NotSupportedFeature::LockTableNowait,
        ));
    }
    let tables = relations
        .iter()
        .map(|relation| {
            let PgNode::RangeVar(relation) = node(Some(relation))? else {
                return Err(HeadError::internal(
                    "a locked relation that is not a RangeVar",
                ));
            };
            table_name(relation)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Statement::Lock { tables })
}

pub(super) fn transaction_control(transaction: &TransactionStmt) -> Result<Statement, HeadError> {
    let TransactionStmt {
        kind,
        options,
        savepoint_name,
        gid,
        chain,
        // PostgreSQL does not attach a position to "AND CHAIN" or
        // savepoint-related refusals.
        location: _,
    } = transaction;
    if *chain {
        return Err(HeadError::not_supported(
            NotSupportedFeature::TransactionChain,
        ));
    }
    match TransactionStmtKind::try_from(*kind) {
        Ok(TransactionStmtKind::TransStmtBegin | TransactionStmtKind::TransStmtStart) => {
            // Neither a savepoint name nor a two-phase commit id is ever
            // set for `BEGIN`/`START TRANSACTION`: both belong to kinds
            // (`SAVEPOINT`, `PREPARE TRANSACTION`, ...) the wildcard arm
            // below refuses outright, never reaching here.
            if !savepoint_name.is_empty() || !gid.is_empty() {
                return Err(HeadError::internal(
                    "a BEGIN carries a savepoint name or two-phase commit id",
                ));
            }
            Ok(Statement::Begin {
                read_only: transaction_modes(options)?.1,
            })
        }
        Ok(TransactionStmtKind::TransStmtCommit) => {
            if !savepoint_name.is_empty() || !gid.is_empty() {
                return Err(HeadError::internal(
                    "a COMMIT carries a savepoint name or two-phase commit id",
                ));
            }
            Ok(Statement::Commit)
        }
        Ok(TransactionStmtKind::TransStmtRollback) => {
            if !savepoint_name.is_empty() || !gid.is_empty() {
                return Err(HeadError::internal(
                    "a ROLLBACK carries a savepoint name or two-phase commit id",
                ));
            }
            Ok(Statement::Rollback)
        }
        _ => Err(HeadError::not_supported(
            NotSupportedFeature::SavepointsAndTwoPhaseCommands,
        )),
    }
}

fn transaction_modes(options: &[Node]) -> Result<(bool, Option<bool>), HeadError> {
    let mut sets_isolation = false;
    let mut read_only = None;
    for option in options {
        let PgNode::DefElem(option) = node(Some(option))? else {
            return Err(HeadError::internal(
                "a transaction option that is not a DefElem",
            ));
        };
        let DefElem {
            // A transaction mode is never namespaced or itself an
            // ALTER-style add/drop/set action; both are structurally
            // impossible for `BEGIN`/`SET TRANSACTION`'s own option-list
            // grammar production.
            defnamespace,
            defname,
            arg,
            defaction,
            // PostgreSQL does not attach a position to an unrecognized or
            // unsupported transaction mode.
            location: _,
        } = &**option;
        if !defnamespace.is_empty() {
            return Err(HeadError::internal(
                "a transaction mode option carries a namespace",
            ));
        }
        if !matches!(
            DefElemAction::try_from(*defaction),
            Ok(DefElemAction::DefelemUnspec)
        ) {
            return Err(HeadError::internal(
                "a transaction mode option carries an ALTER-style action",
            ));
        }
        let argument = arg.as_deref().map(|arg| node(Some(arg)));
        match (defname.as_str(), argument) {
            ("transaction_isolation", Some(Ok(PgNode::AConst(constant)))) => {
                let Some(Val::Sval(level)) = &constant.val else {
                    return Err(HeadError::internal(
                        "a transaction isolation level that is not a string constant",
                    ));
                };
                if level.sval != "repeatable read" {
                    return Err(HeadError::not_supported(
                        NotSupportedFeature::IsolationLevel(level.sval.clone()),
                    ));
                }
                sets_isolation = true;
            }
            ("transaction_read_only", Some(Ok(PgNode::AConst(constant)))) => {
                read_only = Some(matches!(
                    &constant.val,
                    Some(Val::Ival(value)) if value.ival != 0
                ));
            }
            ("transaction_deferrable", _) => {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::DeferrableTransactions,
                ))
            }
            (name, _) => {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::TransactionOption(name.to_string()),
                ))
            }
        }
    }
    Ok((sets_isolation, read_only))
}

pub(super) fn set_variable(set: &VariableSetStmt) -> Result<Statement, HeadError> {
    let VariableSetStmt {
        kind,
        name,
        args,
        is_local,
    } = set;
    match VariableSetKind::try_from(*kind) {
        Ok(VariableSetKind::VarSetMulti) if name == "TRANSACTION" => {
            if *is_local {
                return Err(HeadError::not_supported(
                    NotSupportedFeature::SetLocalTransaction,
                ));
            }
            let (sets_isolation, read_only) = transaction_modes(args)?;
            Ok(Statement::SetTransaction {
                sets_isolation,
                read_only,
            })
        }
        Ok(VariableSetKind::VarSetValue) => {
            let values = set_values(args)?;
            let target = KnownSetting::lookup(&SettingName::from_text(name))?;
            let value = target.parse(&values)?;
            Ok(Statement::Set(SetStatement {
                target,
                value,
                local: *is_local,
            }))
        }
        Ok(VariableSetKind::VarReset | VariableSetKind::VarResetAll) => {
            Err(HeadError::not_supported(NotSupportedFeature::ResetSetting))
        }
        _ => Err(HeadError::not_supported(
            NotSupportedFeature::SetToDefaultOrFromCurrent,
        )),
    }
}

fn set_values(args: &[Node]) -> Result<Vec<String>, HeadError> {
    args.iter()
        .map(|argument| {
            let PgNode::AConst(constant) = node(Some(argument))? else {
                return Err(HeadError::not_supported(NotSupportedFeature::SetValue));
            };
            if constant.isnull {
                return Err(HeadError::not_supported(NotSupportedFeature::SetValue));
            }
            match &constant.val {
                Some(Val::Sval(text)) => Ok(text.sval.clone()),
                Some(Val::Ival(integer)) => Ok(integer.ival.to_string()),
                Some(Val::Fval(float)) => Ok(float.fval.clone()),
                Some(Val::Boolval(boolean)) => Ok(boolean.boolval.to_string()),
                Some(Val::Bsval(_)) | None => {
                    Err(HeadError::not_supported(NotSupportedFeature::SetValue))
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()
}
