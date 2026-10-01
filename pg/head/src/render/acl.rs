use crate::catalog::{AclEntry, Catalog, Grantee, Oid};
use crate::security::privileges::Privileges;

pub(super) fn render_acl(catalog: &Catalog, entries: &[AclEntry]) -> String {
    let name = |oid: Oid| {
        catalog
            .role_name(oid)
            .map_or_else(|| oid.get().to_string(), |name| name.as_str().to_string())
    };
    let items = entries
        .iter()
        .map(|entry| {
            let grantee = match entry.grantee {
                Grantee::Public => None,
                Grantee::Role(oid) => Some(name(oid)),
            };
            (grantee, entry.privileges, name(entry.grantor))
        })
        .collect::<Vec<_>>();
    acl_array(
        &items
            .iter()
            .map(|(grantee, privileges, grantor)| {
                (grantee.as_deref(), *privileges, grantor.as_str())
            })
            .collect::<Vec<_>>(),
    )
}

fn acl_array(items: &[(Option<&str>, Privileges, &str)]) -> String {
    let elements = items
        .iter()
        .map(|(grantee, privileges, grantor)| {
            let grantee = grantee.map_or_else(String::new, |name| {
                render_acl_name(name).as_acl().to_string()
            });
            let item = format!(
                "{grantee}={}/{}",
                privileges.letters(),
                render_acl_name(grantor).as_acl()
            );
            encode_array_element(&item)
        })
        .collect::<Vec<_>>();
    format!("{{{}}}", elements.join(","))
}

struct RenderedAclName(String);

impl RenderedAclName {
    fn as_acl(&self) -> &str {
        &self.0
    }
}

fn render_acl_name(name: &str) -> RenderedAclName {
    let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain {
        return RenderedAclName(name.to_string());
    }
    RenderedAclName(format!("\"{}\"", name.replace('"', "\"\"")))
}

fn encode_array_element(value: &str) -> String {
    let needs_quotes = value.is_empty()
        || value
            .chars()
            .any(|c| matches!(c, '"' | '\\' | ',' | '{' | '}') || c.is_whitespace());
    if !needs_quotes {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
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

    fn letters(text: &str) -> Privileges {
        Privileges::parse(text).expect("valid letters")
    }

    #[test]
    fn acl_text_matches_what_postgres_18_prints() {
        let owner = letters("arwdDxtm");
        assert_eq!(
            acl_array(&[
                (Some("postgres"), owner, "postgres"),
                (None, letters("rwd"), "postgres"),
                (Some("Bob Q"), letters("wd"), "postgres"),
            ]),
            r#"{postgres=arwdDxtm/postgres,=rwd/postgres,"\"Bob Q\"=wd/postgres"}"#
        );
        assert_eq!(
            acl_array(&[
                (Some("postgres"), owner, "postgres"),
                (Some("we\"ird"), letters("r"), "postgres")
            ]),
            r#"{postgres=arwdDxtm/postgres,"\"we\"\"ird\"=r/postgres"}"#
        );
    }

    #[test]
    fn render_acl_name_leaves_a_bare_keyword_unquoted_unlike_quote_ident() {
        assert_eq!(render_acl_name("select").as_acl(), "select");
        assert_eq!(
            crate::render::ident::render_identifier("select").as_sql(),
            "\"select\""
        );
    }
}
