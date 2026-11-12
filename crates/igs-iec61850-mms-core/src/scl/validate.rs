// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The checks of a parsed SCL document against the frozen 2007B schema, and of its own references (task S2).
//!
//! - SCL-002: the structure. Elements the type does not admit, occurrences, required attributes, and the values
//!   of the attributes (enumerations, booleans, integer ranges). The content of `Private` is not checked.
//! - SCL-003: the references. Data-type templates (`lnType`, the `type` of data objects, data attributes and
//!   SDO/BDA), the members of data sets (`FCDA`), the `datSet` of control blocks, the IEDs named by `ConnectedAP`
//!   and `ExtRef`.
//! - SCL-004: the namespaces. A root outside the 2007B namespace, and an element of another namespace where the
//!   schema has no `##other` wildcard.
//!
//! A document with any finding is refused as a whole: [`validate`] returns every finding, sorted by line, and the
//! caller builds nothing. The order of the elements inside a sequence is not checked (see PROVENANCE, D-021).

use std::collections::{HashMap, HashSet};
use std::fmt;

use super::generated::schema_2007b::NAMESPACE;
use super::schema::{self, AttrKind, TypeSpec, SIMPLE};
use super::{Document, Element};

/// The type of the `Private` element: its content is a wildcard, and is not checked.
const PRIVATE_TYPE: &str = "tPrivate";

/// The control blocks whose `datSet` names a data set of the same logical node.
const CONTROL_BLOCKS: &[&str] = &[
    "ReportControl",
    "LogControl",
    "GSEControl",
    "SampledValueControl",
];

/// A finding of the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The rule: `SCL-002`, `SCL-003` or `SCL-004`.
    pub code: &'static str,
    /// The file the document was read from, as the caller names it.
    pub file: String,
    /// The path of the element, for example `SCL/IED[IED1]/AccessPoint[AP1]`.
    pub path: String,
    /// The line of the element's start tag, from 1.
    pub line: u32,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} {}: {}",
            self.file, self.line, self.code, self.path, self.message
        )
    }
}

/// Checks a parsed document. `file` names it in the diagnostics. The result is `Ok` when no rule is broken.
pub fn validate(document: &Document, file: &str) -> Result<(), Vec<Diagnostic>> {
    let mut checker = Checker {
        file,
        diagnostics: Vec::new(),
    };
    let root = &document.root;
    if root.namespace.as_deref() != Some(NAMESPACE) {
        checker.report(
            "SCL-004",
            root,
            "SCL",
            "the root is not in the namespace of the 2007B schema".to_string(),
        );
    } else if let Some(root_ty) = schema::root_type(&root.name).and_then(schema::type_named) {
        checker.structure(root, root_ty, "SCL");
        checker.references(root);
    } else {
        checker.report(
            "SCL-002",
            root,
            "SCL",
            "the root is not a root element of the schema".to_string(),
        );
    }
    checker.finish()
}

struct Checker<'a> {
    file: &'a str,
    diagnostics: Vec<Diagnostic>,
}

impl Checker<'_> {
    fn report(&mut self, code: &'static str, element: &Element, path: &str, message: String) {
        self.diagnostics.push(Diagnostic {
            code,
            file: self.file.to_string(),
            path: path.to_string(),
            line: element.line,
            message,
        });
    }

    fn finish(mut self) -> Result<(), Vec<Diagnostic>> {
        if self.diagnostics.is_empty() {
            return Ok(());
        }
        self.diagnostics
            .sort_by(|a, b| (a.line, a.code, &a.path).cmp(&(b.line, b.code, &b.path)));
        Err(self.diagnostics)
    }

    /// SCL-002 and SCL-004: the element against its type, then its children against the type's children.
    fn structure(&mut self, element: &Element, ty: &TypeSpec, path: &str) {
        for (name, value) in &element.attributes {
            match ty.attributes.iter().find(|spec| spec.name == name.as_str()) {
                Some(spec) => {
                    if let Some(problem) = check_value(&spec.kind, value) {
                        self.report(
                            "SCL-002",
                            element,
                            path,
                            format!("attribute {name} {problem}"),
                        );
                    }
                }
                // The schema's attribute wildcard is ##other: it admits namespaced attributes, which the parser
                // does not keep. Every attribute kept is unqualified, so an unnamed one is refused.
                None => {
                    self.report(
                        "SCL-002",
                        element,
                        path,
                        format!("unknown attribute {name}"),
                    );
                }
            }
        }
        for spec in ty.attributes.iter().filter(|spec| spec.required) {
            if element.attribute(spec.name).is_none() {
                self.report(
                    "SCL-002",
                    element,
                    path,
                    format!("missing required attribute {}", spec.name),
                );
            }
        }

        let mut counts: HashMap<&str, u32> = HashMap::new();
        for child in &element.children {
            let child_path = segment(path, child);
            if child.namespace.as_deref() != Some(NAMESPACE) {
                if !ty.any_child {
                    self.report(
                        "SCL-004",
                        child,
                        &child_path,
                        format!(
                            "element {} is in namespace {:?}; the schema admits no other namespace here",
                            child.name, child.namespace
                        ),
                    );
                }
                continue;
            }
            let Some(spec) = ty.children.iter().find(|spec| spec.element == child.name) else {
                self.report(
                    "SCL-002",
                    child,
                    &child_path,
                    format!("element {} is not admitted in {}", child.name, element.name),
                );
                continue;
            };
            let count = counts.entry(spec.element).or_insert(0);
            *count = (*count).saturating_add(1);
            if spec.ty == SIMPLE || spec.ty == PRIVATE_TYPE {
                continue;
            }
            if let Some(child_ty) = schema::type_named(spec.ty) {
                self.structure(child, child_ty, &child_path);
            }
        }
        for spec in ty.children {
            let count = counts.get(spec.element).copied().unwrap_or(0);
            if count < spec.min {
                self.report(
                    "SCL-002",
                    element,
                    path,
                    format!(
                        "{} occurs {count} time(s); at least {} required",
                        spec.element, spec.min
                    ),
                );
            }
            if let Some(max) = spec.max.filter(|max| count > *max) {
                self.report(
                    "SCL-002",
                    element,
                    path,
                    format!(
                        "{} occurs {count} time(s); at most {max} admitted",
                        spec.element
                    ),
                );
            }
        }
    }

    /// SCL-003: the references of the document.
    fn references(&mut self, root: &Element) {
        let templates = Templates::build(root);
        self.check_templates(root, &templates);

        let mut ieds: HashSet<&str> = HashSet::new();
        for ied in root.children.iter().filter(|child| child.name == "IED") {
            if let Some(name) = ied.attribute("name") {
                ieds.insert(name);
            }
        }
        self.check_ieds(root, &templates);

        let mut links = Vec::new();
        collect(root, "SCL", "ConnectedAP", &mut links);
        collect(root, "SCL", "ExtRef", &mut links);
        for (path, element) in links {
            if let Some(target) = element.attribute("iedName") {
                if !ieds.contains(target) {
                    self.report(
                        "SCL-003",
                        element,
                        &path,
                        format!("iedName {target} is not an IED of this file"),
                    );
                }
            }
        }
    }

    fn check_templates(&mut self, root: &Element, templates: &Templates<'_>) {
        let Some(types) = root
            .children
            .iter()
            .find(|child| child.name == "DataTypeTemplates")
        else {
            return;
        };
        let types_path = segment("SCL", types);
        for node in &types.children {
            let node_path = segment(&types_path, node);
            for member in &node.children {
                let member_path = segment(&node_path, member);
                match (node.name.as_str(), member.name.as_str()) {
                    ("LNodeType", "DO") | ("DOType", "SDO") => {
                        self.require_type(member, &member_path, &templates.do_types);
                    }
                    ("DOType", "DA") | ("DAType", "BDA") => {
                        self.check_data_attribute(member, &member_path, templates);
                    }
                    _ => {}
                }
            }
        }
    }

    /// A `type` that is present names a template of the table.
    fn require_type(&mut self, element: &Element, path: &str, table: &HashMap<&str, &Element>) {
        if let Some(id) = element.attribute("type") {
            if !table.contains_key(id) {
                self.report(
                    "SCL-003",
                    element,
                    path,
                    format!("type {id} is not defined in DataTypeTemplates"),
                );
            }
        }
    }

    /// A data attribute of basic type takes no type; an enumeration takes an EnumType, a structure a DAType.
    fn check_data_attribute(&mut self, element: &Element, path: &str, templates: &Templates<'_>) {
        let table = match element.attribute("bType") {
            Some("Enum") => &templates.enum_types,
            Some("Struct") => &templates.da_types,
            _ => return,
        };
        match element.attribute("type") {
            None => {
                let btype = element.attribute("bType").unwrap_or_default();
                self.report(
                    "SCL-003",
                    element,
                    path,
                    format!("bType {btype} requires a type"),
                );
            }
            Some(_) => self.require_type(element, path, table),
        }
    }

    fn check_ieds<'a>(&mut self, root: &'a Element, templates: &Templates<'a>) {
        for ied in root.children.iter().filter(|child| child.name == "IED") {
            let ied_path = segment("SCL", ied);
            let mut lds: Vec<(String, &'a Element)> = Vec::new();
            for access_point in ied.children.iter().filter(|c| c.name == "AccessPoint") {
                let access_path = segment(&ied_path, access_point);
                for server in access_point.children.iter().filter(|c| c.name == "Server") {
                    let server_path = segment(&access_path, server);
                    for device in server.children.iter().filter(|c| c.name == "LDevice") {
                        lds.push((segment(&server_path, device), device));
                    }
                }
            }
            let mut by_inst: HashMap<&str, &'a Element> = HashMap::new();
            for (_, device) in &lds {
                by_inst
                    .entry(device.attribute("inst").unwrap_or_default())
                    .or_insert(*device);
            }
            for (device_path, device) in lds {
                let device_inst = device.attribute("inst").unwrap_or_default();
                for node in device
                    .children
                    .iter()
                    .filter(|c| c.name == "LN0" || c.name == "LN")
                {
                    let node_path = segment(&device_path, node);
                    self.check_node(node, &node_path, device_inst, &by_inst, templates);
                }
            }
        }
    }

    fn check_node<'a>(
        &mut self,
        node: &'a Element,
        path: &str,
        device_inst: &str,
        by_inst: &HashMap<&str, &'a Element>,
        templates: &Templates<'a>,
    ) {
        if let Some(ln_type) = node.attribute("lnType") {
            if !templates.ln_types.contains_key(ln_type) {
                self.report(
                    "SCL-003",
                    node,
                    path,
                    format!("lnType {ln_type} is not an LNodeType of DataTypeTemplates"),
                );
            }
        }
        let data_sets: HashSet<&str> = node
            .children
            .iter()
            .filter(|c| c.name == "DataSet")
            .filter_map(|c| c.attribute("name"))
            .collect();
        for data_set in node.children.iter().filter(|c| c.name == "DataSet") {
            let set_path = segment(path, data_set);
            for member in data_set.children.iter().filter(|c| c.name == "FCDA") {
                let member_path = segment(&set_path, member);
                self.check_member(member, &member_path, device_inst, by_inst, templates);
            }
        }
        for block in node
            .children
            .iter()
            .filter(|c| CONTROL_BLOCKS.contains(&c.name.as_str()))
        {
            if let Some(set) = block.attribute("datSet") {
                if !data_sets.contains(set) {
                    let block_path = segment(path, block);
                    self.report(
                        "SCL-003",
                        block,
                        &block_path,
                        format!("datSet {set} is not a DataSet of this logical node"),
                    );
                }
            }
        }
    }

    /// A data-set member (`FCDA`): its logical device and logical node exist in the same IED, and its data object
    /// and data attribute follow the templates of the node's type.
    fn check_member<'a>(
        &mut self,
        member: &'a Element,
        path: &str,
        device_inst: &str,
        by_inst: &HashMap<&str, &'a Element>,
        templates: &Templates<'a>,
    ) {
        let target_inst = member.attribute("ldInst").unwrap_or(device_inst);
        let Some(device) = by_inst.get(target_inst) else {
            self.report(
                "SCL-003",
                member,
                path,
                format!("ldInst {target_inst} is not an LDevice of this IED"),
            );
            return;
        };
        let (Some(ln_class), prefix, inst) = (
            member.attribute("lnClass"),
            member.attribute("prefix").unwrap_or_default(),
            member.attribute("lnInst").unwrap_or_default(),
        ) else {
            return;
        };
        let target = device
            .children
            .iter()
            .filter(|c| c.name == "LN0" || c.name == "LN")
            .find(|c| {
                c.attribute("lnClass") == Some(ln_class)
                    && c.attribute("prefix").unwrap_or_default() == prefix
                    && c.attribute("inst").unwrap_or_default() == inst
            });
        let Some(target) = target else {
            self.report(
                "SCL-003",
                member,
                path,
                format!("no logical node {prefix}{ln_class}{inst} in LDevice {target_inst}"),
            );
            return;
        };
        let Some(do_name) = member.attribute("doName") else {
            return;
        };
        let Some(ln_type) = target
            .attribute("lnType")
            .and_then(|id| templates.ln_types.get(id).copied())
        else {
            return;
        };
        let mut parts: Vec<&str> = do_name.split('.').collect();
        if let Some(da_name) = member.attribute("daName") {
            parts.extend(da_name.split('.'));
        }
        let mut owner: Option<&'a Element> = Some(ln_type);
        for part in &parts {
            let Some(current) = owner else {
                self.report(
                    "SCL-003",
                    member,
                    path,
                    format!("{part} cannot be resolved: the member before it is not a structure"),
                );
                return;
            };
            let Some(found) = current.children.iter().find(|c| {
                matches!(c.name.as_str(), "DO" | "DA" | "SDO" | "BDA")
                    && c.attribute("name") == Some(*part)
            }) else {
                self.report(
                    "SCL-003",
                    member,
                    path,
                    format!(
                        "{part} is not a member of {}",
                        current.attribute("id").unwrap_or(current.name.as_str())
                    ),
                );
                return;
            };
            owner = match found.name.as_str() {
                "DO" | "SDO" => found
                    .attribute("type")
                    .and_then(|id| templates.do_types.get(id).copied()),
                _ if found.attribute("bType") == Some("Struct") => found
                    .attribute("type")
                    .and_then(|id| templates.da_types.get(id).copied()),
                _ => None,
            };
        }
    }
}

/// The templates of the document, by identifier.
pub(super) struct Templates<'a> {
    pub(super) ln_types: HashMap<&'a str, &'a Element>,
    pub(super) do_types: HashMap<&'a str, &'a Element>,
    pub(super) da_types: HashMap<&'a str, &'a Element>,
    pub(super) enum_types: HashMap<&'a str, &'a Element>,
}

impl<'a> Templates<'a> {
    pub(super) fn build(root: &'a Element) -> Self {
        let mut templates = Templates {
            ln_types: HashMap::new(),
            do_types: HashMap::new(),
            da_types: HashMap::new(),
            enum_types: HashMap::new(),
        };
        let Some(types) = root
            .children
            .iter()
            .find(|child| child.name == "DataTypeTemplates")
        else {
            return templates;
        };
        for node in &types.children {
            let Some(id) = node.attribute("id") else {
                continue;
            };
            let table = match node.name.as_str() {
                "LNodeType" => &mut templates.ln_types,
                "DOType" => &mut templates.do_types,
                "DAType" => &mut templates.da_types,
                "EnumType" => &mut templates.enum_types,
                _ => continue,
            };
            table.entry(id).or_insert(node);
        }
        templates
    }
}

/// The path of a child, with what names it: its `name`, or for a logical node its class and instance, or its `inst`.
pub(super) fn segment(path: &str, child: &Element) -> String {
    let identity = child
        .attribute("name")
        .map(str::to_string)
        .or_else(|| {
            child
                .attribute("lnClass")
                .map(|class| format!("{class}{}", child.attribute("inst").unwrap_or_default()))
        })
        .or_else(|| child.attribute("inst").map(str::to_string));
    match identity {
        Some(identity) => format!("{path}/{}[{identity}]", child.name),
        None => format!("{path}/{}", child.name),
    }
}

/// Collects the elements named `name` below `element`, with their paths. `Private` is opaque and not searched.
fn collect<'a>(element: &'a Element, path: &str, name: &str, out: &mut Vec<(String, &'a Element)>) {
    for child in &element.children {
        if child.name == "Private" {
            continue;
        }
        let child_path = segment(path, child);
        if child.name == name {
            out.push((child_path.clone(), child));
        }
        collect(child, &child_path, name, out);
    }
}

/// Why a value is not admitted by its kind, or `None` when it is.
fn check_value(kind: &AttrKind, value: &str) -> Option<String> {
    match kind {
        AttrKind::Text => None,
        AttrKind::Boolean => (!matches!(value, "true" | "false" | "0" | "1"))
            .then(|| format!("is not a boolean: {value:?}")),
        AttrKind::Integer { min, max } => match value.parse::<i64>() {
            Ok(number) if (*min..=*max).contains(&number) => None,
            Ok(_) => Some(format!("is out of the range {min}..={max}: {value:?}")),
            Err(_) => Some(format!("is not an integer: {value:?}")),
        },
        AttrKind::Enumeration(values) => {
            (!values.contains(&value)).then(|| format!("is not one of {values:?}: {value:?}"))
        }
    }
}
