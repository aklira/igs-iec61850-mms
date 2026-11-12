// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The server model: logical devices, logical nodes, data objects and data attributes, checked against the
//! catalogue of the namespace (MODEL-001, MODEL-004; task A3).
//!
//! A logical node is built from its class and from the data objects the application instantiates. The build
//! checks, against the namespace:
//!
//! - the class exists and is concrete;
//! - every data object is defined by the class (or one of its bases), and none is given twice;
//! - every mandatory data object of the class is present;
//! - every attribute named by the application is defined by the common data class of its object, and none is
//!   given twice;
//! - every mandatory attribute (and mandatory sub data object) of the common data class is present, and is
//!   always instantiated.
//!
//! Optional and conditional members are instantiated only when the application names them. Their presence
//! conditions other than `M` are not evaluated here (PROVENANCE D-007). Each type is resolved to its MMS form
//! ([`types`]); a type that is not resolved is refused with [`ModelError::UnmappedType`] or
//! [`ModelError::Unsupported`].
//!
//! The MMS layout of a logical node (one named variable with its flattened names) is in [`layout`].

pub mod layout;
pub mod types;

use std::collections::BTreeSet;
use std::fmt;

use igs_iso9506_mms_codec::mms::TypeDescription;

use crate::catalogue::{self, LnClass, Presence, TypeKind};
use crate::names::{Name, NameError};

/// The deepest nesting of constructed attributes and sub data objects that is walked. The namespaces nest
/// at most three levels; a deeper definition is refused rather than followed.
pub const MAX_DEPTH: usize = 8;

/// Why a model is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelError {
    /// A name of the model is not a valid IEC 61850 name (A1).
    Name(NameError),
    /// A name is given twice where it must be unique.
    Duplicate {
        /// What is duplicated: `logical device`, `logical node`, `data object` or `attribute`.
        what: &'static str,
        /// The name.
        name: String,
    },
    /// The logical-node class is not in the catalogue of the namespace.
    UnknownLnClass(String),
    /// The logical-node class is abstract: a logical node cannot be an instance of it.
    AbstractLnClass(&'static str),
    /// The data object is not defined by the class of its logical node.
    UnknownDataObject {
        /// The class.
        class: &'static str,
        /// The data object.
        name: String,
    },
    /// A mandatory data object of the class is not present.
    MissingDataObject {
        /// The class.
        class: &'static str,
        /// The data object.
        name: String,
    },
    /// The common data class of a data object does not define the attribute.
    UnknownAttribute {
        /// The data object, as `logical node.data object`.
        path: String,
        /// The attribute.
        name: String,
    },
    /// The common data class is not in the catalogue, or it has several variants that cannot be chosen yet.
    UnknownCdc {
        /// The data object, as `logical node.data object`.
        path: String,
        /// The common data class.
        cdc: String,
    },
    /// A type name that the mapping does not resolve.
    UnmappedType {
        /// The attribute, as `logical node.data object.attribute`.
        path: String,
        /// The type name, as the namespace gives it.
        type_name: String,
    },
    /// A member whose form is not mapped yet (arrays, values chosen by the instance, service attributes).
    Unsupported {
        /// The member.
        path: String,
        /// The reason.
        reason: &'static str,
    },
    /// The definitions nest deeper than [`MAX_DEPTH`].
    TooDeep {
        /// The member.
        path: String,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(error) => write!(f, "{error}"),
            Self::Duplicate { what, name } => write!(f, "the {what} {name} is given twice"),
            Self::UnknownLnClass(class) => {
                write!(f, "the logical-node class {class} is not in the catalogue")
            }
            Self::AbstractLnClass(class) => write!(f, "the logical-node class {class} is abstract"),
            Self::UnknownDataObject { class, name } => {
                write!(f, "the class {class} does not define {name}")
            }
            Self::MissingDataObject { class, name } => {
                write!(
                    f,
                    "the mandatory data object {name} of the class {class} is missing"
                )
            }
            Self::UnknownAttribute { path, name } => {
                write!(f, "{path} does not define the attribute {name}")
            }
            Self::UnknownCdc { path, cdc } => {
                write!(f, "{path} has the common data class {cdc}, not usable here")
            }
            Self::UnmappedType { path, type_name } => {
                write!(f, "{path} has the type {type_name}, not mapped yet")
            }
            Self::Unsupported { path, reason } => write!(f, "{path}: {reason}"),
            Self::TooDeep { path } => write!(f, "{path} nests deeper than {MAX_DEPTH} levels"),
        }
    }
}

impl std::error::Error for ModelError {}

impl From<NameError> for ModelError {
    fn from(error: NameError) -> Self {
        Self::Name(error)
    }
}

/// Whether a presence condition makes the member mandatory: `M`, and the multiple form of it.
pub fn is_required(presence: &Presence) -> bool {
    matches!(presence.condition, "M" | "Mmulti")
}

/// Whether a presence condition allows several instances of the data object (`Omulti`, `Mmulti`, and the ranges):
/// the instances are named by the name of the definition and a number (`Ind1`, `Ind2`).
pub fn is_multiple(presence: &Presence) -> bool {
    presence.condition.contains("multi")
}

/// Whether the name of a data object is the name of this definition, or one of its instances.
pub fn instance_of(name: &str, definition: &catalogue::DataObject) -> bool {
    if name == definition.name {
        return true;
    }
    if !is_multiple(&definition.presence) {
        return false;
    }
    name.strip_prefix(definition.name)
        .is_some_and(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
}

/// A data object as the application declares it: its name, and the attributes it instantiates on top of the
/// mandatory ones.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DataObjectSpec {
    name: String,
    attributes: Vec<String>,
}

impl DataObjectSpec {
    /// A data object of this name, with the mandatory members only.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            attributes: Vec::new(),
        }
    }

    /// The name of the data object.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Adds the optional or conditional attributes (or sub data objects) that the object instantiates.
    #[must_use]
    pub fn with_attributes<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.attributes.extend(names.into_iter().map(Into::into));
        self
    }
}

/// A member of a data object: a data attribute, or a sub data object (which has no functional constraint of its
/// own; its attributes have one each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub(crate) name: String,
    pub(crate) fc: Option<&'static str>,
    pub(crate) node: Node,
}

/// The form of a member: a leaf of a basic, enumerated or fixed type, or a branch of constructed attributes or
/// sub data objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A leaf, with its MMS type.
    Leaf(TypeDescription),
    /// A branch, with its members.
    Branch(Vec<Element>),
}

impl Element {
    /// The name of the member.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The functional constraint of a data attribute. `None` for a sub data object.
    pub fn functional_constraint(&self) -> Option<&'static str> {
        self.fc
    }

    /// The form of the member.
    pub fn node(&self) -> &Node {
        &self.node
    }
}

/// A logical node: a name, its class, and its data objects in the order the application gave them (the order of
/// the SCL data objects, 8-1 §7.3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalNode {
    name: Name,
    class: &'static LnClass,
    objects: Vec<Element>,
}

impl LogicalNode {
    /// Builds a logical node of `class` named `name`, with the data objects `specs`, after the checks of the
    /// module.
    pub fn new(name: &str, class: &str, specs: &[DataObjectSpec]) -> Result<Self, ModelError> {
        let name = Name::new(name)?;
        let definition = catalogue::ln_class(class)
            .ok_or_else(|| ModelError::UnknownLnClass(class.to_string()))?;
        if definition.is_abstract {
            return Err(ModelError::AbstractLnClass(definition.name));
        }
        let definitions = definition.all_data_objects();
        let mut given = BTreeSet::new();
        for spec in specs {
            if !given.insert(spec.name.as_str()) {
                return Err(ModelError::Duplicate {
                    what: "data object",
                    name: spec.name.clone(),
                });
            }
            if !definitions.iter().any(|d| instance_of(&spec.name, d)) {
                return Err(ModelError::UnknownDataObject {
                    class: definition.name,
                    name: spec.name.clone(),
                });
            }
        }
        for candidate in &definitions {
            let present = specs.iter().any(|spec| instance_of(&spec.name, candidate));
            if is_required(&candidate.presence) && !present {
                return Err(ModelError::MissingDataObject {
                    class: definition.name,
                    name: candidate.name.to_string(),
                });
            }
        }
        let path = name.to_string();
        let mut objects = Vec::with_capacity(specs.len());
        for spec in specs {
            let Some(candidate) = definitions.iter().find(|d| instance_of(&spec.name, d)) else {
                continue;
            };
            let object_path = format!("{path}.{}", spec.name);
            let cdc = single_cdc(candidate.cdc, &object_path)?;
            let listed = check_listed(cdc, &spec.attributes, &object_path)?;
            let members = members_of(cdc, &listed, &object_path, 0, candidate.underlying_type)?;
            objects.push(Element {
                name: spec.name.clone(),
                fc: None,
                node: Node::Branch(members),
            });
        }
        Ok(Self {
            name,
            class: definition,
            objects,
        })
    }

    /// The name of the logical node.
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The class of the logical node, from the catalogue.
    pub fn class(&self) -> &'static LnClass {
        self.class
    }

    /// The data objects, in the order given by the application.
    pub fn objects(&self) -> &[Element] {
        &self.objects
    }
}

/// A logical device: a name, and its logical nodes, with unique names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalDevice {
    name: Name,
    nodes: Vec<LogicalNode>,
}

impl LogicalDevice {
    /// A logical device of this name, with these logical nodes.
    pub fn new(name: &str, nodes: Vec<LogicalNode>) -> Result<Self, ModelError> {
        let name = Name::new(name)?;
        let mut seen = BTreeSet::new();
        for node in &nodes {
            if !seen.insert(node.name.clone()) {
                return Err(ModelError::Duplicate {
                    what: "logical node",
                    name: node.name.to_string(),
                });
            }
        }
        Ok(Self { name, nodes })
    }

    /// The name of the logical device: the name of its MMS domain.
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The logical nodes, in the order given.
    pub fn nodes(&self) -> &[LogicalNode] {
        &self.nodes
    }
}

/// A server: its logical devices, with unique names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerModel {
    devices: Vec<LogicalDevice>,
}

impl ServerModel {
    /// An empty server.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a logical device. Its name must be new in the server.
    pub fn add_device(&mut self, device: LogicalDevice) -> Result<(), ModelError> {
        if self.devices.iter().any(|d| d.name == device.name) {
            return Err(ModelError::Duplicate {
                what: "logical device",
                name: device.name.to_string(),
            });
        }
        self.devices.push(device);
        Ok(())
    }

    /// The logical devices, in the order they were added.
    pub fn devices(&self) -> &[LogicalDevice] {
        &self.devices
    }
}

fn single_cdc(name: &str, path: &str) -> Result<&'static catalogue::Cdc, ModelError> {
    let matches: Vec<&'static catalogue::Cdc> = catalogue::generated::ns_7_3::CDCS
        .iter()
        .filter(|c| c.name == name)
        .collect();
    // A common data class with variants (the setting groups) needs the choice of the variant, which is not made yet.
    if matches.len() != 1 {
        return Err(ModelError::UnknownCdc {
            path: path.to_string(),
            cdc: name.to_string(),
        });
    }
    matches
        .first()
        .copied()
        .ok_or_else(|| ModelError::UnknownCdc {
            path: path.to_string(),
            cdc: name.to_string(),
        })
}

fn check_listed(
    cdc: &catalogue::Cdc,
    listed: &[String],
    path: &str,
) -> Result<BTreeSet<String>, ModelError> {
    let mut given = BTreeSet::new();
    for name in listed {
        if !given.insert(name.clone()) {
            return Err(ModelError::Duplicate {
                what: "attribute",
                name: name.clone(),
            });
        }
        let defined = cdc.data_attributes.iter().any(|a| a.name == name)
            || cdc.sub_data_objects.iter().any(|s| s.name == name);
        if !defined {
            return Err(ModelError::UnknownAttribute {
                path: path.to_string(),
                name: name.clone(),
            });
        }
    }
    Ok(given)
}

/// The members of a common data class that are instantiated: the mandatory ones, and the listed ones.
///
/// `underlying` is the type name that the data object of the class gives to the attributes whose type the class
/// leaves open (the enumeration of an enumerated status, IEC 61850-7-3 parameterised classes).
fn members_of(
    cdc: &catalogue::Cdc,
    listed: &BTreeSet<String>,
    path: &str,
    depth: usize,
    underlying: Option<&'static str>,
) -> Result<Vec<Element>, ModelError> {
    if depth >= MAX_DEPTH {
        return Err(ModelError::TooDeep {
            path: path.to_string(),
        });
    }
    let mut members = Vec::new();
    for attribute in cdc.data_attributes {
        if is_required(&attribute.presence) || listed.contains(attribute.name) {
            members.push(attribute_element(attribute, path, underlying)?);
        }
    }
    for sub in cdc.sub_data_objects {
        if is_required(&sub.presence) || listed.contains(sub.name) {
            let sub_path = format!("{path}.{}", sub.name);
            let sub_cdc = single_cdc(sub.cdc, &sub_path)?;
            let sub_members = members_of(
                sub_cdc,
                &BTreeSet::new(),
                &sub_path,
                depth.saturating_add(1),
                None,
            )?;
            members.push(Element {
                name: sub.name.to_string(),
                fc: None,
                node: Node::Branch(sub_members),
            });
        }
    }
    Ok(members)
}

fn attribute_element(
    attribute: &catalogue::DataAttribute,
    path: &str,
    underlying: Option<&'static str>,
) -> Result<Element, ModelError> {
    let here = format!("{path}.{}", attribute.name);
    if attribute.is_array {
        return Err(ModelError::Unsupported {
            path: here,
            reason: "array attributes are not mapped yet",
        });
    }
    // An attribute the class leaves without a type name takes the name of its data object, with its own kind.
    let (type_name, kind) = match (attribute.type_name, underlying) {
        (None, Some(name)) => (Some(name), attribute.type_kind),
        _ => (attribute.type_name, attribute.type_kind),
    };
    let node = resolve(&here, type_name, kind, 0)?;
    Ok(Element {
        name: attribute.name.to_string(),
        fc: Some(attribute.fc),
        node,
    })
}

/// The form of a type. A constructed attribute has all its components, in the order of its definition.
fn resolve(
    path: &str,
    type_name: Option<&str>,
    kind: TypeKind,
    depth: usize,
) -> Result<Node, ModelError> {
    if depth >= MAX_DEPTH {
        return Err(ModelError::TooDeep {
            path: path.to_string(),
        });
    }
    let unmapped = || ModelError::UnmappedType {
        path: path.to_string(),
        type_name: type_name.unwrap_or("(none)").to_string(),
    };
    match kind {
        TypeKind::Basic => {
            let name = type_name.ok_or_else(unmapped)?;
            types::basic(name).map(Node::Leaf).ok_or_else(unmapped)
        }
        TypeKind::Enumerated => {
            // An enumeration without a name is parameterised by the instance (the enumeration of an
            // enumerated controllable object), which is not chosen yet.
            let name = type_name.ok_or_else(|| ModelError::Unsupported {
                path: path.to_string(),
                reason: "enumeration chosen by the instance is not mapped yet",
            })?;
            types::enumeration(name)
                .map(Node::Leaf)
                .ok_or_else(unmapped)
        }
        TypeKind::Constructed => {
            let name = type_name.ok_or_else(unmapped)?;
            let definition = catalogue::constructed_attribute(name).ok_or_else(unmapped)?;
            let mut members = Vec::with_capacity(definition.components.len());
            for component in definition.components {
                let component_path = format!("{path}.{}", component.name);
                let node = resolve(
                    &component_path,
                    component.type_name,
                    component.type_kind,
                    depth.saturating_add(1),
                )?;
                members.push(Element {
                    name: component.name.to_string(),
                    fc: None,
                    node,
                });
            }
            Ok(Node::Branch(members))
        }
        TypeKind::Scsm | TypeKind::Undefined => Err(ModelError::Unsupported {
            path: path.to_string(),
            reason: "type given by the specific mapping or by the instance is not mapped yet",
        }),
    }
}
