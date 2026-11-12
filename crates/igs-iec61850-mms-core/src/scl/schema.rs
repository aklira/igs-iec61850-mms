// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The frozen SCL schema as the validator reads it: the types, their children and attributes (task S2).
//!
//! The table is generated from the 2007B schema of IEC 61850-6:2009 (see `generated`); this module only gives
//! its shape and the lookups. The schema is not read at run time.

use super::generated::schema_2007b::{ROOTS, TYPES};

/// The type name of an element whose type is a simple type: its text is not checked by the structure.
pub(crate) const SIMPLE: &str = "simple";

/// A complex type: its children, whether it admits foreign-namespace children, and its attributes.
#[derive(Debug)]
pub(crate) struct TypeSpec {
    /// The name of the type, as the schema gives it (`anon:<root>` or `<Parent>/<Element>` for local types).
    pub(crate) name: &'static str,
    /// The children the schema names, with their types and occurrences.
    pub(crate) children: &'static [ChildSpec],
    /// Whether the type admits children of other namespaces (the `##other` wildcard).
    pub(crate) any_child: bool,
    /// The attributes the schema names.
    pub(crate) attributes: &'static [AttrSpec],
}

/// A child element of a type.
#[derive(Debug)]
pub(crate) struct ChildSpec {
    /// The local name of the child.
    pub(crate) element: &'static str,
    /// The type of the child, or [`SIMPLE`].
    pub(crate) ty: &'static str,
    /// The least number of occurrences.
    pub(crate) min: u32,
    /// The greatest number of occurrences; `None` is unbounded.
    pub(crate) max: Option<u32>,
}

/// An attribute of a type.
#[derive(Debug)]
pub(crate) struct AttrSpec {
    /// The local name of the attribute.
    pub(crate) name: &'static str,
    /// Whether the attribute is required.
    pub(crate) required: bool,
    /// The values the attribute admits.
    pub(crate) kind: AttrKind,
}

/// The values an attribute admits.
#[derive(Debug)]
pub(crate) enum AttrKind {
    /// Any text.
    Text,
    /// `true`, `false`, `0` or `1`.
    Boolean,
    /// An integer within the bounds, inclusive.
    Integer {
        /// The least value.
        min: i64,
        /// The greatest value.
        max: i64,
    },
    /// One of the listed values.
    Enumeration(&'static [&'static str]),
}

/// The type a root element has, if the schema names it.
pub(crate) fn root_type(element: &str) -> Option<&'static str> {
    ROOTS
        .iter()
        .find(|(name, _)| *name == element)
        .map(|(_, ty)| *ty)
}

/// The complex type with this name.
pub(crate) fn type_named(name: &str) -> Option<&'static TypeSpec> {
    TYPES
        .binary_search_by(|spec| spec.name.cmp(name))
        .ok()
        .and_then(|index| TYPES.get(index))
}
