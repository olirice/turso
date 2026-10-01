const PRIVILEGE_LETTERS: &str = "arwdDxtmXUCTc";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Privileges(u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ObjectKind {
    Table,
    Schema,
}

#[derive(Debug, Clone)]
pub(crate) enum PrivilegeKeyword {
    Insert,
    Select,
    Update,
    Delete,
    Truncate,
    References,
    Trigger,
    Maintain,
    Usage,
    Create,
    Other(String),
}

impl PrivilegeKeyword {
    pub(crate) fn parsed(name: &str) -> Self {
        match name.to_ascii_uppercase().as_str() {
            "INSERT" => PrivilegeKeyword::Insert,
            "SELECT" => PrivilegeKeyword::Select,
            "UPDATE" => PrivilegeKeyword::Update,
            "DELETE" => PrivilegeKeyword::Delete,
            "TRUNCATE" => PrivilegeKeyword::Truncate,
            "REFERENCES" => PrivilegeKeyword::References,
            "TRIGGER" => PrivilegeKeyword::Trigger,
            "MAINTAIN" => PrivilegeKeyword::Maintain,
            "USAGE" => PrivilegeKeyword::Usage,
            "CREATE" => PrivilegeKeyword::Create,
            _ => PrivilegeKeyword::Other(name.to_string()),
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        match self {
            PrivilegeKeyword::Insert => "INSERT",
            PrivilegeKeyword::Select => "SELECT",
            PrivilegeKeyword::Update => "UPDATE",
            PrivilegeKeyword::Delete => "DELETE",
            PrivilegeKeyword::Truncate => "TRUNCATE",
            PrivilegeKeyword::References => "REFERENCES",
            PrivilegeKeyword::Trigger => "TRIGGER",
            PrivilegeKeyword::Maintain => "MAINTAIN",
            PrivilegeKeyword::Usage => "USAGE",
            PrivilegeKeyword::Create => "CREATE",
            PrivilegeKeyword::Other(name) => name,
        }
    }
}

impl Privileges {
    pub(crate) const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    pub(crate) fn from_keyword(keyword: &PrivilegeKeyword, kind: ObjectKind) -> Option<Self> {
        let letter = match (keyword, kind) {
            (PrivilegeKeyword::Insert, ObjectKind::Table) => 'a',
            (PrivilegeKeyword::Select, ObjectKind::Table) => 'r',
            (PrivilegeKeyword::Update, ObjectKind::Table) => 'w',
            (PrivilegeKeyword::Delete, ObjectKind::Table) => 'd',
            (PrivilegeKeyword::Truncate, ObjectKind::Table) => 'D',
            (PrivilegeKeyword::References, ObjectKind::Table) => 'x',
            (PrivilegeKeyword::Trigger, ObjectKind::Table) => 't',
            (PrivilegeKeyword::Maintain, ObjectKind::Table) => 'm',
            (PrivilegeKeyword::Usage, ObjectKind::Schema) => 'U',
            (PrivilegeKeyword::Create, ObjectKind::Schema) => 'C',
            _ => return None,
        };
        Self::from_letter(letter)
    }

    pub(crate) fn all_on(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Table => Self::from_bits(0x00FF),
            ObjectKind::Schema => Self::from_bits(0x0600),
        }
    }

    fn constant(letters: &str) -> Self {
        letters.chars().fold(Privileges(0), |acc, letter| {
            acc.union(Self::from_letter(letter).unwrap_or(Privileges(0)))
        })
    }

    #[cfg(test)]
    pub(crate) fn parse(text: &str) -> Result<Self, crate::error::HeadError> {
        let mut result = Privileges(0);
        for ch in text.chars() {
            if let Some(priv_bit) = Self::from_letter(ch) {
                result = result.union(priv_bit);
            } else {
                return Err(crate::error::HeadError::raise(
                    crate::error::PgError::UnknownPrivilegeLetter(ch),
                ));
            }
        }
        Ok(result)
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn bits(self) -> u16 {
        self.0
    }

    pub(crate) fn letters(self) -> String {
        PRIVILEGE_LETTERS
            .chars()
            .enumerate()
            .filter(|(index, _)| self.0 & (1 << index) != 0)
            .map(|(_, letter)| letter)
            .collect()
    }

    fn from_letter(letter: char) -> Option<Self> {
        PRIVILEGE_LETTERS
            .chars()
            .position(|candidate| candidate == letter)
            .map(|index| Self(1 << index))
    }
}

impl ObjectKind {
    pub(crate) fn class_oid(self) -> i64 {
        match self {
            ObjectKind::Table => 1259,
            ObjectKind::Schema => 2615,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            ObjectKind::Table => "table",
            ObjectKind::Schema => "schema",
        }
    }
}

pub(crate) fn default_acl_entries(
    objtype: u8,
    owner: crate::catalog::Oid,
) -> Option<Vec<crate::catalog::AclEntry>> {
    use crate::catalog::{AclEntry, Grantee};
    let (public, owner_letters): (Option<&str>, &str) = match objtype {
        b'n' => (None, "UC"),
        b'r' => (None, "arwdDxtm"),
        b's' => (None, "rwU"),
        b'f' => (Some("X"), "X"),
        b'T' => (Some("U"), "U"),
        b'l' => (Some("U"), "U"),
        b'F' => (None, "U"),
        b'S' => (None, "U"),
        _ => return None,
    };
    let mut entries = Vec::with_capacity(2);
    if let Some(letters) = public {
        entries.push(AclEntry {
            grantee: Grantee::Public,
            grantor: owner,
            privileges: Privileges::constant(letters),
        });
    }
    entries.push(AclEntry {
        grantee: Grantee::Role(owner),
        grantor: owner,
        privileges: Privileges::constant(owner_letters),
    });
    Some(entries)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    #[test]
    fn all_on_grants_every_privilege_of_the_kind() {
        assert_eq!(
            super::Privileges::all_on(super::ObjectKind::Table).letters(),
            "arwdDxtm"
        );
        assert_eq!(
            super::Privileges::all_on(super::ObjectKind::Schema).letters(),
            "UC"
        );
    }

    use super::*;

    fn letters(text: &str) -> Privileges {
        Privileges::parse(text).expect("valid letters")
    }

    #[test]
    fn letters_render_in_postgres_order_whatever_order_they_were_granted() {
        assert_eq!(letters("mdra").letters(), "ardm");
        assert_eq!(letters("CU").letters(), "UC");
    }
}
