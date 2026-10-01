use crate::engine::store::FromCatalog;
use crate::error::HeadError;
use crate::head::FromStartup;
use crate::parse::FromParser;
use crate::render::ident::{self, RenderedIdent};

const MAX_IDENTIFIER_BYTES: usize = 63;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Ident(String);

impl Ident {
    pub(crate) fn from_parse_tree(
        _: FromParser,
        name: impl Into<String>,
    ) -> Result<Self, HeadError> {
        let name = name.into();
        if name.len() > MAX_IDENTIFIER_BYTES {
            return Err(HeadError::internal(
                "admit refuses an identifier over 63 bytes before it reaches from_parse_tree",
            ));
        }
        Ok(Ident(name))
    }

    pub(crate) fn from_catalog(_: FromCatalog, name: impl Into<String>) -> Self {
        Ident(name.into())
    }

    pub(crate) fn from_startup_user(_: FromStartup, name: impl Into<String>) -> Self {
        Ident(name.into())
    }

    pub(crate) fn from_setting(name: impl Into<String>) -> Self {
        let name = name.into();
        let mut clipped = name.len().min(MAX_IDENTIFIER_BYTES);
        while clipped > 0 && !name.is_char_boundary(clipped) {
            clipped -= 1;
        }
        Ident(name.get(..clipped).unwrap_or_default().to_string())
    }

    pub(crate) fn literal(name: impl Into<String>) -> Self {
        Ident(name.into())
    }

    /// PostgreSQL's own `makeObjectName` (used for both primary keys, whose
    /// `middle` is `None`, and not-null constraints, whose `middle` is the
    /// column name): only the table name is ever clipped to fit, `middle`
    /// and the suffix are kept whole.
    fn generated(
        table: &Ident,
        middle: Option<&str>,
        label: GeneratedLabel,
        attempt: usize,
    ) -> Result<Self, HeadError> {
        let label = match attempt {
            0 => label.suffix().to_string(),
            number => format!("{}{number}", label.suffix()),
        };
        let overhead = middle.map_or(0, |middle| middle.len() + 1) + label.len() + 1;
        let budget = MAX_IDENTIFIER_BYTES.saturating_sub(overhead);
        let mut clipped = table.0.len().min(budget);
        while clipped > 0 && !table.0.is_char_boundary(clipped) {
            clipped -= 1;
        }
        let Some(prefix) = table.0.get(..clipped) else {
            return Err(HeadError::internal(
                "a generated identifier could not clip its table name at a character boundary",
            ));
        };
        Ok(Ident(match middle {
            Some(middle) => format!("{prefix}_{middle}_{label}"),
            None => format!("{prefix}_{label}"),
        }))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn render(&self) -> RenderedIdent {
        ident::render_identifier(&self.0)
    }

    fn render_message(&self) -> MessageName {
        MessageName(self.0.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum GeneratedLabel {
    PrimaryKey,
    NotNull,
}

impl GeneratedLabel {
    fn suffix(self) -> &'static str {
        match self {
            GeneratedLabel::PrimaryKey => "pkey",
            GeneratedLabel::NotNull => "not_null",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MessageName(String);

impl MessageName {
    pub(crate) fn as_message(&self) -> &str {
        &self.0
    }
}

macro_rules! kinded_ident {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
        pub(crate) struct $name(Ident);

        impl $name {
            pub(crate) fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }
    };
}

kinded_ident!(RoleName);
kinded_ident!(SchemaName);
kinded_ident!(TableName);
kinded_ident!(ColumnName);
kinded_ident!(PolicyName);
kinded_ident!(ConstraintName);
kinded_ident!(FunctionName);
kinded_ident!(TypeName);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PreparedName(Ident);

impl RoleName {
    pub(crate) fn from_parse_tree(
        proof: FromParser,
        name: impl Into<String>,
    ) -> Result<Self, HeadError> {
        Ok(RoleName(Ident::from_parse_tree(proof, name)?))
    }

    pub(crate) fn from_catalog(proof: FromCatalog, name: impl Into<String>) -> Self {
        RoleName(Ident::from_catalog(proof, name))
    }

    pub(crate) fn from_startup_user(proof: FromStartup, name: impl Into<String>) -> Self {
        RoleName(Ident::from_startup_user(proof, name))
    }

    pub(crate) fn render_message(&self) -> MessageName {
        self.0.render_message()
    }
}

impl SchemaName {
    /// The schema user objects live in.
    pub(crate) fn public() -> Self {
        Self::literal("public")
    }

    /// PostgreSQL's system catalog schema.
    pub(crate) fn pg_catalog() -> Self {
        Self::literal("pg_catalog")
    }

    pub(crate) fn literal(name: &'static str) -> Self {
        SchemaName(Ident::literal(name))
    }

    pub(crate) fn from_parse_tree(
        proof: FromParser,
        name: impl Into<String>,
    ) -> Result<Self, HeadError> {
        Ok(SchemaName(Ident::from_parse_tree(proof, name)?))
    }

    pub(crate) fn from_catalog(proof: FromCatalog, name: impl Into<String>) -> Self {
        SchemaName(Ident::from_catalog(proof, name))
    }

    pub(crate) fn from_setting(name: impl Into<String>) -> Self {
        SchemaName(Ident::from_setting(name))
    }

    pub(crate) fn render_message(&self) -> MessageName {
        self.0.render_message()
    }
}

impl TableName {
    pub(crate) fn ident(&self) -> &Ident {
        &self.0
    }

    pub(crate) fn literal(name: &'static str) -> Self {
        TableName(Ident::literal(name))
    }

    pub(crate) fn from_parse_tree(
        proof: FromParser,
        name: impl Into<String>,
    ) -> Result<Self, HeadError> {
        Ok(TableName(Ident::from_parse_tree(proof, name)?))
    }

    pub(crate) fn from_catalog(proof: FromCatalog, name: impl Into<String>) -> Self {
        TableName(Ident::from_catalog(proof, name))
    }

    pub(crate) fn render_message(&self) -> MessageName {
        self.0.render_message()
    }
}

impl ColumnName {
    /// The `tableoid` system column every relation exposes.
    pub(crate) fn tableoid() -> Self {
        Self::literal("tableoid")
    }

    /// The display name of an unaliased `array_agg(...)` select item.
    pub(crate) fn array_agg() -> Self {
        Self::literal("array_agg")
    }

    pub(crate) fn literal(name: impl Into<String>) -> Self {
        ColumnName(Ident::literal(name))
    }

    pub(crate) fn from_parse_tree(
        proof: FromParser,
        name: impl Into<String>,
    ) -> Result<Self, HeadError> {
        Ok(ColumnName(Ident::from_parse_tree(proof, name)?))
    }

    pub(crate) fn from_catalog(proof: FromCatalog, name: impl Into<String>) -> Self {
        ColumnName(Ident::from_catalog(proof, name))
    }

    pub(crate) fn render(&self) -> RenderedIdent {
        self.0.render()
    }

    pub(crate) fn render_message(&self) -> MessageName {
        self.0.render_message()
    }
}

impl FunctionName {
    /// `array_agg`, the one aggregate whose `ORDER BY` the parser rewrites.
    pub(crate) fn array_agg() -> Self {
        Self::literal("array_agg")
    }

    pub(crate) fn literal(name: &'static str) -> Self {
        FunctionName(Ident::literal(name))
    }

    pub(crate) fn from_parse_tree(
        proof: FromParser,
        name: impl Into<String>,
    ) -> Result<Self, HeadError> {
        Ok(FunctionName(Ident::from_parse_tree(proof, name)?))
    }
}

impl TypeName {
    pub(crate) fn from_parse_tree(
        proof: FromParser,
        name: impl Into<String>,
    ) -> Result<Self, HeadError> {
        Ok(TypeName(Ident::from_parse_tree(proof, name)?))
    }

    #[cfg(test)]
    pub(crate) fn literal(name: &'static str) -> Self {
        TypeName(Ident::literal(name))
    }
}

impl PolicyName {
    pub(crate) fn from_parse_tree(
        proof: FromParser,
        name: impl Into<String>,
    ) -> Result<Self, HeadError> {
        Ok(PolicyName(Ident::from_parse_tree(proof, name)?))
    }

    pub(crate) fn from_catalog(proof: FromCatalog, name: impl Into<String>) -> Self {
        PolicyName(Ident::from_catalog(proof, name))
    }

    pub(crate) fn render_message(&self) -> MessageName {
        self.0.render_message()
    }
}

impl PreparedName {
    pub(crate) fn from_parse_tree(
        proof: FromParser,
        name: impl Into<String>,
    ) -> Result<Self, HeadError> {
        Ok(PreparedName(Ident::from_parse_tree(proof, name)?))
    }

    pub(crate) fn render_message(&self) -> MessageName {
        self.0.render_message()
    }
}

impl ConstraintName {
    pub(crate) fn ident(&self) -> &Ident {
        &self.0
    }

    pub(crate) fn from_catalog(proof: FromCatalog, name: impl Into<String>) -> Self {
        ConstraintName(Ident::from_catalog(proof, name))
    }

    pub(crate) fn generated(
        table: &TableName,
        middle: Option<&str>,
        label: GeneratedLabel,
        attempt: usize,
    ) -> Result<Self, HeadError> {
        Ok(ConstraintName(Ident::generated(
            &table.0, middle, label, attempt,
        )?))
    }

    pub(crate) fn render(&self) -> RenderedIdent {
        self.0.render()
    }

    pub(crate) fn render_message(&self) -> MessageName {
        self.0.render_message()
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
    fn generated_names_never_exceed_63_bytes() {
        let table = TableName::literal("t");
        assert_eq!(
            ConstraintName::generated(&table, None, GeneratedLabel::PrimaryKey, 0)
                .expect("a short table name never fails to generate")
                .as_str(),
            "t_pkey"
        );
        assert_eq!(
            ConstraintName::generated(&table, None, GeneratedLabel::PrimaryKey, 1)
                .expect("a short table name never fails to generate")
                .as_str(),
            "t_pkey1"
        );
    }

    #[test]
    fn generated_names_clip_a_long_table_name_at_a_character_boundary() {
        let table = TableName::literal(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        let generated = ConstraintName::generated(&table, None, GeneratedLabel::PrimaryKey, 0)
            .expect("clipping a long table name never fails");
        assert_eq!(generated.as_str().len(), MAX_IDENTIFIER_BYTES);
        assert!(generated.as_str().ends_with("_pkey"));
    }

    #[test]
    fn not_null_names_match_postgres_18s_own_table_column_not_null_shape() {
        let table = TableName::literal("notes");
        assert_eq!(
            ConstraintName::generated(&table, Some("owner"), GeneratedLabel::NotNull, 0)
                .expect("a short table and column name never fails to generate")
                .as_str(),
            "notes_owner_not_null"
        );
    }

    #[test]
    fn not_null_names_clip_only_the_table_name_a_character_boundary_like_postgres_18() {
        let table = TableName::literal(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        let generated =
            ConstraintName::generated(&table, Some("owner"), GeneratedLabel::NotNull, 0)
                .expect("clipping a long table name never fails");
        assert_eq!(generated.as_str().len(), MAX_IDENTIFIER_BYTES);
        assert!(generated.as_str().ends_with("_owner_not_null"));
    }
}
