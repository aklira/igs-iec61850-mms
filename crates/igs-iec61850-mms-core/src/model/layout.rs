// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The MMS layout of a logical node (IEC 61850-8-1 §7.3.1, §7.3.3, §7.3.4; task A3).
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

use igs_iso9506_mms_codec::mms::{
    Data, DataMMSString, FloatingPoint, Identifier, TypeDescription, TypeDescriptionStructure,
    TypeDescriptionStructureComponentsItem, TypeSpecification, UtcTime,
};
use igs_iso9506_mms_codec::schema::BitString as BitStringValue;

use super::{Element, LogicalNode, ModelError, Node};

/// The identifier limit of the IEC 61850 MMS profile: 64 characters (8-1 Annex F.3.2).
pub const IDENTIFIER_LIMIT: usize = 64;

/// The functional constraints in the order of 8-1 Figure 4 (the recommended order of the components).
pub const FUNCTIONAL_CONSTRAINT_ORDER: [&str; 22] = [
    "MX", "ST", "CO", "CF", "DC", "SP", "SG", "RP", "LG", "BR", "GO", "GS", "SV", "SE", "MS", "US",
    "EX", "SR", "OR", "BL", "RG", "RS",
];

/// A named variable of a logical node: its MMS name, the path of its components from the root, and its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableLayout {
    /// The MMS name: the logical node itself, or the flattened name of the node.
    pub name: String,
    /// The indexes of the components from the root of the logical node. Empty for the root.
    pub path: Vec<usize>,
    /// The MMS type of the variable.
    pub ty: TypeDescription,
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
        TypeDescription::OctetString(_) => Data::OctetString(Vec::new()),
        TypeDescription::VisibleString(_) => Data::VisibleString(String::new()),
        TypeDescription::MMSString(_) => Data::MMSString(DataMMSString(String::new())),
        TypeDescription::UtcTime => Data::UtcTime(UtcTime(vec![0u8; 8])),
        _ => return None,
    })
}

/// The type of a member: its leaf type, or the structure of its members.
fn describe(element: &Element) -> TypeDescription {
    match &element.node {
        Node::Leaf(ty) => ty.clone(),
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

/// A flattened name of a view: its name below the view, the path of its components inside the view, and its type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Flat {
    name: String,
    path: Vec<usize>,
    ty: TypeDescription,
}

/// The components of a view of the members under one functional constraint, and their flattened names.
#[derive(Debug, Default)]
struct View {
    components: Vec<(String, TypeDescription)>,
    flat: Vec<Flat>,
}

/// Pushes a member as a whole, with its flattened names; its path is `path`.
fn flatten(name: &str, path: Vec<usize>, element: &Element, out: &mut Vec<Flat>) {
    out.push(Flat {
        name: name.to_string(),
        path: path.clone(),
        ty: describe(element),
    });
    if let Node::Branch(members) = &element.node {
        for (index, member) in members.iter().enumerate() {
            let mut child_path = path.clone();
            child_path.push(index);
            flatten(&format!("{name}${}", member.name), child_path, member, out);
        }
    }
}

/// The view of `members` under `fc`. A member with the constraint is taken whole; a sub data object (a member
/// without constraint) is taken when one of its own members has it.
fn view(members: &[Element], fc: &str) -> View {
    let mut out = View::default();
    for member in members {
        match member.fc {
            Some(own) if own == fc => {
                let index = out.components.len();
                out.components.push((member.name.clone(), describe(member)));
                flatten(&member.name, vec![index], member, &mut out.flat);
            }
            Some(_) => {}
            None => {
                if let Node::Branch(children) = &member.node {
                    let inner = view(children, fc);
                    if inner.components.is_empty() {
                        continue;
                    }
                    let index = out.components.len();
                    let ty = structure(inner.components);
                    out.components.push((member.name.clone(), ty.clone()));
                    out.flat.push(Flat {
                        name: member.name.clone(),
                        path: vec![index],
                        ty: ty.clone(),
                    });
                    for flat in inner.flat {
                        let mut path = vec![index];
                        path.extend(flat.path);
                        out.flat.push(Flat {
                            name: format!("{}${}", member.name, flat.name),
                            path,
                            ty: flat.ty,
                        });
                    }
                }
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
        let inner = view(objects, fc);
        if inner.components.is_empty() {
            continue;
        }
        let index = components.len();
        components.push((fc.to_string(), structure(inner.components)));
        for item in inner.flat {
            let mut path = vec![index];
            path.extend(item.path.iter().copied());
            flat.push(Flat {
                name: format!("{fc}${}", item.name),
                path,
                ty: item.ty,
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
    }];
    let mut long_names = 0usize;
    for item in flat {
        // The first component is the functional constraint: its own name is not a variable (D-006).
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
        });
    }
    Ok(LnLayout {
        variables,
        root,
        root_value,
        long_names,
    })
}
