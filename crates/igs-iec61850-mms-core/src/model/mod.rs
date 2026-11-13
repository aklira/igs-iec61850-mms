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

use std::collections::{BTreeMap, BTreeSet};
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
    /// The integer values that the instance gives to its own data attributes, by name: the sizes of the arrays (`maxPts`).
    integers: BTreeMap<String, i64>,
}

impl DataObjectSpec {
    /// A data object of this name, with the mandatory members only.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            attributes: Vec::new(),
            integers: BTreeMap::new(),
        }
    }

    /// The name of the data object.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The integer values that the instance gives to the attributes of the object: the sizes of its arrays.
    pub fn with_integers(mut self, values: BTreeMap<String, i64>) -> Self {
        self.integers = values;
        self
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
    /// The member belongs to a controllable data object, whose writes go through its control state machine and
    /// not directly (MODEL-005). Set on the data objects; the attributes inherit it in the layout.
    pub(crate) stateful: bool,
}

/// The form of a member: a leaf of a basic or fixed type, an enumeration with the values it defines, or a branch of
/// constructed attributes or sub data objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A leaf, with its MMS type.
    Leaf(TypeDescription),
    /// An enumeration: its MMS type, and the values the namespace defines (8-1 §8.1.2.2; the writes of §8.1.3.4.4.2).
    Enumerated {
        /// The MMS type.
        ty: TypeDescription,
        /// The defined values, in the namespace order.
        values: Vec<i64>,
    },
    /// A branch, with its members.
    Branch(Vec<Element>),
    /// An array of `count` elements of the type of its element (IEC 61850-7-2 `ARRAY 0..m OF p`; 8-1 §8.1.2.7). The count is given by
    /// the instance (the size attribute of the common data class, IEC 61850-7-2 §6.2.5.5).
    Array {
        /// The number of elements.
        count: u32,
        /// The element.
        element: Box<Node>,
    },
}

/// Whether a common data class is controllable: its class has the service attributes of 8-1 (`Oper`, `SBO`, `Cancel`).
pub fn is_controllable(cdc: &str) -> bool {
    catalogue::generated::ns_8_1::SERVICE_CDCS
        .iter()
        .any(|service| service.cdc == cdc)
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

    /// Whether the member belongs to a controllable data object (see [`is_controllable`]).
    pub fn is_controllable(&self) -> bool {
        self.stateful
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
        Self::with_setting_groups(name, class, specs, false)
    }

    /// Builds a logical node as [`LogicalNode::new`], in a device that has setting groups (`groups` true: a `SettingControl` of
    /// the device, IEC 61850-6) or not. The setting data objects take the variant of the device (IEC 61850-7-3 §7.6.1).
    pub fn with_setting_groups(
        name: &str,
        class: &str,
        specs: &[DataObjectSpec],
        groups: bool,
    ) -> Result<Self, ModelError> {
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
            let shape = data_object_shape(candidate.cdc, groups, &object_path)?;
            let listed = check_listed(&shape, &spec.attributes, &object_path)?;
            let members = members_of(
                &shape,
                &listed,
                &object_path,
                0,
                candidate.underlying_type,
                groups,
                &spec.integers,
            )?;
            objects.push(Element {
                name: spec.name.clone(),
                fc: None,
                node: Node::Branch(members),
                stateful: is_controllable(candidate.cdc),
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

/// The attributes of a data object, as its device instantiates them. A common data class with setting-group variants (IEC 61850-7-3
/// §7.6.1) is the variant SP alone in a device without setting groups; in a device with setting groups, the active values (SG) and
/// the values edited (SE), which are the attributes of the variant SE with their own functional constraint.
struct Shape {
    base: &'static catalogue::Cdc,
    attributes: Vec<&'static catalogue::DataAttribute>,
}

fn data_object_shape(name: &str, groups: bool, path: &str) -> Result<Shape, ModelError> {
    let unknown = || ModelError::UnknownCdc {
        path: path.to_string(),
        cdc: name.to_string(),
    };
    let classes: Vec<&'static catalogue::Cdc> = catalogue::generated::ns_7_3::CDCS
        .iter()
        .filter(|c| c.name == name)
        .collect();
    if let [only] = classes.as_slice() {
        return Ok(Shape {
            base: only,
            attributes: only.data_attributes.iter().collect(),
        });
    }
    let variant = |wanted: &str| {
        classes
            .iter()
            .copied()
            .find(|c| c.variant == Some(wanted))
            .ok_or_else(unknown)
    };
    if groups {
        let sg = variant("SG")?;
        let se = variant("SE")?;
        let mut attributes: Vec<&'static catalogue::DataAttribute> =
            sg.data_attributes.iter().collect();
        // The attributes of SE that are edited values; the others (description, extension) are the same as in SG.
        attributes.extend(se.data_attributes.iter().filter(|a| a.fc == "SE"));
        Ok(Shape {
            base: sg,
            attributes,
        })
    } else {
        let sp = variant("SP")?;
        Ok(Shape {
            base: sp,
            attributes: sp.data_attributes.iter().collect(),
        })
    }
}

fn check_listed(
    shape: &Shape,
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
        let defined = shape.attributes.iter().any(|a| a.name == name)
            || shape.base.sub_data_objects.iter().any(|s| s.name == name);
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
    shape: &Shape,
    listed: &BTreeSet<String>,
    path: &str,
    depth: usize,
    underlying: Option<&'static str>,
    groups: bool,
    integers: &BTreeMap<String, i64>,
) -> Result<Vec<Element>, ModelError> {
    if depth >= MAX_DEPTH {
        return Err(ModelError::TooDeep {
            path: path.to_string(),
        });
    }
    let mut members = Vec::new();
    for attribute in &shape.attributes {
        if is_required(&attribute.presence) || listed.contains(attribute.name) {
            members.push(attribute_element(attribute, path, underlying, integers)?);
        }
    }
    for sub in shape.base.sub_data_objects {
        if is_required(&sub.presence) || listed.contains(sub.name) {
            let sub_path = format!("{path}.{}", sub.name);
            let sub_shape = data_object_shape(sub.cdc, groups, &sub_path)?;
            let sub_members = members_of(
                &sub_shape,
                &BTreeSet::new(),
                &sub_path,
                depth.saturating_add(1),
                None,
                groups,
                &BTreeMap::new(),
            )?;
            members.push(Element {
                name: sub.name.to_string(),
                fc: None,
                node: Node::Branch(sub_members),
                stateful: is_controllable(sub.cdc),
            });
        }
    }
    Ok(members)
}

/// The most elements an array may have in a configuration (the sizes of the curves and the histories are a few hundred).
const MAX_ARRAY_ELEMENTS: u32 = 4096;

fn attribute_element(
    attribute: &catalogue::DataAttribute,
    path: &str,
    underlying: Option<&'static str>,
    integers: &BTreeMap<String, i64>,
) -> Result<Element, ModelError> {
    let here = format!("{path}.{}", attribute.name);
    // An attribute the class leaves without a type name takes the name of its data object, with its own kind.
    let (type_name, kind) = match (attribute.type_name, underlying) {
        (None, Some(name)) => (Some(name), attribute.type_kind),
        _ => (attribute.type_name, attribute.type_kind),
    };
    let node = if attribute.is_array {
        // The number of elements is the value that the instance gives to the size attribute of the class (7-2 §6.2.5.5).
        let count = attribute
            .size_attribute
            .and_then(|size| integers.get(size))
            .copied()
            .ok_or_else(|| ModelError::Unsupported {
                path: here.clone(),
                reason: "the number of elements of the array is not given by the instance",
            })?;
        let count = u32::try_from(count)
            .ok()
            .filter(|count| *count <= MAX_ARRAY_ELEMENTS)
            .ok_or_else(|| ModelError::Unsupported {
                path: here.clone(),
                reason: "the number of elements of the array is out of range",
            })?;
        Node::Array {
            count,
            element: Box::new(resolve(&here, type_name, kind, 0)?),
        }
    } else {
        resolve(&here, type_name, kind, 0)?
    };
    Ok(Element {
        name: attribute.name.to_string(),
        fc: Some(attribute.fc),
        node,
        stateful: false,
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
            let ty = types::enumeration(name).ok_or_else(unmapped)?;
            let values = types::enumeration_values(name).ok_or_else(unmapped)?;
            Ok(Node::Enumerated { ty, values })
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
                    stateful: false,
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
