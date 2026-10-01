use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::SchemaName;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SettingName(String);

impl SettingName {
    pub(crate) fn from_text(name: &str) -> Self {
        SettingName(name.to_ascii_lowercase())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum KnownSetting {
    SearchPath,
    RowSecurity,
    StatementTimeout,
    LockTimeout,
    IdleInTransactionSessionTimeout,
    TransactionTimeout,
    DateStyle,
    IntervalStyle,
    ExtraFloatDigits,
    SynchronizeSeqscans,
}

pub(crate) struct SettingMeta {
    pub(crate) known: KnownSetting,
    pub(crate) name: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) unit: Option<&'static str>,
    pub(crate) category: &'static str,
    pub(crate) short_desc: &'static str,
    pub(crate) extra_desc: Option<&'static str>,
    pub(crate) vartype: &'static str,
    pub(crate) accepted_values: &'static [&'static str],
    pub(crate) min_val: Option<&'static str>,
    pub(crate) max_val: Option<&'static str>,
    pub(crate) enumvals: Option<&'static [&'static str]>,
    pub(crate) boot_val: &'static str,
}

const CONNECTION_DEFAULTS: &str = "Client Connection Defaults / Statement Behavior";
const LOCALE_AND_FORMATTING: &str = "Client Connection Defaults / Locale and Formatting";
const PREVIOUS_VERSIONS: &str = "Version and Platform Compatibility / Previous PostgreSQL Versions";

const SEARCH_PATH_META: SettingMeta = SettingMeta {
    known: KnownSetting::SearchPath,
    name: "search_path",
    display_name: "search_path",
    unit: None,
    category: CONNECTION_DEFAULTS,
    short_desc: "Sets the schema search order for names that are not schema-qualified.",
    extra_desc: None,
    vartype: "string",
    accepted_values: &[],
    min_val: None,
    max_val: None,
    enumvals: None,
    boot_val: "\"$user\", public",
};

const ROW_SECURITY_META: SettingMeta = SettingMeta {
    known: KnownSetting::RowSecurity,
    name: "row_security",
    display_name: "row_security",
    unit: None,
    category: CONNECTION_DEFAULTS,
    short_desc: "Enable row security.",
    extra_desc: Some("When enabled, row security will be applied to all users."),
    vartype: "bool",
    accepted_values: &[],
    min_val: None,
    max_val: None,
    enumvals: None,
    boot_val: "on",
};

const STATEMENT_TIMEOUT_META: SettingMeta = SettingMeta {
    known: KnownSetting::StatementTimeout,
    name: "statement_timeout",
    display_name: "statement_timeout",
    unit: Some("ms"),
    category: CONNECTION_DEFAULTS,
    short_desc: "Sets the maximum allowed duration of any statement.",
    extra_desc: Some("A value of 0 turns off the timeout."),
    vartype: "integer",
    accepted_values: &["0"],
    min_val: Some("0"),
    max_val: Some("2147483647"),
    enumvals: None,
    boot_val: "0",
};

const LOCK_TIMEOUT_META: SettingMeta = SettingMeta {
    known: KnownSetting::LockTimeout,
    name: "lock_timeout",
    display_name: "lock_timeout",
    unit: Some("ms"),
    category: CONNECTION_DEFAULTS,
    short_desc: "Sets the maximum allowed duration of any wait for a lock.",
    extra_desc: Some("A value of 0 turns off the timeout."),
    vartype: "integer",
    accepted_values: &["0"],
    min_val: Some("0"),
    max_val: Some("2147483647"),
    enumvals: None,
    boot_val: "0",
};

const IDLE_IN_TRANSACTION_SESSION_TIMEOUT_META: SettingMeta = SettingMeta {
    known: KnownSetting::IdleInTransactionSessionTimeout,
    name: "idle_in_transaction_session_timeout",
    display_name: "idle_in_transaction_session_timeout",
    unit: Some("ms"),
    category: CONNECTION_DEFAULTS,
    short_desc: "Sets the maximum allowed idle time between queries, when in a transaction.",
    extra_desc: Some("A value of 0 turns off the timeout."),
    vartype: "integer",
    accepted_values: &["0"],
    min_val: Some("0"),
    max_val: Some("2147483647"),
    enumvals: None,
    boot_val: "0",
};

const TRANSACTION_TIMEOUT_META: SettingMeta = SettingMeta {
    known: KnownSetting::TransactionTimeout,
    name: "transaction_timeout",
    display_name: "transaction_timeout",
    unit: Some("ms"),
    category: CONNECTION_DEFAULTS,
    short_desc: "Sets the maximum allowed duration of any transaction within a session (not a prepared transaction).",
    extra_desc: Some("A value of 0 turns off the timeout."),
    vartype: "integer",
    accepted_values: &["0"],
    min_val: Some("0"),
    max_val: Some("2147483647"),
    enumvals: None,
    boot_val: "0",
};

const DATE_STYLE_META: SettingMeta = SettingMeta {
    known: KnownSetting::DateStyle,
    name: "datestyle",
    display_name: "DateStyle",
    unit: None,
    category: LOCALE_AND_FORMATTING,
    short_desc: "Sets the display format for date and time values.",
    extra_desc: Some("Also controls interpretation of ambiguous date inputs."),
    vartype: "string",
    accepted_values: &["ISO", "ISO, MDY"],
    min_val: None,
    max_val: None,
    enumvals: None,
    boot_val: "ISO, MDY",
};

const INTERVAL_STYLE_META: SettingMeta = SettingMeta {
    known: KnownSetting::IntervalStyle,
    name: "intervalstyle",
    display_name: "IntervalStyle",
    unit: None,
    category: LOCALE_AND_FORMATTING,
    short_desc: "Sets the display format for interval values.",
    extra_desc: None,
    vartype: "enum",
    accepted_values: &["postgres"],
    min_val: None,
    max_val: None,
    enumvals: Some(&["postgres", "postgres_verbose", "sql_standard", "iso_8601"]),
    boot_val: "postgres",
};

const EXTRA_FLOAT_DIGITS_META: SettingMeta = SettingMeta {
    known: KnownSetting::ExtraFloatDigits,
    name: "extra_float_digits",
    display_name: "extra_float_digits",
    unit: None,
    category: LOCALE_AND_FORMATTING,
    short_desc: "Sets the number of digits displayed for floating-point values.",
    extra_desc: Some(
        "This affects real, double precision, and geometric data types. A zero or \
         negative parameter value is added to the standard number of digits (FLT_DIG or \
         DBL_DIG as appropriate). Any value greater than zero selects precise output mode.",
    ),
    vartype: "integer",
    accepted_values: &["0", "1", "2", "3"],
    min_val: Some("-15"),
    max_val: Some("3"),
    enumvals: None,
    boot_val: "1",
};

const SYNCHRONIZE_SEQSCANS_META: SettingMeta = SettingMeta {
    known: KnownSetting::SynchronizeSeqscans,
    name: "synchronize_seqscans",
    display_name: "synchronize_seqscans",
    unit: None,
    category: PREVIOUS_VERSIONS,
    short_desc: "Enable synchronized sequential scans.",
    extra_desc: None,
    vartype: "bool",
    accepted_values: &["on", "off", "true", "false"],
    min_val: None,
    max_val: None,
    enumvals: None,
    boot_val: "on",
};

pub(crate) const SETTINGS: &[SettingMeta] = &[
    SEARCH_PATH_META,
    ROW_SECURITY_META,
    STATEMENT_TIMEOUT_META,
    LOCK_TIMEOUT_META,
    IDLE_IN_TRANSACTION_SESSION_TIMEOUT_META,
    TRANSACTION_TIMEOUT_META,
    DATE_STYLE_META,
    INTERVAL_STYLE_META,
    EXTRA_FLOAT_DIGITS_META,
    SYNCHRONIZE_SEQSCANS_META,
];

impl KnownSetting {
    fn meta(self) -> &'static SettingMeta {
        match self {
            KnownSetting::SearchPath => &SEARCH_PATH_META,
            KnownSetting::RowSecurity => &ROW_SECURITY_META,
            KnownSetting::StatementTimeout => &STATEMENT_TIMEOUT_META,
            KnownSetting::LockTimeout => &LOCK_TIMEOUT_META,
            KnownSetting::IdleInTransactionSessionTimeout => {
                &IDLE_IN_TRANSACTION_SESSION_TIMEOUT_META
            }
            KnownSetting::TransactionTimeout => &TRANSACTION_TIMEOUT_META,
            KnownSetting::DateStyle => &DATE_STYLE_META,
            KnownSetting::IntervalStyle => &INTERVAL_STYLE_META,
            KnownSetting::ExtraFloatDigits => &EXTRA_FLOAT_DIGITS_META,
            KnownSetting::SynchronizeSeqscans => &SYNCHRONIZE_SEQSCANS_META,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        self.meta().name
    }

    fn accepted_values(self) -> &'static [&'static str] {
        self.meta().accepted_values
    }

    pub(crate) fn default_value(self) -> SettingValue {
        match self {
            KnownSetting::SearchPath => {
                SettingValue::SearchPath(search_path(&[self.meta().boot_val.to_string()]))
            }
            KnownSetting::RowSecurity => SettingValue::RowSecurity(true),
            KnownSetting::StatementTimeout
            | KnownSetting::LockTimeout
            | KnownSetting::IdleInTransactionSessionTimeout
            | KnownSetting::TransactionTimeout
            | KnownSetting::DateStyle
            | KnownSetting::IntervalStyle
            | KnownSetting::ExtraFloatDigits
            | KnownSetting::SynchronizeSeqscans => {
                SettingValue::Text(self.meta().boot_val.to_string())
            }
        }
    }

    pub(crate) fn parse(self, values: &[String]) -> Result<SettingValue, HeadError> {
        match self {
            KnownSetting::SearchPath => Ok(SettingValue::SearchPath(search_path(values))),
            KnownSetting::RowSecurity => row_security_value(self.name(), values),
            KnownSetting::StatementTimeout
            | KnownSetting::LockTimeout
            | KnownSetting::IdleInTransactionSessionTimeout
            | KnownSetting::TransactionTimeout
            | KnownSetting::DateStyle
            | KnownSetting::IntervalStyle
            | KnownSetting::ExtraFloatDigits
            | KnownSetting::SynchronizeSeqscans => {
                without_effect(self.name(), self.accepted_values(), values)
            }
        }
    }

    pub(crate) fn lookup(name: &SettingName) -> Result<KnownSetting, HeadError> {
        SETTINGS
            .iter()
            .find(|meta| meta.name == name.as_str())
            .map(|meta| meta.known)
            .ok_or_else(|| unrecognized(name.as_str()))
    }
}

#[derive(Debug, Clone)]
pub(crate) enum SettingValue {
    SearchPath(Vec<SchemaName>),
    RowSecurity(bool),
    Text(String),
}

impl SettingValue {
    pub(crate) fn display(&self) -> String {
        match self {
            SettingValue::SearchPath(path) => path
                .iter()
                .map(|schema| schema.as_str().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            SettingValue::RowSecurity(value) => if *value { "on" } else { "off" }.to_string(),
            SettingValue::Text(text) => text.clone(),
        }
    }
}

pub(crate) fn unrecognized(name: &str) -> HeadError {
    HeadError::raise(PgError::UnrecognizedConfigurationParameter(
        name.to_string(),
    ))
}

fn unsupported_set(name: &'static str, values: &[String]) -> HeadError {
    HeadError::not_supported(NotSupportedFeature::SetVariableTo {
        name,
        values: values.to_vec(),
    })
}

fn without_effect(
    name: &'static str,
    allowed: &[&str],
    values: &[String],
) -> Result<SettingValue, HeadError> {
    match values {
        [value]
            if allowed
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(value)) =>
        {
            Ok(SettingValue::Text(value.clone()))
        }
        _ => Err(unsupported_set(name, values)),
    }
}

fn row_security_value(name: &'static str, values: &[String]) -> Result<SettingValue, HeadError> {
    match values {
        [value] => match value.to_ascii_lowercase().as_str() {
            "on" | "true" => Ok(SettingValue::RowSecurity(true)),
            "off" | "false" => Ok(SettingValue::RowSecurity(false)),
            _ => Err(unsupported_set(name, values)),
        },
        _ => Err(unsupported_set(name, values)),
    }
}

fn search_path(values: &[String]) -> Vec<SchemaName> {
    values
        .iter()
        .flat_map(|value| value.split(','))
        .map(|schema| schema.trim().trim_matches('"').to_string())
        .filter(|schema| !schema.is_empty())
        .map(SchemaName::from_setting)
        .collect()
}
