// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The SCL parser (IEC 61850-6; feature `scl`; task S1, SCL-004).
//!
//! The parser reads an SCL document into a plain tree of elements: names, namespaces, attributes and children. It does
//! not interpret the document (the semantic checks and the templates are the task S2). Its job is to refuse, before any
//! model is built, a document that is too large, too deep, or too big in elements, instances, attributes or values, and
//! a document that carries a document type declaration: SCL has none, and a declaration is where external entities and
//! entity expansion come from. Nothing is fetched: there is no network access and no entity is resolved.
//!
//! Every bound is checked while the tree is built, and the recursion of the builder is bounded by [`DEPTH_CEILING`].

pub mod build;
mod generated;
mod schema;
pub mod validate;

use std::fmt;

use roxmltree::ParsingOptions;

/// The deepest nesting the builder accepts, whatever the limits ask: the recursion of the builder never goes deeper.
pub const DEPTH_CEILING: usize = 256;

/// The elements that an SCL document instantiates: each one is an instance of the model (SCL-004: the instance count).
const INSTANCE_NAMES: &[&str] = &[
    "IED",
    "AccessPoint",
    "Server",
    "LDevice",
    "LN0",
    "LN",
    "DOI",
    "DAI",
    "SDI",
    "ConnectedAP",
    "SubNetwork",
];

/// The bounds of a parse. The defaults hold a large substation description with room to spare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// The largest document, in bytes.
    pub max_bytes: usize,
    /// The deepest nesting of elements. At most [`DEPTH_CEILING`], whatever this asks.
    pub max_depth: usize,
    /// The largest number of elements.
    pub max_elements: usize,
    /// The largest number of instances (the elements of [`INSTANCE_NAMES`]).
    pub max_instances: usize,
    /// The largest number of attributes of one element.
    pub max_attributes: usize,
    /// The longest value of an attribute, in bytes.
    pub max_value_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 67_108_864,
            max_depth: 64,
            max_elements: 1_000_000,
            max_instances: 250_000,
            max_attributes: 128,
            max_value_len: 4096,
        }
    }
}

/// An element of the document: its name, its namespace, its attributes, and its children in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// The local name of the element.
    pub name: String,
    /// The namespace of the element, if it has one.
    pub namespace: Option<String>,
    /// The line of the start tag, from 1 (diagnostics of the semantic checks give it).
    pub line: u32,
    /// The attributes, in document order, as (local name, value).
    pub attributes: Vec<(String, String)>,
    /// The child elements, in document order.
    pub children: Vec<Element>,
    /// The direct text of the element, trimmed (its character data outside the children; empty when there is none).
    pub text: String,
}

impl Element {
    /// The value of the attribute with this local name, if the element has it.
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(attribute, _)| attribute == name)
            .map(|(_, value)| value.as_str())
    }
}

/// A parsed document: its root element, and the counts that the bounds were checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The root element, which is `SCL`.
    pub root: Element,
    /// The number of elements.
    pub elements: usize,
    /// The number of instances (the elements of the instance list).
    pub instances: usize,
    /// The deepest nesting of elements, the root being at depth 1.
    pub depth: usize,
}

/// Why a document is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SclError {
    /// The document is larger than the limit.
    TooLarge {
        /// The size of the document, in bytes.
        size: usize,
        /// The limit.
        limit: usize,
    },
    /// The document is not UTF-8.
    NotUtf8,
    /// The document carries a document type declaration, which SCL does not use and which this parser refuses.
    DocTypeNotAllowed,
    /// The document is not well-formed XML.
    Xml(String),
    /// The document nests deeper than the limit.
    TooDeep {
        /// The limit that applies.
        limit: usize,
    },
    /// The document has more elements than the limit.
    TooManyElements {
        /// The limit.
        limit: usize,
    },
    /// The document has more instances than the limit.
    TooManyInstances {
        /// The limit.
        limit: usize,
    },
    /// An element has more attributes than the limit.
    TooManyAttributes {
        /// The element.
        element: String,
        /// The limit.
        limit: usize,
    },
    /// An attribute value is longer than the limit.
    ValueTooLong {
        /// The element.
        element: String,
        /// The attribute.
        attribute: String,
        /// The limit.
        limit: usize,
    },
    /// The root element is not `SCL`.
    NotScl {
        /// The name of the root element.
        root: String,
    },
}

impl fmt::Display for SclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { size, limit } => {
                write!(f, "the document has {size} bytes, at most {limit} allowed")
            }
            Self::NotUtf8 => write!(f, "the document is not UTF-8"),
            Self::DocTypeNotAllowed => write!(
                f,
                "a document type declaration is not allowed in an SCL document"
            ),
            Self::Xml(message) => write!(f, "the document is not well-formed XML: {message}"),
            Self::TooDeep { limit } => write!(f, "the document nests deeper than {limit} levels"),
            Self::TooManyElements { limit } => {
                write!(f, "the document has more than {limit} elements")
            }
            Self::TooManyInstances { limit } => {
                write!(f, "the document has more than {limit} instances")
            }
            Self::TooManyAttributes { element, limit } => {
                write!(f, "{element} has more than {limit} attributes")
            }
            Self::ValueTooLong {
                element,
                attribute,
                limit,
            } => write!(
                f,
                "the value of {element}.{attribute} is longer than {limit} bytes"
            ),
            Self::NotScl { root } => write!(f, "the root element is {root}, not SCL"),
        }
    }
}

impl std::error::Error for SclError {}

/// The counts of a build, checked against the limits as they grow.
#[derive(Debug, Default)]
struct Counts {
    elements: usize,
    instances: usize,
    depth: usize,
}

/// Parses an SCL document, within the limits.
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Document, SclError> {
    if bytes.len() > limits.max_bytes {
        return Err(SclError::TooLarge {
            size: bytes.len(),
            limit: limits.max_bytes,
        });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SclError::NotUtf8)?;
    if text.contains("<!DOCTYPE") {
        return Err(SclError::DocTypeNotAllowed);
    }
    let max_depth = limits.max_depth.min(DEPTH_CEILING);
    // The tree reader counts its nodes too (text and attributes): twice the element bound is a safe ceiling for it.
    let nodes_limit = u32::try_from(limits.max_elements.saturating_mul(2)).unwrap_or(u32::MAX);
    // The default entity resolver refuses to resolve any entity: nothing is fetched (SCL-004).
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit,
        ..ParsingOptions::default()
    };
    let xml =
        roxmltree::Document::parse_with_options(text, options).map_err(|error| match error {
            roxmltree::Error::DtdDetected => SclError::DocTypeNotAllowed,
            roxmltree::Error::NodesLimitReached => SclError::TooManyElements {
                limit: limits.max_elements,
            },
            other => SclError::Xml(other.to_string()),
        })?;
    let root = xml.root_element();
    if root.tag_name().name() != "SCL" {
        return Err(SclError::NotScl {
            root: root.tag_name().name().to_string(),
        });
    }
    let mut counts = Counts::default();
    let element = build(root, 1, max_depth, limits, &mut counts)?;
    Ok(Document {
        root: element,
        elements: counts.elements,
        instances: counts.instances,
        depth: counts.depth,
    })
}

/// Builds the element of a node, and of its children. `depth` is the depth of the node, the root being at 1; the
/// recursion goes no deeper than `max_depth`, which is at most [`DEPTH_CEILING`].
fn build(
    node: roxmltree::Node<'_, '_>,
    depth: usize,
    max_depth: usize,
    limits: &Limits,
    counts: &mut Counts,
) -> Result<Element, SclError> {
    if depth > max_depth {
        return Err(SclError::TooDeep { limit: max_depth });
    }
    counts.elements = counts.elements.saturating_add(1);
    if counts.elements > limits.max_elements {
        return Err(SclError::TooManyElements {
            limit: limits.max_elements,
        });
    }
    counts.depth = counts.depth.max(depth);
    let name = node.tag_name().name().to_string();
    if INSTANCE_NAMES.contains(&name.as_str()) {
        counts.instances = counts.instances.saturating_add(1);
        if counts.instances > limits.max_instances {
            return Err(SclError::TooManyInstances {
                limit: limits.max_instances,
            });
        }
    }

    let mut attributes = Vec::new();
    // Attributes of another namespace (xsi:schemaLocation, for example) are not SCL attributes: they are not kept.
    for attribute in node
        .attributes()
        .filter(|attribute| attribute.namespace().is_none())
    {
        if attributes.len() >= limits.max_attributes {
            return Err(SclError::TooManyAttributes {
                element: name,
                limit: limits.max_attributes,
            });
        }
        if attribute.value().len() > limits.max_value_len {
            return Err(SclError::ValueTooLong {
                element: name,
                attribute: attribute.name().to_string(),
                limit: limits.max_value_len,
            });
        }
        attributes.push((attribute.name().to_string(), attribute.value().to_string()));
    }

    let mut children = Vec::new();
    for child in node.children().filter(roxmltree::Node::is_element) {
        children.push(build(
            child,
            depth.saturating_add(1),
            max_depth,
            limits,
            counts,
        )?);
    }
    let line = node.document().text_pos_at(node.range().start).row;
    let raw: String = node
        .children()
        .filter(roxmltree::Node::is_text)
        .filter_map(|text| text.text())
        .collect();
    let text = raw.trim().to_string();
    if text.len() > limits.max_value_len {
        return Err(SclError::ValueTooLong {
            element: name,
            attribute: "#text".to_string(),
            limit: limits.max_value_len,
        });
    }
    Ok(Element {
        name,
        namespace: node.tag_name().namespace().map(str::to_string),
        line,
        attributes,
        children,
        text,
    })
}
