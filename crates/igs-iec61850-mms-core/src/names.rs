// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Names of the IEC 61850 model and of their MMS addresses (IEC 61850-7-2 §6.2.3.3, IEC 61850-8-1 §7.3 and
//! §8.1.3.2; task A1, requirement MMS-001).
//!
//! Three limits, kept apart as MMS-001 asks: an IEC 61850 name (an ObjectName, at most 64 characters), an MMS
//! identifier (the domain and the item of an MMS address, at most 64 characters, the IEC profile of
//! IEC 61850-8-1 Annex F.3.2), and an object reference (at most 129 octets, §8.1.3.2). The application string
//! values have their own limit and are not here.

use std::fmt;

/// The longest IEC 61850 name, in characters (7-2 §6.2.3.3, ObjectName: VisString64).
pub const MAX_NAME: usize = 64;

/// The longest MMS identifier of the IEC 61850 profile, in characters (8-1 Annex F.3.2).
pub const MAX_IDENTIFIER: usize = 64;

/// The longest object reference, in octets (7-2 §6.2.3.4 and 8-1 §8.1.3.2: VisString129).
pub const MAX_OBJECT_REFERENCE: usize = 129;

/// The functional constraints that may name an instance (7-2 Table 4 and the list of 8-1 Figure 4).
///
/// The wildcard `XX` of 7-2 is for services only, and is not mapped to MMS (8-1 Figure 4, NOTE 2).
pub const FUNCTIONAL_CONSTRAINTS: &[&str] = &[
    "ST", "MX", "SP", "SV", "CF", "DC", "SG", "SE", "SR", "OR", "BL", "EX", "CO", "RP", "BR", "LG",
    "GO", "GS", "MS", "US", "RG", "RS",
];

/// Why a name is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NameError {
    /// The name is empty.
    Empty,
    /// The name is longer than its limit.
    TooLong {
        /// The limit, in characters.
        max: usize,
        /// The length given, in characters.
        length: usize,
    },
    /// The name does not start with a letter.
    InvalidStart,
    /// A character outside the allowed set.
    InvalidCharacter(char),
    /// The wildcard functional constraint `XX`, which is not mapped to MMS.
    WildcardConstraint,
    /// A functional constraint that the standard does not define.
    UnknownConstraint(String),
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "the name is empty"),
            Self::TooLong { max, length } => {
                write!(f, "the name has {length} characters, at most {max} allowed")
            }
            Self::InvalidStart => write!(f, "the name does not start with a letter"),
            Self::InvalidCharacter(c) => write!(f, "the character {c:?} is not allowed in a name"),
            Self::WildcardConstraint => {
                write!(f, "the wildcard constraint XX is not mapped to MMS")
            }
            Self::UnknownConstraint(code) => write!(f, "{code:?} is not a functional constraint"),
        }
    }
}

impl std::error::Error for NameError {}

/// An IEC 61850 name: an ObjectName of 7-2 §6.2.3.3, of 1 to 64 characters, starting with a letter, then letters,
/// digits and underscores. The name is case sensitive.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    /// Checks a name against the ObjectName rules.
    pub fn new(text: &str) -> Result<Self, NameError> {
        validate_name(text)?;
        Ok(Self(text.to_string()))
    }

    /// The name, as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn validate_name(text: &str) -> Result<(), NameError> {
    let length = text.chars().count();
    if length == 0 {
        return Err(NameError::Empty);
    }
    if length > MAX_NAME {
        return Err(NameError::TooLong {
            max: MAX_NAME,
            length,
        });
    }
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return Err(NameError::InvalidStart),
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return Err(NameError::InvalidCharacter(c));
        }
    }
    Ok(())
}

/// A functional constraint, as its two-letter code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionalConstraint(&'static str);

impl FunctionalConstraint {
    /// The constraint of this code. `XX` is refused: it is for services only.
    pub fn new(code: &str) -> Result<Self, NameError> {
        if code == "XX" {
            return Err(NameError::WildcardConstraint);
        }
        FUNCTIONAL_CONSTRAINTS
            .iter()
            .find(|known| **known == code)
            .map(|known| Self(known))
            .ok_or_else(|| NameError::UnknownConstraint(code.to_string()))
    }

    /// The two-letter code.
    pub fn code(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for FunctionalConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// The identifier of an MMS address: letters, digits, `$` and `_` (the BasicIdentifier of 8-1 §24.2.2.2.1), at
/// most [`MAX_IDENTIFIER`] characters, starting with a letter. An IEC 61850 flattened name is one of these.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MmsIdentifier(String);

impl MmsIdentifier {
    /// Checks an identifier.
    pub fn new(text: &str) -> Result<Self, NameError> {
        let length = text.chars().count();
        if length == 0 {
            return Err(NameError::Empty);
        }
        if length > MAX_IDENTIFIER {
            return Err(NameError::TooLong {
                max: MAX_IDENTIFIER,
                length,
            });
        }
        match text.chars().next() {
            Some(first) if first.is_ascii_alphabetic() => {}
            _ => return Err(NameError::InvalidStart),
        }
        for c in text.chars() {
            if !(c.is_ascii_alphanumeric() || c == '_' || c == '$') {
                return Err(NameError::InvalidCharacter(c));
            }
        }
        Ok(Self(text.to_string()))
    }

    /// The identifier, as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The scope of an MMS object name (ISO 9506-2 §7.5.2 and 8-1 §7.3.2, §8.1.3.2.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Domain-specific: the logical device.
    Domain(Name),
    /// VMD-specific: the server's own objects, written `/item`.
    Vmd,
    /// AA-specific: the objects of the association, written `@item`.
    AaSpecific,
}

/// A fully scope-qualified MMS object name: `domain/item`, `/item` or `@item` (8-1 §8.1.3.2.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MmsName {
    scope: Scope,
    item: MmsIdentifier,
}

impl MmsName {
    /// A name of a scope and an item.
    pub fn new(scope: Scope, item: MmsIdentifier) -> Self {
        Self { scope, item }
    }

    /// The scope.
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// The item.
    pub fn item(&self) -> &MmsIdentifier {
        &self.item
    }

    /// The name as text: `domain/item`, `/item` or `@item`. The text is at most [`MAX_OBJECT_REFERENCE`] octets
    /// for the names of the IEC 61850 profile; [`MmsName::parse`] checks it.
    pub fn to_text(&self) -> String {
        match &self.scope {
            Scope::Domain(domain) => format!("{domain}/{}", self.item.as_str()),
            Scope::Vmd => format!("/{}", self.item.as_str()),
            Scope::AaSpecific => format!("@{}", self.item.as_str()),
        }
    }

    /// Reads a scope-qualified name. A `/` splits the domain from the item; a leading `/` or `@` gives a VMD or
    /// AA-specific name. The text is limited to [`MAX_OBJECT_REFERENCE`] octets.
    pub fn parse(text: &str) -> Result<Self, NameError> {
        check_length(text, MAX_OBJECT_REFERENCE)?;
        if let Some(item) = text.strip_prefix('@') {
            return Ok(Self::new(Scope::AaSpecific, MmsIdentifier::new(item)?));
        }
        if let Some(item) = text.strip_prefix('/') {
            return Ok(Self::new(Scope::Vmd, MmsIdentifier::new(item)?));
        }
        match text.split_once('/') {
            Some((domain, item)) => Ok(Self::new(
                Scope::Domain(Name::new(domain)?),
                MmsIdentifier::new(item)?,
            )),
            None => Err(NameError::InvalidCharacter('/')),
        }
    }
}

impl fmt::Display for MmsName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

/// Refuses a text longer than `max` octets (the object references are ASCII, so octets are characters).
pub(crate) fn check_length(text: &str, max: usize) -> Result<(), NameError> {
    if text.len() > max {
        return Err(NameError::TooLong {
            max,
            length: text.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_a_letter_then_letters_digits_and_underscores_up_to_64() {
        assert!(Name::new("a").is_ok());
        assert!(Name::new("MMXU1").is_ok());
        assert!(Name::new("Pos_stVal_2").is_ok());
        assert!(Name::new(&"x".repeat(64)).is_ok());
        assert_eq!(
            Name::new(&"x".repeat(65)),
            Err(NameError::TooLong {
                max: 64,
                length: 65
            })
        );
        assert_eq!(Name::new(""), Err(NameError::Empty));
        assert_eq!(Name::new("1ABC"), Err(NameError::InvalidStart));
        assert_eq!(Name::new("_ABC"), Err(NameError::InvalidStart));
        assert_eq!(Name::new("A$B"), Err(NameError::InvalidCharacter('$')));
        assert_eq!(Name::new("A.B"), Err(NameError::InvalidCharacter('.')));
        assert_eq!(Name::new("é"), Err(NameError::InvalidStart));
    }

    #[test]
    fn an_mms_identifier_admits_the_dollar_sign_and_64_characters() {
        assert!(MmsIdentifier::new("LLN0$BR$EventsBRCBPreConf01$TimeofEntry").is_ok());
        assert!(MmsIdentifier::new(&format!("A{}", "b".repeat(63))).is_ok());
        assert_eq!(
            MmsIdentifier::new(&format!("A{}", "b".repeat(64))),
            Err(NameError::TooLong {
                max: 64,
                length: 65
            })
        );
        assert_eq!(
            MmsIdentifier::new("A.B"),
            Err(NameError::InvalidCharacter('.'))
        );
        assert_eq!(MmsIdentifier::new("(7)"), Err(NameError::InvalidStart));
    }

    #[test]
    fn the_functional_constraints_are_the_standard_codes_and_xx_is_refused() {
        for code in FUNCTIONAL_CONSTRAINTS {
            assert_eq!(FunctionalConstraint::new(code).unwrap().code(), *code);
        }
        assert_eq!(
            FunctionalConstraint::new("XX"),
            Err(NameError::WildcardConstraint)
        );
        assert_eq!(
            FunctionalConstraint::new("ZZ"),
            Err(NameError::UnknownConstraint("ZZ".into()))
        );
        assert_eq!(
            FunctionalConstraint::new("st"),
            Err(NameError::UnknownConstraint("st".into()))
        );
    }

    #[test]
    fn an_mms_name_has_the_three_scopes() {
        let domain = MmsName::parse("LD1/XCBR1$ST$Pos$stVal").unwrap();
        assert_eq!(domain.scope(), &Scope::Domain(Name::new("LD1").unwrap()));
        assert_eq!(domain.item().as_str(), "XCBR1$ST$Pos$stVal");
        assert_eq!(domain.to_text(), "LD1/XCBR1$ST$Pos$stVal");

        let vmd = MmsName::parse("/LLN0$RP$urcb01").unwrap();
        assert_eq!(vmd.scope(), &Scope::Vmd);
        assert_eq!(vmd.to_text(), "/LLN0$RP$urcb01");

        let aa = MmsName::parse("@PhysDev").unwrap();
        assert_eq!(aa.scope(), &Scope::AaSpecific);
        assert_eq!(aa.to_text(), "@PhysDev");
        assert!(MmsName::parse("LD1").is_err(), "no domain and no scope");
        assert!(MmsName::parse("LD1/a/b").is_err(), "one separator only");
        assert!(MmsName::parse("LD1/").is_err());
    }

    #[test]
    fn an_mms_name_fits_129_octets_with_two_identifiers_of_64() {
        // A domain of 64 and an item of 64 make 129 octets: the longest name of the profile.
        let at_limit = format!("{}/{}", "L".repeat(64), "a".repeat(64));
        assert_eq!(at_limit.len(), MAX_OBJECT_REFERENCE);
        assert!(MmsName::parse(&at_limit).is_ok());
        // An item of 65 is refused as an identifier, whatever the domain.
        let item_over = format!("{}/{}", "L".repeat(10), "a".repeat(65));
        assert_eq!(
            MmsName::parse(&item_over),
            Err(NameError::TooLong {
                max: 64,
                length: 65
            })
        );
    }
}
