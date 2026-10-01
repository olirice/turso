use pg_query::protobuf::KeywordKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedIdent(String);

impl RenderedIdent {
    pub(crate) fn as_sql(&self) -> &str {
        &self.0
    }
}

pub(crate) fn render_identifier(name: &str) -> RenderedIdent {
    let plain = name
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if plain && !is_reserved_word(name) {
        return RenderedIdent(name.to_string());
    }
    RenderedIdent(format!("\"{}\"", name.replace('"', "\"\"")))
}

pub(crate) fn render_qualified(schema: &str, name: &str) -> RenderedIdent {
    RenderedIdent(format!(
        "{}.{}",
        render_identifier(schema).as_sql(),
        render_identifier(name).as_sql()
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedLiteral(String);

impl RenderedLiteral {
    pub(crate) fn as_sql(&self) -> &str {
        &self.0
    }
}

pub(crate) fn render_literal(text: &str) -> RenderedLiteral {
    RenderedLiteral(format!("'{}'", text.replace('\'', "''")))
}

pub(crate) fn render_portable_literal(text: &str) -> RenderedLiteral {
    let has_backslash = text.contains('\\');
    let mut out = String::with_capacity(text.len() + 2);
    if has_backslash {
        out.push('E');
    }
    out.push('\'');
    for ch in text.chars() {
        match ch {
            '\'' => out.push_str("''"),
            '\\' if has_backslash => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out.push('\'');
    RenderedLiteral(out)
}

fn is_reserved_word(name: &str) -> bool {
    pg_query::scan(name).is_ok_and(|scanned| match scanned.tokens.as_slice() {
        [token] => !matches!(
            KeywordKind::try_from(token.keyword_kind),
            Ok(KeywordKind::NoKeyword | KeywordKind::UnreservedKeyword)
        ),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_identifier_matches_postgres_18_quote_ident() {
        let cases: &[(&str, &str)] = &[
            ("users", "users"),
            ("Users", "\"Users\""),
            ("u1", "u1"),
            ("1u", "\"1u\""),
            ("_u", "_u"),
            ("u\"v", "\"u\"\"v\""),
            ("select", "\"select\""),
            ("name", "name"),
        ];
        for (input, expected) in cases {
            assert_eq!(render_identifier(input).as_sql(), *expected);
        }
    }

    #[test]
    fn render_qualified_quotes_each_part_separately() {
        assert_eq!(render_qualified("public", "notes").as_sql(), "public.notes");
        assert_eq!(
            render_qualified("public", "Notes").as_sql(),
            "public.\"Notes\""
        );
    }

    #[test]
    fn render_literal_doubles_only_the_single_quote() {
        let cases: &[(&str, &str)] = &[("abc", "'abc'"), ("it's", "'it''s'"), ("", "''")];
        for (input, expected) in cases {
            assert_eq!(render_literal(input).as_sql(), *expected);
        }
    }

    #[test]
    fn render_portable_literal_matches_postgres_18_quote_literal() {
        let cases: &[(&str, &str)] = &[
            ("hello", "'hello'"),
            ("it's", "'it''s'"),
            ("back\\slash", "E'back\\\\slash'"),
        ];
        for (input, expected) in cases {
            assert_eq!(render_portable_literal(input).as_sql(), *expected);
        }
    }
}
