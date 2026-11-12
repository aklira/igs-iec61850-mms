// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Object references of the ACSI model and their MMS addresses (IEC 61850-7-2 §6.2.3.4; IEC 61850-8-1 §7.3,
//! §8.1.3.2; task A1).
//!
//! Three forms of the same path, each with its rule:
//!
//! - the ACSI reference, `LD/LN.DO.DA(n).SubDA`: the functional constraint is not in it (7-2 §6.2.3.4), and an
//!   array element is written `(n)`;
//! - the MMS item, `LN$FC$DO$DA`, in the domain `LD`: the functional constraint follows the logical node, and the
//!   names are joined with `$` (8-1 §7.3.1, §7.3.3). An array element is not in the item: MMS reaches it by an
//!   alternate access (8-1 §7.3.3, §7.3.4), so [`ObjectReference::to_mms`] refuses it;
//! - the data reference of a report or a log, `LD/LN$FC$DO$DA(n)`: the same as the MMS item, with the array
//!   elements in it (8-1 §8.1.3.2.4).
//!
//! A control block is an object of the logical node: its reference is `LD/LN.CBName`, and its MMS address is
//! `LD/LN$RP$CBName` (or `BR`, and so on), as 8-1 §8.1.3.2.2 gives it.

use std::fmt;

use crate::names::{
    FunctionalConstraint, MmsIdentifier, MmsName, Name, NameError, Scope, MAX_OBJECT_REFERENCE,
};

/// One name of the path below the logical node, with the index of an array element when it has one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathItem {
    name: Name,
    index: Option<u32>,
}

impl PathItem {
    /// A name of the path, not an array element.
    pub fn new(name: Name) -> Self {
        Self { name, index: None }
    }

    /// An element of the array named `name`.
    pub fn element(name: Name, index: u32) -> Self {
        Self {
            name,
            index: Some(index),
        }
    }

    /// The name.
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The index of the array element, if this is one.
    pub fn index(&self) -> Option<u32> {
        self.index
    }
}

/// Why a reference or an MMS item is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefError {
    /// A name of the reference or of the item is not valid.
    Name(NameError),
    /// The text does not follow the syntax of the form: why.
    Syntax(&'static str),
    /// The text is longer than its limit (129 octets for a reference, 64 for an MMS identifier).
    TooLong {
        /// The limit.
        max: usize,
        /// The length given.
        length: usize,
    },
    /// The reference names an array element: MMS reaches it by an alternate access (8-1 §7.3.3).
    ArrayElement,
    /// The reference names a logical device only: it has no logical node, so no MMS item.
    NoLogicalNode,
    /// The item of an MMS name is not a logical node, a functional constraint and names.
    NotAnItem(&'static str),
}

impl From<NameError> for RefError {
    fn from(error: NameError) -> Self {
        match error {
            NameError::TooLong { max, length } => Self::TooLong { max, length },
            other => Self::Name(other),
        }
    }
}

impl fmt::Display for RefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(error) => write!(f, "{error}"),
            Self::Syntax(why) => write!(f, "{why}"),
            Self::TooLong { max, length } => {
                write!(f, "{length} characters, at most {max} allowed")
            }
            Self::ArrayElement => write!(
                f,
                "an array element is reached by an alternate access, not by a name"
            ),
            Self::NoLogicalNode => write!(f, "a logical device has no MMS item of its own"),
            Self::NotAnItem(why) => write!(f, "not an MMS item of the model: {why}"),
        }
    }
}

impl std::error::Error for RefError {}

/// A reference to an object of the model: a logical device, a logical node, or a path below the logical node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectReference {
    ld: Name,
    ln: Option<Name>,
    path: Vec<PathItem>,
}

impl ObjectReference {
    /// A reference to a logical device.
    pub fn logical_device(ld: Name) -> Self {
        Self {
            ld,
            ln: None,
            path: Vec::new(),
        }
    }

    /// A reference to a logical node, with the path below it.
    pub fn new(ld: Name, ln: Name, path: Vec<PathItem>) -> Self {
        Self {
            ld,
            ln: Some(ln),
            path,
        }
    }

    /// The logical device.
    pub fn logical_device_name(&self) -> &Name {
        &self.ld
    }

    /// The logical node, if the reference names one.
    pub fn logical_node(&self) -> Option<&Name> {
        self.ln.as_ref()
    }

    /// The names below the logical node, in order.
    pub fn path(&self) -> &[PathItem] {
        &self.path
    }

    /// Reads an ACSI reference: `LD`, `LD/LN`, or `LD/LN.DO[.DA…]` with array elements written `(n)`. A reference
    /// is at most 129 octets.
    pub fn parse(text: &str) -> Result<Self, RefError> {
        check(text)?;
        let Some((ld, rest)) = text.split_once('/') else {
            if text.contains(['.', '(', ')']) {
                return Err(RefError::Syntax(
                    "a logical device is named alone, without its path",
                ));
            }
            return Ok(Self::logical_device(Name::new(text)?));
        };
        let ld = Name::new(ld)?;
        let mut parts = rest.split('.');
        let Some(ln) = parts.next() else {
            return Err(RefError::Syntax("the logical node is missing"));
        };
        let ln = Name::new(ln)?;
        let path = parts.map(parse_item).collect::<Result<Vec<_>, _>>()?;
        Ok(Self::new(ld, ln, path))
    }

    /// The MMS item of an attribute with functional constraint `fc`: `LN$FC$DO$DA…` in the domain of the logical
    /// device (8-1 §7.3.1, §7.3.3, §7.3.4). A reference with an array element is refused: see
    /// [`RefError::ArrayElement`].
    pub fn to_mms(&self, fc: FunctionalConstraint) -> Result<MmsName, RefError> {
        let ln = self.ln.as_ref().ok_or(RefError::NoLogicalNode)?;
        let mut item = format!("{ln}${fc}");
        for part in &self.path {
            if part.index.is_some() {
                return Err(RefError::ArrayElement);
            }
            item.push('$');
            item.push_str(part.name.as_str());
        }
        let identifier = MmsIdentifier::new(&item)?;
        Ok(MmsName::new(Scope::Domain(self.ld.clone()), identifier))
    }

    /// The MMS item of the logical node itself: its name in the domain of the logical device (8-1 §7.3.2).
    pub fn logical_node_item(&self) -> Result<MmsName, RefError> {
        let ln = self.ln.as_ref().ok_or(RefError::NoLogicalNode)?;
        Ok(MmsName::new(
            Scope::Domain(self.ld.clone()),
            MmsIdentifier::new(ln.as_str())?,
        ))
    }

    /// Reads an MMS item of the model: a domain-scoped name `LN$FC$DO$DA…`, whose domain is the logical device.
    /// The inverse of [`ObjectReference::to_mms`].
    pub fn from_mms(name: &MmsName) -> Result<(Self, FunctionalConstraint), RefError> {
        let Scope::Domain(ld) = name.scope() else {
            return Err(RefError::NotAnItem("the name is not domain-specific"));
        };
        let mut parts = name.item().as_str().split('$');
        let ln = parts
            .next()
            .ok_or(RefError::NotAnItem("the logical node is missing"))?;
        let fc = parts
            .next()
            .ok_or(RefError::NotAnItem("the functional constraint is missing"))?;
        let fc = FunctionalConstraint::new(fc)?;
        let path = parts
            .map(|part| Name::new(part).map(PathItem::new).map_err(RefError::from))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((Self::new(ld.clone(), Name::new(ln)?, path), fc))
    }

    /// The data reference of a report or a log: `LD/LN$FC$DO$DA(n)…` (8-1 §8.1.3.2.4), at most 129 octets.
    pub fn to_data_ref(&self, fc: FunctionalConstraint) -> Result<String, RefError> {
        let ln = self.ln.as_ref().ok_or(RefError::NoLogicalNode)?;
        let mut text = format!("{}/{ln}${fc}", self.ld);
        for part in &self.path {
            text.push('$');
            text.push_str(part.name.as_str());
            if let Some(index) = part.index {
                text.push_str(&format!("({index})"));
            }
        }
        check(&text)?;
        Ok(text)
    }

    /// Reads a data reference (see [`ObjectReference::to_data_ref`]).
    pub fn parse_data_ref(text: &str) -> Result<(Self, FunctionalConstraint), RefError> {
        check(text)?;
        let (ld, rest) = text.split_once('/').ok_or(RefError::Syntax(
            "a data reference names its logical device",
        ))?;
        let ld = Name::new(ld)?;
        let mut parts = rest.split('$');
        let ln = parts
            .next()
            .ok_or(RefError::Syntax("the logical node is missing"))?;
        let fc = parts
            .next()
            .ok_or(RefError::Syntax("the functional constraint is missing"))?;
        let fc = FunctionalConstraint::new(fc)?;
        let path = parts.map(parse_item).collect::<Result<Vec<_>, _>>()?;
        Ok((Self::new(ld, Name::new(ln)?, path), fc))
    }
}

impl fmt::Display for ObjectReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ld)?;
        let Some(ln) = &self.ln else {
            return Ok(());
        };
        write!(f, "/{ln}")?;
        for part in &self.path {
            write!(f, ".{}", part.name)?;
            if let Some(index) = part.index {
                write!(f, "({index})")?;
            }
        }
        Ok(())
    }
}

/// A name of a path, with its array element when it has one: `name` or `name(n)`.
fn parse_item(text: &str) -> Result<PathItem, RefError> {
    let Some((name, rest)) = text.split_once('(') else {
        return Ok(PathItem::new(Name::new(text)?));
    };
    let digits = rest
        .strip_suffix(')')
        .ok_or(RefError::Syntax("an array element closes with ')'"))?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(RefError::Syntax(
            "an array element is written with a decimal index",
        ));
    }
    let index: u32 = digits
        .parse()
        .map_err(|_| RefError::Syntax("the index of an array element is out of range"))?;
    Ok(PathItem::element(Name::new(name)?, index))
}

/// The length limit of a reference or a data reference.
fn check(text: &str) -> Result<(), RefError> {
    if text.len() > MAX_OBJECT_REFERENCE {
        return Err(RefError::TooLong {
            max: MAX_OBJECT_REFERENCE,
            length: text.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fc(code: &str) -> FunctionalConstraint {
        FunctionalConstraint::new(code).unwrap()
    }

    fn name(text: &str) -> Name {
        Name::new(text).unwrap()
    }

    #[test]
    fn the_examples_of_the_standard_map_to_their_mms_items() {
        // IEC 61850-8-1 §7.3.3 and §7.3.4 examples.
        let xcbr = ObjectReference::parse("LD1/XCBR1.Pos.stVal").unwrap();
        assert_eq!(
            xcbr.to_mms(fc("ST")).unwrap().to_text(),
            "LD1/XCBR1$ST$Pos$stVal"
        );
        let mmxu = ObjectReference::parse("LD1/MMXU1.A.phsA").unwrap();
        assert_eq!(
            mmxu.to_mms(fc("MX")).unwrap().to_text(),
            "LD1/MMXU1$MX$A$phsA"
        );
        let origin = ObjectReference::parse("LD1/XCBR1.Pos.origin.orCat").unwrap();
        assert_eq!(
            origin.to_mms(fc("ST")).unwrap().to_text(),
            "LD1/XCBR1$ST$Pos$origin$orCat"
        );
        // A logical node is its own MMS item in the domain (§7.3.2).
        let node = ObjectReference::parse("LD1/XCBR1").unwrap();
        assert_eq!(node.logical_node_item().unwrap().to_text(), "LD1/XCBR1");
    }

    #[test]
    fn a_control_block_has_the_address_of_its_functional_constraint() {
        // §8.1.3.2.2: the scope-qualified name of the control block, with its constraint.
        let block = ObjectReference::parse("LD1/LLN0.urcb01").unwrap();
        assert_eq!(
            block.to_mms(fc("RP")).unwrap().to_text(),
            "LD1/LLN0$RP$urcb01"
        );
        let buffered = ObjectReference::parse("LD1/LLN0.brcb01").unwrap();
        assert_eq!(
            buffered.to_mms(fc("BR")).unwrap().to_text(),
            "LD1/LLN0$BR$brcb01"
        );
    }

    #[test]
    fn an_array_element_is_not_a_name_but_a_data_reference_keeps_it() {
        let array = ObjectReference::parse("LD1/MMHA1.HA.phsAHar(7).cVal.mag.f").unwrap();
        assert_eq!(array.to_mms(fc("MX")), Err(RefError::ArrayElement));
        // §8.1.3.2.4 example: LDevice/MMHA1$MX$HA$phsAHar(7)$cVal$mag$f.
        assert_eq!(
            array.to_data_ref(fc("MX")).unwrap(),
            "LD1/MMHA1$MX$HA$phsAHar(7)$cVal$mag$f"
        );
        let (back, constraint) =
            ObjectReference::parse_data_ref("LD1/MMHA1$MX$HA$phsAHar(7)$cVal$mag$f").unwrap();
        assert_eq!(back, array);
        assert_eq!(constraint.code(), "MX");
    }

    #[test]
    fn the_syntax_of_the_reference_is_checked() {
        assert!(
            ObjectReference::parse("LD1").is_ok(),
            "a logical device alone"
        );
        assert!(ObjectReference::parse("LD1/LN1").is_ok());
        assert!(ObjectReference::parse("LD1.LN1").is_err());
        assert!(ObjectReference::parse("LD1/").is_err());
        assert!(
            ObjectReference::parse("LD1/LN1.DO(x)").is_err(),
            "index not decimal"
        );
        assert!(
            ObjectReference::parse("LD1/LN1.DO(7").is_err(),
            "unclosed index"
        );
        assert!(
            ObjectReference::parse("LD1/LN1.DO()").is_err(),
            "empty index"
        );
        assert!(
            ObjectReference::parse("LD1/LN1.1DO").is_err(),
            "names start with a letter"
        );
        assert!(
            ObjectReference::parse("LD1/LN1.DO$DA").is_err(),
            "$ is not in a reference"
        );
    }

    #[test]
    fn a_reference_is_limited_to_129_octets() {
        // LD of 64, then "/LN1." and a path of 60: 129 octets.
        let ld = "L".repeat(64);
        let at_limit = format!("{ld}/LN1.{}", "a".repeat(60));
        assert_eq!(at_limit.len(), 129);
        assert!(ObjectReference::parse(&at_limit).is_ok());
        let over = format!("{ld}/LN1.{}", "a".repeat(61));
        assert_eq!(
            ObjectReference::parse(&over),
            Err(RefError::TooLong {
                max: 129,
                length: 130
            })
        );
    }

    #[test]
    fn an_mms_item_is_read_back_into_its_reference() {
        let item = MmsName::parse("LD1/MMXU1$MX$A$phsA").unwrap();
        let (reference, constraint) = ObjectReference::from_mms(&item).unwrap();
        assert_eq!(reference.to_string(), "LD1/MMXU1.A.phsA");
        assert_eq!(constraint.code(), "MX");
        // Not domain-scoped, or without its constraint: refused.
        assert!(ObjectReference::from_mms(&MmsName::parse("/MMXU1$MX").unwrap()).is_err());
        assert!(ObjectReference::from_mms(&MmsName::parse("LD1/MMXU1").unwrap()).is_err());
        assert!(ObjectReference::from_mms(&MmsName::parse("LD1/MMXU1$XX$A").unwrap()).is_err());
        assert!(ObjectReference::from_mms(&MmsName::parse("LD1/MMXU1$ZZ$A").unwrap()).is_err());
    }

    #[test]
    fn a_flattened_item_is_limited_to_64_characters() {
        // An item of 64 characters is an MMS identifier; one more is refused by its length.
        let path = "b".repeat(64 - "LN1$ST$".len());
        let at_limit = ObjectReference::parse(&format!("LD1/LN1.{path}")).unwrap();
        assert_eq!(at_limit.to_mms(fc("ST")).unwrap().item().as_str().len(), 64);
        let over = ObjectReference::parse(&format!("LD1/LN1.{}", "b".repeat(64 - 6))).unwrap();
        assert_eq!(
            over.to_mms(fc("ST")),
            Err(RefError::TooLong {
                max: 64,
                length: 65
            })
        );
    }

    #[test]
    fn a_reference_with_a_name_too_long_is_refused_by_the_name() {
        let long = format!("LD1/{}", "A".repeat(65));
        // A name past 64 characters is a too-long error of the reference, through the conversion of NameError.
        assert_eq!(
            ObjectReference::parse(&long),
            Err(RefError::TooLong {
                max: 64,
                length: 65
            })
        );
        assert_eq!(name("LN1").as_str(), "LN1");
    }
}
