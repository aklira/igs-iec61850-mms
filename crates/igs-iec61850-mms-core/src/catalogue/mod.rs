// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Catalogue of the standard data model: basic types, functional constraints, presence conditions,
//! enumerations, constructed attributes, common data classes (CDC), logical-node classes, and the
//! 8-1 service additions.
//!
//! The data lives in [`generated`], written by `cargo xtask catalogue` from the latest IEC code
//! components (NSD files of IEC 61850-7-2, 7-3, 7-4 and the SNSD of 8-1); see PROVENANCE.md D-002.
//! The order of attributes and data objects is the order of the NSD files, which is also the order
//! of the components of the MMS structures. This module only defines the types and simple lookups;
//! interpretation (presence evaluation, inheritance, MMS mapping) belongs to the model tasks.

pub mod generated;

/// Identity of a namespace (IEC 61850-7-1, namespace version, revision and release).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Namespace {
    /// Namespace identifier, for example `IEC 61850-7-4`.
    pub id: &'static str,
    /// Version (year), for example 2007.
    pub version: u16,
    /// Revision letter, for example `B`.
    pub revision: char,
    /// Release number of the code component.
    pub release: u8,
}

/// Kind of the type of an attribute (`typeKind` of the NSD files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    /// A basic type of 7-2.
    Basic,
    /// An enumeration.
    Enumerated,
    /// A constructed attribute.
    Constructed,
    /// A type defined by the specific communication service mapping (8-1).
    Scsm,
    /// Left open by the namespace (parameterised by the using CDC).
    Undefined,
}

/// Presence condition and its arguments (IEC 61850-7-3, presence conditions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presence {
    /// Condition name, for example `M`, `O` or `MOcond`.
    pub condition: &'static str,
    /// Arguments of the condition, as written in the namespace, if any.
    pub arguments: Option<&'static str>,
}

/// A functional constraint and the services and control blocks it applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionalConstraint {
    /// Two-letter abbreviation, for example `ST`.
    pub abbreviation: &'static str,
    /// ACSI services applicable to attributes of this constraint.
    pub services: &'static [&'static str],
    /// Control blocks whose data sets may hold members of this constraint.
    pub data_set_member_of: &'static [&'static str],
}

/// A presence condition defined by a namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresenceCondition {
    /// Condition name.
    pub name: &'static str,
    /// Kind of argument the condition takes, if any.
    pub argument: Option<&'static str>,
}

/// One literal of an enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Literal {
    /// Literal name.
    pub name: &'static str,
    /// Integer value on the wire (IEC 61850-8-1 §8.1.2.2).
    pub value: i32,
    /// Deprecated by the namespace.
    pub deprecated: bool,
}

/// An enumeration type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enumeration {
    /// Type name.
    pub name: &'static str,
    /// Literals, in namespace order.
    pub literals: &'static [Literal],
}

/// A component of a constructed attribute or of an 8-1 service type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubDataAttribute {
    /// Component name.
    pub name: &'static str,
    /// Type name, absent when the using CDC parameterises it.
    pub type_name: Option<&'static str>,
    /// Kind of the type (`Basic` when the namespace gives none, the NSD default).
    pub type_kind: TypeKind,
    /// Presence condition (`M` when the namespace gives none, the NSD default).
    pub presence: Presence,
    /// Minimum value, as written in the namespace.
    pub min_value: Option<&'static str>,
    /// Maximum value, as written in the namespace.
    pub max_value: Option<&'static str>,
    /// Default value, as written in the namespace.
    pub default_value: Option<&'static str>,
}

/// A constructed attribute type, or an 8-1 service constructed attribute or type realisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstructedAttribute {
    /// Type name.
    pub name: &'static str,
    /// The type kind of some components is chosen by the using attribute.
    pub type_kind_parameterized: bool,
    /// Components, in namespace order.
    pub components: &'static [SubDataAttribute],
}

/// A data attribute of a CDC, or an 8-1 service data attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataAttribute {
    /// Attribute name.
    pub name: &'static str,
    /// Functional constraint abbreviation.
    pub fc: &'static str,
    /// Type name, absent when the CDC is parameterised.
    pub type_name: Option<&'static str>,
    /// Kind of the type.
    pub type_kind: TypeKind,
    /// Presence condition.
    pub presence: Presence,
    /// The attribute is an array.
    pub is_array: bool,
    /// Attribute giving the array size, if any.
    pub size_attribute: Option<&'static str>,
    /// Trigger option data-change.
    pub dchg: bool,
    /// Trigger option quality-change.
    pub qchg: bool,
    /// Trigger option data-update.
    pub dupd: bool,
    /// Default value, as written in the namespace.
    pub default_value: Option<&'static str>,
    /// Underlying type (8-1 service attributes only).
    pub underlying_type: Option<&'static str>,
    /// Kind of the underlying type (8-1 service attributes only).
    pub underlying_type_kind: Option<TypeKind>,
}

/// A sub-data object of a CDC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubDataObject {
    /// Name.
    pub name: &'static str,
    /// CDC of the sub-data object.
    pub cdc: &'static str,
    /// Presence condition.
    pub presence: Presence,
    /// The sub-data object is an array.
    pub is_array: bool,
    /// Attribute giving the array size, if any.
    pub size_attribute: Option<&'static str>,
}

/// A service parameter of a controllable CDC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceParameter {
    /// Name.
    pub name: &'static str,
    /// Type name, absent when parameterised.
    pub type_name: Option<&'static str>,
    /// Kind of the type.
    pub type_kind: TypeKind,
}

/// A common data class (IEC 61850-7-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cdc {
    /// Class name.
    pub name: &'static str,
    /// Variant (`SP`, `SG` or `SE` for setting classes), if any.
    pub variant: Option<&'static str>,
    /// The class carries statistical information.
    pub statistics: bool,
    /// Deprecated by the namespace.
    pub deprecated: bool,
    /// The enumeration type is chosen by the using data object.
    pub enum_parameterized: bool,
    /// The type kind of some attributes is chosen by the using data object.
    pub type_kind_parameterized: bool,
    /// Data attributes, in namespace order.
    pub data_attributes: &'static [DataAttribute],
    /// Sub-data objects, in namespace order.
    pub sub_data_objects: &'static [SubDataObject],
    /// Service parameters, in namespace order.
    pub service_parameters: &'static [ServiceParameter],
}

/// A data object of a logical-node class (IEC 61850-7-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataObject {
    /// Name.
    pub name: &'static str,
    /// CDC of the data object.
    pub cdc: &'static str,
    /// Presence condition.
    pub presence: Presence,
    /// Presence condition of derived statistics (`M` when the namespace gives none, the NSD default).
    pub ds_presence: Presence,
    /// Transient data object.
    pub transient: bool,
    /// Deprecated by the namespace.
    pub deprecated: bool,
    /// Underlying type, for a CDC parameterised by the data object.
    pub underlying_type: Option<&'static str>,
    /// Kind of the underlying type.
    pub underlying_type_kind: Option<TypeKind>,
}

/// A logical-node class, abstract or concrete (IEC 61850-7-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LnClass {
    /// Class name.
    pub name: &'static str,
    /// Base class, if any.
    pub base: Option<&'static str>,
    /// Abstract class (not instantiable).
    pub is_abstract: bool,
    /// The class can hold a log.
    pub can_have_log: bool,
    /// Data objects declared by this class (not inherited), in namespace order.
    pub data_objects: &'static [DataObject],
}

/// Deepest inheritance chain accepted between logical-node classes (the 7-4 classes have at most a few
/// levels); a longer chain, or a cycle, ends the walk.
pub const MAX_LN_DEPTH: usize = 16;

impl LnClass {
    /// Data objects of the class and of its base classes, the most general base first. A base that is
    /// missing from the catalogue, or a chain deeper than [`MAX_LN_DEPTH`], ends the walk.
    pub fn all_data_objects(&self) -> Vec<&'static DataObject> {
        let mut chain: Vec<&'static [DataObject]> = vec![self.data_objects];
        let mut base = self.base;
        while let Some(name) = base {
            if chain.len() >= MAX_LN_DEPTH {
                break;
            }
            match ln_class(name) {
                Some(class) => {
                    chain.push(class.data_objects);
                    base = class.base;
                }
                None => break,
            }
        }
        chain.into_iter().rev().flatten().collect()
    }
}

/// 8-1 additions to a CDC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceCdc {
    /// CDC name.
    pub cdc: &'static str,
    /// Attributes added by the mapping, in namespace order.
    pub attributes: &'static [DataAttribute],
}

/// A namespace that a service namespace applies to (AppNS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliesTo {
    /// Namespace identifier.
    pub id: &'static str,
    /// Version.
    pub version: u16,
    /// Revision letter.
    pub revision: char,
    /// Release number.
    pub release: u8,
}

/// The CDC of this name, or of this name and variant.
pub fn cdc(name: &str, variant: Option<&str>) -> Option<&'static Cdc> {
    generated::ns_7_3::CDCS
        .iter()
        .find(|c| c.name == name && (variant.is_none() || c.variant == variant))
}

/// The logical-node class of this name, abstract or concrete.
pub fn ln_class(name: &str) -> Option<&'static LnClass> {
    generated::ns_7_4::LN_CLASSES
        .iter()
        .find(|c| c.name == name)
}

/// The enumeration of this name, in 7-2, 7-3 or 7-4.
pub fn enumeration(name: &str) -> Option<&'static Enumeration> {
    [
        generated::ns_7_2::ENUMERATIONS,
        generated::ns_7_3::ENUMERATIONS,
        generated::ns_7_4::ENUMERATIONS,
    ]
    .into_iter()
    .flatten()
    .find(|e| e.name == name)
}

/// The constructed attribute of this name, in 7-2 or 7-3.
pub fn constructed_attribute(name: &str) -> Option<&'static ConstructedAttribute> {
    [
        generated::ns_7_2::CONSTRUCTED_ATTRIBUTES,
        generated::ns_7_3::CONSTRUCTED_ATTRIBUTES,
    ]
    .into_iter()
    .flatten()
    .find(|c| c.name == name)
}

/// The functional constraint of this abbreviation, in 7-2 or 8-1.
pub fn functional_constraint(abbreviation: &str) -> Option<&'static FunctionalConstraint> {
    [
        generated::ns_7_2::FUNCTIONAL_CONSTRAINTS,
        generated::ns_8_1::FUNCTIONAL_CONSTRAINTS,
    ]
    .into_iter()
    .flatten()
    .find(|f| f.abbreviation == abbreviation)
}
