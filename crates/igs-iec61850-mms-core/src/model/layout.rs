// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The MMS layout of a logical node (IEC 61850-8-1 §7.3.1, §7.3.3, §7.3.4; tasks A3 and A4).
//!
//! A logical node is one MMS named variable, of a structure type with one component per functional constraint
//! it instantiates, in the order of 8-1 Figure 4; each functional constraint holds one component per data object
//! that has an attribute of that constraint; a data object holds its attributes, and its sub data objects, with
//! their own attributes. Each node of that tree is also a flattened named variable, whose name joins the names
//! of the path with `$` (`XCBR1$ST$Pos$stVal`). The flattened names are variables of the same tree, addressed
//! by their path, so that one value serves every name of a node.
//!
//! The flattened names of the functional-constraint level (`XCBR1$ST`) are not named variables: the examples of
//! §7.3.3 start at the data object. A flattened name longer than the identifier limit of the profile is not
//! listed; the number of such names is reported, and they remain reachable by alternate access (D-006).
//!
//! Each variable says whether an MMS Write may change it (D-012): the attributes of a constraint that the namespace
//! lets SetDataValues write, outside the controllable data objects, whose writes go through their state machine.
//! Each enumeration inside a variable says which values it defines (8-1 §8.1.3.4.4.2).

use igs_iso9506_mms_codec::mms::{
    Data, DataMMSString, FloatingPoint, Identifier, TimeOfDay, TypeDescription,
    TypeDescriptionStructure, TypeDescriptionStructureComponentsItem, TypeSpecification, UtcTime,
};
use igs_iso9506_mms_codec::schema::BitString as BitStringValue;

use super::{Element, LogicalNode, ModelError, Node};
use crate::catalogue;

/// The identifier limit of the IEC 61850 MMS profile: 64 characters (8-1 Annex F.3.2).
pub const IDENTIFIER_LIMIT: usize = 64;

/// The functional constraints in the order of 8-1 Figure 4 (the recommended order of the components).
pub const FUNCTIONAL_CONSTRAINT_ORDER: [&str; 22] = [
    "MX", "ST", "CO", "CF", "DC", "SP", "SG", "RP", "LG", "BR", "GO", "GS", "SV", "SE", "MS", "US",
    "EX", "SR", "OR", "BL", "RG", "RS",
];

/// Whether an MMS Write may change the attributes of a functional constraint: the namespace lists SetDataValues among
/// its services. The setting-group edit `SE` is changed by its own services, and the others are read-only.
pub fn writable_constraint(fc: &str) -> bool {
    catalogue::functional_constraint(fc).is_some_and(|f| f.services.contains(&"SetDataValues"))
}

/// The values that an enumeration inside a variable defines: the path of the enumeration from the variable, and the
/// values the namespace defines for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedValues {
    /// The indexes of the components from the variable to the enumeration. Empty when the variable is the enumeration.
    pub path: Vec<usize>,
    /// The values of the enumeration, in the namespace order.
    pub values: Vec<i64>,
}

/// A named variable of a logical node: its MMS name, the path of its components from the root, its type, and what a
/// write may do with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableLayout {
    /// The MMS name: the logical node itself, or the flattened name of the node.
    pub name: String,
    /// The indexes of the components from the root of the logical node. Empty for the root.
    pub path: Vec<usize>,
    /// The MMS type of the variable.
    pub ty: TypeDescription,
    /// The functional constraint of the variable. `None` for the logical node itself.
    pub functional_constraint: Option<&'static str>,
    /// Whether an MMS Write may change the variable (D-012). A variable with a read-only part is read-only.
    pub writable: bool,
    /// The enumerations inside the variable, with the values they define. A write holds only defined values.
    pub enumerations: Vec<DefinedValues>,
}

/// The MMS layout of a logical node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnLayout {
    /// The named variables: the logical node first, then its flattened names in the order of the tree.
    pub variables: Vec<VariableLayout>,
    /// The MMS type of the logical node.
    pub root: TypeDescription,
    /// The initial value of the logical node: a value of its type, with the zero of each leaf.
    pub root_value: Data,
    /// The number of flattened names left out because they are longer than the identifier limit.
    pub long_names: usize,
}

/// The initial value of a type: zero for the numbers, `false`, the empty string, and zero bits. The value
/// conforms to its type. `None` for a type that the model does not produce.
pub fn default_value(ty: &TypeDescription) -> Option<Data> {
    Some(match ty {
        TypeDescription::Structure(structure) => {
            let mut members = Vec::with_capacity(structure.components.len());
            for component in &structure.components {
                match &component.component_type {
                    TypeSpecification::TypeDescription(inner) => {
                        members.push(default_value(inner)?)
                    }
                    TypeSpecification::TypeName(_) => return None,
                }
            }
            Data::Structure(members)
        }
        TypeDescription::Boolean => Data::Boolean(false),
        TypeDescription::Integer(_) => Data::Integer(0),
        TypeDescription::Unsigned(_) => Data::Unsigned(0),
        TypeDescription::FloatingPoint(form) => {
            // The width octet of the exponent, then the octets of the format (the sign and the rest).
            let exponent = u8::try_from(form.exponent_width.0).ok()?;
            let octets = usize::try_from(form.format_width.0).ok()?.checked_div(8)?;
            let mut value = vec![0u8; octets.saturating_add(1)];
            *value.first_mut()? = exponent;
            Data::FloatingPoint(FloatingPoint(value))
        }
        TypeDescription::BitString(size) => {
            let bits = usize::try_from(size.0).ok()?;
            let bytes = bits.saturating_add(7).checked_div(8)?;
            let unused = bytes.saturating_mul(8).saturating_sub(bits);
            Data::BitString(BitStringValue {
                unused: u8::try_from(unused).ok()?,
                bytes: vec![0u8; bytes],
            })
        }
        // Zero octets of the size: a conforming value, and the reserved value of an EntryID (8-1 §8.1.3.5).
        TypeDescription::OctetString(size) => {
            Data::OctetString(vec![0u8; usize::try_from(size.0).ok()?])
        }
        TypeDescription::VisibleString(_) => Data::VisibleString(String::new()),
        TypeDescription::MMSString(_) => Data::MMSString(DataMMSString(String::new())),
        TypeDescription::UtcTime => Data::UtcTime(UtcTime(vec![0u8; 8])),
        // The zero time of day: the six-octet form (`true`) carries the day, the four-octet form does not.
        TypeDescription::BinaryTime(six_octets) => Data::BinaryTime(if *six_octets {
            TimeOfDay::from_milliseconds_and_day(0, 0).ok()?
        } else {
            TimeOfDay::from_milliseconds(0).ok()?
        }),
        _ => return None,
    })
}

/// The value at a path of component indexes. `None` when the path leaves the structure.
pub fn value_at<'a>(root: &'a Data, path: &[usize]) -> Option<&'a Data> {
    let mut current = root;
    for index in path {
        match current {
            Data::Structure(members) => current = members.get(*index)?,
            _ => return None,
        }
    }
    Some(current)
}

/// Replaces the value at a path of component indexes. `false` when the path leaves the structure, and the value is
/// then not written.
pub fn set_at(root: &mut Data, path: &[usize], value: Data) -> bool {
    match path.split_first() {
        None => {
            *root = value;
            true
        }
        Some((index, rest)) => match root {
            Data::Structure(members) => match members.get_mut(*index) {
                Some(member) => set_at(member, rest, value),
                None => false,
            },
            _ => false,
        },
    }
}

/// Whether every enumeration inside a value holds one of the values it defines (8-1 §8.1.3.4.4.2: a value beyond the
/// defined set is refused). A value that lacks an enumeration does not hold the defined values, so it is refused too.
pub fn defines_enumerations(value: &Data, enumerations: &[DefinedValues]) -> bool {
    enumerations
        .iter()
        .all(|defined| match value_at(value, &defined.path) {
            Some(Data::Integer(number)) => {
                i64::try_from(*number).is_ok_and(|number| defined.values.contains(&number))
            }
            _ => false,
        })
}

/// The type of a member: its leaf or enumeration type, or the structure of its members.
fn describe(element: &Element) -> TypeDescription {
    match &element.node {
        Node::Leaf(ty) | Node::Enumerated { ty, .. } => ty.clone(),
        Node::Branch(members) => {
            let parts: Vec<(String, TypeDescription)> = members
                .iter()
                .map(|member| (member.name.clone(), describe(member)))
                .collect();
            structure(parts)
        }
    }
}

fn structure(parts: Vec<(String, TypeDescription)>) -> TypeDescription {
    TypeDescription::Structure(TypeDescriptionStructure {
        packed: None,
        components: parts
            .into_iter()
            .map(|(name, ty)| TypeDescriptionStructureComponentsItem {
                component_name: Some(Identifier(name)),
                component_type: TypeSpecification::TypeDescription(Box::new(ty)),
            })
            .collect(),
    })
}

/// The enumerations inside a member, with their paths from the member.
fn enumerations_of(element: &Element) -> Vec<DefinedValues> {
    match &element.node {
        Node::Leaf(_) => Vec::new(),
        Node::Enumerated { values, .. } => vec![DefinedValues {
            path: Vec::new(),
            values: values.clone(),
        }],
        Node::Branch(members) => members
            .iter()
            .enumerate()
            .flat_map(|(index, member)| with_prefix(index, enumerations_of(member)))
            .collect(),
    }
}

/// The same enumerations, with the index of their component prepended to their paths.
fn with_prefix(index: usize, mut defined: Vec<DefinedValues>) -> Vec<DefinedValues> {
    for item in &mut defined {
        item.path.insert(0, index);
    }
    defined
}

/// A flattened name of a view: its name below the view, the path of its components inside the view, its type, and
/// what a write may do with it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Flat {
    name: String,
    path: Vec<usize>,
    ty: TypeDescription,
    fc: &'static str,
    writable: bool,
    enumerations: Vec<DefinedValues>,
}

/// The components of a view of the members under one functional constraint, with their flattened names and the
/// enumerations inside them.
#[derive(Debug, Default)]
struct View {
    components: Vec<(String, TypeDescription)>,
    flat: Vec<Flat>,
    enumerations: Vec<DefinedValues>,
}

/// Pushes a member as a whole, with its flattened names; its path is `path`.
fn flatten(
    name: &str,
    path: Vec<usize>,
    element: &Element,
    fc: &'static str,
    writable: bool,
    out: &mut Vec<Flat>,
) {
    out.push(Flat {
        name: name.to_string(),
        path: path.clone(),
        ty: describe(element),
        fc,
        writable,
        enumerations: enumerations_of(element),
    });
    if let Node::Branch(members) = &element.node {
        for (index, member) in members.iter().enumerate() {
            let mut child_path = path.clone();
            child_path.push(index);
            flatten(
                &format!("{name}${}", member.name),
                child_path,
                member,
                fc,
                writable,
                out,
            );
        }
    }
}

/// The view of `members` under `fc`. A member with the constraint is taken whole; a sub data object (a member
/// without constraint) is taken when one of its own members has it. `stateful` is set below a controllable data
/// object, whose attributes are then read-only.
fn view(members: &[Element], fc: &'static str, stateful: bool) -> View {
    let writable = !stateful && writable_constraint(fc);
    let mut out = View::default();
    for member in members {
        match member.fc {
            Some(own) if own == fc => {
                let index = out.components.len();
                out.components.push((member.name.clone(), describe(member)));
                flatten(
                    &member.name,
                    vec![index],
                    member,
                    fc,
                    writable,
                    &mut out.flat,
                );
                out.enumerations
                    .extend(with_prefix(index, enumerations_of(member)));
            }
            Some(_) => {}
            None => {
                let Node::Branch(children) = &member.node else {
                    continue;
                };
                let inner = view(children, fc, stateful || member.stateful);
                let View {
                    components,
                    flat,
                    enumerations,
                } = inner;
                if components.is_empty() {
                    continue;
                }
                let index = out.components.len();
                let ty = structure(components);
                out.components.push((member.name.clone(), ty.clone()));
                out.flat.push(Flat {
                    name: member.name.clone(),
                    path: vec![index],
                    ty,
                    fc,
                    writable: writable && !member.stateful,
                    enumerations: enumerations.clone(),
                });
                for item in flat {
                    let mut path = vec![index];
                    path.extend(item.path.iter().copied());
                    out.flat.push(Flat {
                        name: format!("{}${}", member.name, item.name),
                        path,
                        ..item
                    });
                }
                out.enumerations.extend(with_prefix(index, enumerations));
            }
        }
    }
    out
}

/// The MMS layout of a logical node, with the identifier limit `limit` for its flattened names.
pub fn layout(node: &LogicalNode, limit: usize) -> Result<LnLayout, ModelError> {
    let objects = node.objects();
    let mut components: Vec<(String, TypeDescription)> = Vec::new();
    let mut flat: Vec<Flat> = Vec::new();
    for fc in FUNCTIONAL_CONSTRAINT_ORDER {
        let View {
            components: parts,
            flat: items,
            ..
        } = view(objects, fc, false);
        if parts.is_empty() {
            continue;
        }
        let index = components.len();
        components.push((fc.to_string(), structure(parts)));
        for item in items {
            let mut path = vec![index];
            path.extend(item.path.iter().copied());
            flat.push(Flat {
                name: format!("{fc}${}", item.name),
                path,
                ..item
            });
        }
    }
    let root = structure(components);
    let root_value = default_value(&root).ok_or_else(|| ModelError::Unsupported {
        path: node.name().to_string(),
        reason: "the initial value of a member is not defined",
    })?;

    let mut variables = vec![VariableLayout {
        name: node.name().to_string(),
        path: Vec::new(),
        ty: root.clone(),
        functional_constraint: None,
        writable: false,
        enumerations: Vec::new(),
    }];
    let mut long_names = 0usize;
    for item in flat {
        // A variable below the functional constraint: its path has the constraint, then the data object.
        if item.path.len() < 2 {
            continue;
        }
        let name = format!("{}${}", node.name(), item.name);
        if name.chars().count() > limit {
            long_names = long_names.saturating_add(1);
            continue;
        }
        variables.push(VariableLayout {
            name,
            path: item.path,
            ty: item.ty,
            functional_constraint: Some(item.fc),
            writable: item.writable,
            enumerations: item.enumerations,
        });
    }
    Ok(LnLayout {
        variables,
        root,
        root_value,
        long_names,
    })
}
