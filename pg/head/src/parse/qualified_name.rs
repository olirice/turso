use std::iter::Peekable;
use std::str::Chars;

use crate::error::{HeadError, NotSupportedFeature, PgError};
use crate::ident::{SchemaName, TableName};
use crate::parse::statement::{RelationName, RelationSchema};
use crate::parse::PROOF;

const MAX_IDENTIFIER_BYTES: usize = 63;

pub(crate) fn relation_name(raw: &str) -> Result<RelationName, HeadError> {
    let parts = components(raw)?;
    match parts.as_slice() {
        [name] => Ok(RelationName {
            name: table_name(name)?,
            schema: RelationSchema::Unqualified,
            location: None,
        }),
        [schema, name] => {
            let name = table_name(name)?;
            match schema_name(schema)?.as_str() {
                "public" => Ok(RelationName {
                    name,
                    schema: RelationSchema::Public,
                    location: None,
                }),
                "pg_catalog" => Ok(RelationName {
                    name,
                    schema: RelationSchema::PgCatalog,
                    location: None,
                }),
                _ => Err(HeadError::raise(PgError::UndefinedRelation {
                    qualified: parts.join("."),
                })),
            }
        }
        [_, _, _] => Err(HeadError::raise(PgError::CrossDatabaseReference(
            parts.join("."),
        ))),
        _ => Err(HeadError::raise(PgError::ImproperRelationName(
            parts.join("."),
        ))),
    }
}

pub(crate) fn undefined_relation(relation: &RelationName) -> HeadError {
    let name = relation.name.render_message();
    let qualified = match relation.schema {
        RelationSchema::Unqualified => name.as_message().to_string(),
        RelationSchema::Public => format!("public.{}", name.as_message()),
        RelationSchema::PgCatalog => format!("pg_catalog.{}", name.as_message()),
    };
    HeadError::raise(PgError::UndefinedRelation { qualified })
}

fn components(raw: &str) -> Result<Vec<String>, HeadError> {
    let mut chars = raw.chars().peekable();
    let mut parts = Vec::new();
    loop {
        while chars.next_if(|c| is_pg_name_space(*c)).is_some() {}
        parts.push(component(&mut chars)?);
        while chars.next_if(|c| is_pg_name_space(*c)).is_some() {}
        match chars.next() {
            None => return Ok(parts),
            Some('.') => continue,
            Some(_) => return Err(invalid_name_syntax()),
        }
    }
}

fn component(chars: &mut Peekable<Chars>) -> Result<String, HeadError> {
    match chars.peek() {
        None => Err(invalid_name_syntax()),
        Some('"') => {
            chars.next();
            let mut quoted = String::new();
            loop {
                match chars.next() {
                    None => return Err(invalid_name_syntax()),
                    Some('"') if chars.next_if_eq(&'"').is_some() => quoted.push('"'),
                    Some('"') => return Ok(quoted),
                    Some(other) => quoted.push(other),
                }
            }
        }
        Some(_) => {
            let mut unquoted = String::new();
            while let Some(&next) = chars.peek() {
                if next == '.' || is_pg_name_space(next) {
                    break;
                }
                unquoted.push(fold_ascii(next));
                chars.next();
            }
            if unquoted.is_empty() {
                return Err(invalid_name_syntax());
            }
            Ok(unquoted)
        }
    }
}

fn schema_name(component: &str) -> Result<SchemaName, HeadError> {
    SchemaName::from_parse_tree(PROOF, refuse_over_length(component)?)
}

fn table_name(component: &str) -> Result<TableName, HeadError> {
    TableName::from_parse_tree(PROOF, refuse_over_length(component)?)
}

fn refuse_over_length(component: &str) -> Result<String, HeadError> {
    if component.len() > MAX_IDENTIFIER_BYTES {
        return Err(HeadError::not_supported(
            NotSupportedFeature::IdentifierTooLong,
        ));
    }
    Ok(component.to_string())
}

fn invalid_name_syntax() -> HeadError {
    HeadError::raise(PgError::InvalidNameSyntax)
}

fn is_pg_name_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0B' | '\x0C')
}

fn fold_ascii(c: char) -> char {
    if c.is_ascii_uppercase() {
        c.to_ascii_lowercase()
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(result: Result<RelationName, HeadError>) -> String {
        match result {
            Ok(relation) => format!(
                "{}{}",
                match relation.schema {
                    RelationSchema::Unqualified => String::new(),
                    RelationSchema::Public => "public.".to_string(),
                    RelationSchema::PgCatalog => "pg_catalog.".to_string(),
                },
                relation.name.render_message().as_message()
            ),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn every_verified_qualified_name_case_matches_postgresql_18() {
        let over_63 = "a".repeat(64);
        let cases: &[(&str, &str)] = &[
            ("t", "t"),
            ("T", "t"),
            ("\"t\"", "t"),
            ("\"Mixed Case\"", "Mixed Case"),
            ("\"quo\"\"ted\"", "quo\"ted"),
            ("café", "café"),
            ("\"café\"", "café"),
            ("public.t", "public.t"),
            ("\"public\".t", "public.t"),
            ("\"public\".\"t\"", "public.t"),
            ("pg_catalog.t", "pg_catalog.t"),
            (" public . t ", "public.t"),
            (
                "other_schema.t",
                "42P01: relation \"other_schema.t\" does not exist",
            ),
            (
                "db.public.t",
                "0A000: cross-database references are not implemented: \"db.public.t\"",
            ),
            (
                "a.b.c.d",
                "42601: improper relation name (too many dotted names): a.b.c.d",
            ),
            ("", "42602: invalid name syntax"),
            (".t", "42602: invalid name syntax"),
            ("public.t.", "42602: invalid name syntax"),
            ("\"t", "42602: invalid name syntax"),
            (
                over_63.as_str(),
                "0A000: identifiers longer than 63 bytes is not supported",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(&render(relation_name(input)), expected, "input: {input:?}");
        }
    }
}
