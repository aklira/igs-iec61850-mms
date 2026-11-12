// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Discovery of a server's objects from its MMS answers (IEC 61850-8-1 §7.3, §13.4, ACSI-001, MODEL-002; task A6).
//!
//! A client reads the names of the logical devices, of the logical nodes and of the named variables with GetNameList,
//! and the definitions with GetVariableAccessAttributes. This module turns those answers into the objects of the model,
//! without I/O: the definition of an object is rebuilt from its MMS type description, and each definition is kept in one
//! of three states:
//!
//! - **known**: a leaf of a type this client knows, or a structure or an array of known leaves;
//! - **unknown semantics**: a type the client does not know, or a type named by the server and not sent with it. The
//!   description is kept as it is (`Leaf::Unknown`, `Leaf::Unresolved`), so the client can read and write the value
//!   generically (MODEL-002);
//! - **malformed**: the answer breaks a rule of the protocol or of the mapping (an unnamed component, a zero size, a
//!   duplicate name, a nesting beyond the walk). It is an error, not a value.

use std::collections::BTreeSet;
use std::fmt;

use igs_iso9506_mms_codec::generated::types::{Identifier, TypeDescription, TypeSpecification};

use crate::model::MAX_DEPTH;

/// The size of a string or of a bit string. A size that is not negative is a fixed length. A negative size is the
/// maximum of a variable length (ISO 9506-2 §14.2.2.4.2): this is how the reference server describes its quality
/// (`BitString(-13)`) and its strings (D-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// The length, or the maximum length of a variable one.
    pub length: u32,
    /// Whether the length varies, up to `length`.
    pub variable: bool,
}

/// A leaf of a definition: a type of the MMS mapping, or a type the client keeps without knowing its meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leaf {
    /// A boolean.
    Boolean,
    /// A signed integer of this many bits (1 to 64 for the models of IEC 61850).
    Integer {
        /// The size in bits.
        bits: u8,
    },
    /// An unsigned integer of this many bits.
    Unsigned {
        /// The size in bits.
        bits: u8,
    },
    /// A floating-point value: the size of the format and of its exponent, in bits.
    FloatingPoint {
        /// The size of the format, in bits.
        format_width: u8,
        /// The size of the exponent, in bits.
        exponent_width: u8,
    },
    /// A bit string of this size, in bits (at least one bit).
    BitString {
        /// The size.
        size: Size,
    },
    /// An octet string of this size, in octets.
    OctetString {
        /// The size.
        size: Size,
    },
    /// A visible string of this size, in characters.
    VisibleString {
        /// The size.
        size: Size,
    },
    /// A UTF-8 string of this size, in characters.
    MmsString {
        /// The size.
        size: Size,
    },
    /// A UTC time: eight octets (Annex F.3.1).
    UtcTime,
    /// A generalized time.
    GeneralizedTime,
    /// A binary time: the time of day, or the time sequence.
    BinaryTime {
        /// The form of the time: `true` for the time of day.
        time_of_day: bool,
    },
    /// A binary-coded decimal of this many digits.
    Bcd {
        /// The number of digits.
        digits: u8,
    },
    /// An object identifier.
    ObjectId,
    /// A type that the server names and does not send with its description. Its position in the object is kept.
    Unresolved {
        /// The path of the component that names it.
        path: String,
    },
    /// A type of MMS that this client does not know.
    Unknown,
}

/// The definition of an object: a leaf, a structure with its named members, or an array of one element type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Definition {
    /// A leaf.
    Leaf(Leaf),
    /// A structure: its members, in the order of the description.
    Structure(Vec<Member>),
    /// An array of `elements` elements of one definition.
    Array {
        /// The number of elements.
        elements: u32,
        /// The definition of an element.
        element: Box<Definition>,
    },
}

/// A named member of a structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The name of the member.
    pub name: String,
    /// Its definition.
    pub definition: Definition,
}

impl Definition {
    /// The member of this structure named `name`, if it is one.
    pub fn member(&self, name: &str) -> Option<&Definition> {
        match self {
            Self::Structure(members) => members
                .iter()
                .find(|member| member.name == name)
                .map(|member| &member.definition),
            _ => None,
        }
    }
}

/// Why a definition cannot be rebuilt from its description.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DefinitionError {
    /// The description breaks a rule of the protocol or of the mapping; the path is where.
    Malformed {
        /// The path of the member, from the object.
        path: String,
        /// The rule that is broken.
        reason: &'static str,
    },
    /// The description nests deeper than the walk allows.
    TooDeep {
        /// The path of the member.
        path: String,
    },
}

impl fmt::Display for DefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { path, reason } => write!(f, "{path} is malformed: {reason}"),
            Self::TooDeep { path } => write!(f, "{path} nests deeper than {MAX_DEPTH} levels"),
        }
    }
}

impl std::error::Error for DefinitionError {}

fn malformed(path: &str, reason: &'static str) -> DefinitionError {
    DefinitionError::Malformed {
        path: path.to_string(),
        reason,
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}.{name}")
    }
}

/// The definition of an object from its type description. `path` names the object in the errors.
pub fn define(ty: &TypeDescription, path: &str) -> Result<Definition, DefinitionError> {
    define_at(ty, path, 0)
}

fn define_at(
    ty: &TypeDescription,
    path: &str,
    depth: usize,
) -> Result<Definition, DefinitionError> {
    if depth >= MAX_DEPTH {
        return Err(DefinitionError::TooDeep {
            path: path.to_string(),
        });
    }
    let depth = depth.saturating_add(1);
    Ok(match ty {
        TypeDescription::Boolean => Definition::Leaf(Leaf::Boolean),
        TypeDescription::Integer(bits) => Definition::Leaf(Leaf::Integer {
            bits: bits_of(bits.0, path, 1)?,
        }),
        TypeDescription::Unsigned(bits) => Definition::Leaf(Leaf::Unsigned {
            bits: bits_of(bits.0, path, 1)?,
        }),
        TypeDescription::FloatingPoint(form) => {
            let format_width = bits_of(form.format_width.0, path, 1)?;
            let exponent_width = bits_of(form.exponent_width.0, path, 1)?;
            if exponent_width >= format_width {
                return Err(malformed(
                    path,
                    "the exponent is not shorter than the format",
                ));
            }
            Definition::Leaf(Leaf::FloatingPoint {
                format_width,
                exponent_width,
            })
        }
        TypeDescription::BitString(size) => Definition::Leaf(Leaf::BitString {
            size: size_of(size.0, path, true)?,
        }),
        TypeDescription::OctetString(size) => Definition::Leaf(Leaf::OctetString {
            size: size_of(size.0, path, false)?,
        }),
        TypeDescription::VisibleString(size) => Definition::Leaf(Leaf::VisibleString {
            size: size_of(size.0, path, false)?,
        }),
        TypeDescription::MMSString(size) => Definition::Leaf(Leaf::MmsString {
            size: size_of(size.0, path, false)?,
        }),
        TypeDescription::UtcTime => Definition::Leaf(Leaf::UtcTime),
        TypeDescription::GeneralizedTime => Definition::Leaf(Leaf::GeneralizedTime),
        TypeDescription::BinaryTime(time_of_day) => Definition::Leaf(Leaf::BinaryTime {
            time_of_day: *time_of_day,
        }),
        TypeDescription::Bcd(digits) => Definition::Leaf(Leaf::Bcd {
            digits: bits_of(digits.0, path, 1)?,
        }),
        TypeDescription::ObjId => Definition::Leaf(Leaf::ObjectId),
        TypeDescription::Structure(structure) => {
            let mut names = BTreeSet::new();
            let mut members = Vec::with_capacity(structure.components.len());
            for component in &structure.components {
                let Some(Identifier(name)) = &component.component_name else {
                    return Err(malformed(path, "a component has no name"));
                };
                if !names.insert(name.clone()) {
                    return Err(malformed(path, "two components have the same name"));
                }
                let member_path = join(path, name);
                let definition = define_spec(&component.component_type, &member_path, depth)?;
                members.push(Member {
                    name: name.clone(),
                    definition,
                });
            }
            Definition::Structure(members)
        }
        TypeDescription::Array(array) => {
            let elements = u32::try_from(array.number_of_elements.0)
                .map_err(|_| malformed(path, "an array of a negative number of elements"))?;
            let element = define_spec(&array.element_type, &join(path, "[]"), depth)?;
            Definition::Array {
                elements,
                element: Box::new(element),
            }
        }
        TypeDescription::Unsupported { .. } => Definition::Leaf(Leaf::Unknown),
    })
}

fn define_spec(
    spec: &TypeSpecification,
    path: &str,
    depth: usize,
) -> Result<Definition, DefinitionError> {
    match spec {
        TypeSpecification::TypeDescription(inner) => define_at(inner, path, depth),
        TypeSpecification::TypeName(_) => Ok(Definition::Leaf(Leaf::Unresolved {
            path: path.to_string(),
        })),
    }
}

/// A size in bits, at least `min`: a size of zero is malformed.
fn bits_of(value: i64, path: &str, min: u8) -> Result<u8, DefinitionError> {
    u8::try_from(value)
        .ok()
        .filter(|bits| *bits >= min)
        .ok_or_else(|| malformed(path, "a size outside its range"))
}

/// A size of a string: a negative value is a variable length of at most its absolute value. A zero size is
/// malformed for a bit string (§8.1.2.1 of IEC 61850-8-1: no zero-length bit string); a string may have a zero maximum.
fn size_of(value: i64, path: &str, bit_string: bool) -> Result<Size, DefinitionError> {
    let length = u32::try_from(value.unsigned_abs())
        .map_err(|_| malformed(path, "a size outside its range"))?;
    if length == 0 && bit_string {
        return Err(malformed(path, "a bit string of no bit"));
    }
    Ok(Size {
        length,
        variable: value < 0,
    })
}

/// One functional constraint of a logical node, with the data objects that have attributes of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The functional constraint, as the server names it (`ST`, `MX`, ...).
    pub functional_constraint: String,
    /// The data objects of the constraint, in the order of the description.
    pub objects: Vec<String>,
}

/// The functional constraints of a logical node, and their data objects, from the description of the node.
///
/// The root of the node is a structure with one component per constraint; each constraint is a structure with one
/// component per data object (8-1 §7.3.1). A constraint the server names but does not describe as a structure is
/// malformed.
pub fn functional_groups(root: &TypeDescription) -> Result<Vec<Group>, DefinitionError> {
    let TypeDescription::Structure(structure) = root else {
        return Err(malformed("", "a logical node is not a structure"));
    };
    let mut groups = Vec::with_capacity(structure.components.len());
    for component in &structure.components {
        let Some(Identifier(name)) = &component.component_name else {
            return Err(malformed("", "a functional constraint has no name"));
        };
        let TypeSpecification::TypeDescription(inner) = &component.component_type else {
            return Err(malformed(
                name,
                "a functional constraint is named, not described",
            ));
        };
        let TypeDescription::Structure(objects) = inner.as_ref() else {
            return Err(malformed(
                name,
                "a functional constraint is not a structure",
            ));
        };
        let mut names = Vec::with_capacity(objects.components.len());
        for object in &objects.components {
            let Some(Identifier(object_name)) = &object.component_name else {
                return Err(malformed(name, "a data object has no name"));
            };
            names.push(object_name.clone());
        }
        groups.push(Group {
            functional_constraint: name.clone(),
            objects: names,
        });
    }
    Ok(groups)
}

/// The logical nodes among the named variables of a logical device: the variables whose name is the name of a node,
/// with no `$` (the flattened names of a node have one or more). The order is kept.
pub fn logical_nodes(named_variables: &[String]) -> Vec<String> {
    named_variables
        .iter()
        .filter(|name| !name.contains('$'))
        .cloned()
        .collect()
}

/// The most names a listing keeps. A server that goes on past it is refused: the bound is the RUN-001 limit of the
/// model, far above the size of the demonstration models.
pub const MAX_LISTED_NAMES: usize = 100_000;

/// Why a listing is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PagingError {
    /// A name has already been listed: the server repeats a name over its pages.
    Repeated {
        /// The name that repeats.
        name: String,
    },
    /// The server says that more names follow, and gives none: the listing would never end.
    Stalled,
    /// The listing is longer than [`MAX_LISTED_NAMES`].
    TooLong,
}

impl fmt::Display for PagingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repeated { name } => write!(f, "the listing repeats {name}"),
            Self::Stalled => write!(f, "the listing says that more names follow, and gives none"),
            Self::TooLong => write!(f, "the listing is longer than {MAX_LISTED_NAMES} names"),
        }
    }
}

impl std::error::Error for PagingError {}

/// A listing of names read page by page. The server gives its names in its own order, which MMS does not fix, so a
/// listing is checked by what it must not do: repeat a name, stall, or go on without end. The next page is asked after
/// the last name of this one.
#[derive(Debug, Default)]
pub struct Listing {
    names: Vec<String>,
    seen: BTreeSet<String>,
}

impl Listing {
    /// An empty listing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The last name listed, which the next page continues after.
    pub fn last(&self) -> Option<&str> {
        self.names.last().map(String::as_str)
    }

    /// Adds a page. `more_follows` is what the server answered with it. A name that is already listed is refused, and
    /// so is a page that is empty while more names follow.
    pub fn add_page(&mut self, page: &[String], more_follows: bool) -> Result<(), PagingError> {
        if page.is_empty() && more_follows {
            return Err(PagingError::Stalled);
        }
        for name in page {
            if !self.seen.insert(name.clone()) {
                return Err(PagingError::Repeated { name: name.clone() });
            }
            if self.names.len() >= MAX_LISTED_NAMES {
                return Err(PagingError::TooLong);
            }
            self.names.push(name.clone());
        }
        Ok(())
    }

    /// The names listed, in the order of the server.
    pub fn into_names(self) -> Vec<String> {
        self.names
    }
}
