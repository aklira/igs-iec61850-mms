// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The configuration of one IED, built from a validated SCL document (task S3; SCL-001, SCL-003, SCOPE-002).
//!
//! [`build`] turns the logical devices, logical nodes, data objects and data sets of the IED named by the caller into
//! a [`ServerModel`], and reads the address parameters of its connections. Declared services that the server does not
//! execute are refused (SCOPE-002), and so is every element that would change the model without being in it: an
//! instance value, an instance outside its logical node's type, a member of the template that the catalogue class does
//! not instantiate. The build is all or nothing: any finding gives its diagnostics, and no configuration (SCL-004).
//!
//! The document must have passed [`validate`](super::validate::validate) first: the references are then known to
//! resolve, and the build reports only what the validation does not cover.

use std::collections::{BTreeSet, HashMap};

use crate::model::layout::{layout, IDENTIFIER_LIMIT};
use crate::model::{DataObjectSpec, LogicalDevice, LogicalNode, Node, ServerModel};

use super::validate::{segment, Diagnostic, Templates};
use super::{Document, Element};

/// The most nested levels of a template that the member check follows (the depth of the catalogue's own data objects).
const MAX_TEMPLATE_DEPTH: usize = 8;

/// The services and declarations that the server executes. Any other declared service is refused (SCOPE-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    executed: BTreeSet<String>,
}

impl Capabilities {
    /// What this build executes: the directory and the definitions, and the reads and writes of the model. Data sets
    /// are declared in the configuration but not yet served; reports, logs, controls and setting groups are not executed.
    pub fn current() -> Self {
        Self::from_names(["GetDirectory", "GetDataObjectDefinition", "ReadWrite"])
    }

    /// The capabilities with exactly these names, the names of the SCL elements (`ReadWrite`, `ReportControl`…).
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            executed: names.into_iter().map(Into::into).collect(),
        }
    }

    /// Whether the server executes what this element declares.
    pub fn executes(&self, name: &str) -> bool {
        self.executed.contains(name)
    }
}

/// The address parameters of one connection of the IED: the access point and its declared `Address/P` entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    /// The access point of the connection (`apName`).
    pub access_point: String,
    /// The parameters, as (type, value), in document order (`IP`, `OSI-TSEL`…; the values are not interpreted).
    pub parameters: Vec<(String, String)>,
}

/// A member of a data set: the logical device and node that hold it, its functional constraint, and its data object
/// and optional data attribute (the names of the SCL, `FCDA`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The logical device (`ldInst`, or the device of the data set).
    pub device: String,
    /// The logical node (`prefix` + `lnClass` + `lnInst`, `LLN0` for the node 0).
    pub node: String,
    /// The functional constraint, as the SCL gives it.
    pub functional_constraint: String,
    /// The data object.
    pub data_object: String,
    /// The data attribute, when the member names one.
    pub data_attribute: Option<String>,
}

/// A variable of the model that a data set holds: its logical device, and its name within that device (the flattened
/// name of the served model, `LN$FC$DO$DA`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableRef {
    /// The logical device (the domain of the variable).
    pub device: String,
    /// The name of the variable within the device.
    pub name: String,
}

/// A data set of the IED: the logical node that holds it, its name, its members in document order, and the variables
/// they select, in the same order (a member without a data attribute selects every attribute of its data object under
/// its functional constraint, in the order of the model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSet {
    /// The logical device of the data set.
    pub device: String,
    /// The logical node of the data set.
    pub node: String,
    /// The name of the data set.
    pub name: String,
    /// The members, in document order.
    pub members: Vec<Member>,
    /// The variables that the members select, in order.
    pub variables: Vec<VariableRef>,
}

/// A data set as read, before its members are resolved against the model: with the location of its element.
struct Pending {
    set: DataSet,
    path: String,
    line: u32,
}

/// The configuration of one IED: its model, its data sets, and its address parameters.
#[derive(Debug)]
pub struct Configuration {
    /// The name of the IED the configuration was built for.
    pub ied: String,
    /// The model of the IED: its logical devices, nodes and data objects.
    pub model: ServerModel,
    /// The data sets, in document order.
    pub data_sets: Vec<DataSet>,
    /// The address parameters of the connections of the IED.
    pub addresses: Vec<Address>,
}

/// Builds the configuration of the IED `ied` of a validated document, with what the server executes. Returns every
/// finding, sorted by line, when there is one; no configuration is then returned.
pub fn build(
    document: &Document,
    file: &str,
    ied: &str,
    capabilities: &Capabilities,
) -> Result<Configuration, Vec<Diagnostic>> {
    let root = &document.root;
    let mut builder = Builder {
        file,
        capabilities,
        diagnostics: Vec::new(),
        pending: Vec::new(),
    };
    let Some(ied_element) = root
        .children
        .iter()
        .find(|child| child.name == "IED" && child.attribute("name") == Some(ied))
    else {
        builder.report(
            "SCL-001",
            root,
            "SCL",
            format!("no IED named {ied} in this file; the IED to build is selected by its name"),
        );
        return Err(builder.into_diagnostics());
    };
    let templates = Templates::build(root);
    let ied_path = segment("SCL", ied_element);

    let mut model = ServerModel::new();
    for access_point in ied_element
        .children
        .iter()
        .filter(|child| child.name == "AccessPoint")
    {
        let access_path = segment(&ied_path, access_point);
        builder.services(access_point, &access_path);
        for server in access_point
            .children
            .iter()
            .filter(|child| child.name == "Server")
        {
            let server_path = segment(&access_path, server);
            for device in server
                .children
                .iter()
                .filter(|child| child.name == "LDevice")
            {
                let device_path = segment(&server_path, device);
                let inst = device.attribute("inst").unwrap_or_default();
                let mut nodes = Vec::new();
                for node in device
                    .children
                    .iter()
                    .filter(|child| child.name == "LN0" || child.name == "LN")
                {
                    let node_path = segment(&device_path, node);
                    if let Some(logical) = builder.logical_node(node, &node_path, inst, &templates)
                    {
                        nodes.push(logical);
                    }
                }
                match LogicalDevice::new(inst, nodes) {
                    Ok(logical_device) => {
                        if let Err(error) = model.add_device(logical_device) {
                            builder.report(
                                "SCL-003",
                                device,
                                &device_path,
                                format!("the model refuses the device: {error}"),
                            );
                        }
                    }
                    Err(error) => builder.report(
                        "SCL-003",
                        device,
                        &device_path,
                        format!("the model refuses the device: {error}"),
                    ),
                }
            }
        }
    }
    let addresses = builder.addresses(root, ied);
    let data_sets = builder.resolve_data_sets(&model);
    builder.finish()?;
    Ok(Configuration {
        ied: ied.to_string(),
        model,
        data_sets,
        addresses,
    })
}

struct Builder<'a> {
    file: &'a str,
    capabilities: &'a Capabilities,
    diagnostics: Vec<Diagnostic>,
    pending: Vec<Pending>,
}

impl Builder<'_> {
    fn report(&mut self, code: &'static str, element: &Element, path: &str, message: String) {
        self.diagnostics.push(Diagnostic {
            code,
            file: self.file.to_string(),
            path: path.to_string(),
            line: element.line,
            message,
        });
    }

    fn finish(self) -> Result<(), Vec<Diagnostic>> {
        if self.diagnostics.is_empty() {
            return Ok(());
        }
        Err(self.into_diagnostics())
    }

    fn into_diagnostics(mut self) -> Vec<Diagnostic> {
        self.diagnostics
            .sort_by(|a, b| (a.line, a.code, &a.path).cmp(&(b.line, b.code, &b.path)));
        self.diagnostics
    }

    /// Resolves every data set against the model: each member selects the variables of its data object (and its data
    /// attribute) under its functional constraint, in the order of the model. A member that selects none is refused
    /// (SCL-003), and the data sets are returned only when every member resolves.
    fn resolve_data_sets(&mut self, model: &ServerModel) -> Vec<DataSet> {
        let mut names: HashMap<(String, String), Vec<String>> = HashMap::new();
        for device in model.devices() {
            for node in device.nodes() {
                if let Ok(node_layout) = layout(node, IDENTIFIER_LIMIT) {
                    let variables = node_layout.variables.into_iter().map(|v| v.name).collect();
                    names.insert(
                        (device.name().to_string(), node.name().to_string()),
                        variables,
                    );
                }
            }
        }
        let mut resolved = Vec::new();
        for pending in std::mem::take(&mut self.pending) {
            let mut set = pending.set;
            let mut variables = Vec::new();
            for member in &set.members {
                let prefix = member_prefix(member);
                let found: Vec<String> = names
                    .get(&(member.device.clone(), member.node.clone()))
                    .map(|candidates| select(candidates, &prefix, member.data_attribute.is_some()))
                    .unwrap_or_default();
                if found.is_empty() {
                    self.diagnostics.push(Diagnostic {
                        code: "SCL-003",
                        file: self.file.to_string(),
                        path: pending.path.clone(),
                        line: pending.line,
                        message: format!(
                            "the member {}/{prefix} of the data set {} selects no variable of the model",
                            member.device, set.name
                        ),
                    });
                }
                variables.extend(found.into_iter().map(|name| VariableRef {
                    device: member.device.clone(),
                    name,
                }));
            }
            set.variables = variables;
            resolved.push(set);
        }
        resolved
    }

    /// SCOPE-002: a service declared under an access point, which the server does not execute.
    fn services(&mut self, access_point: &Element, path: &str) {
        for services in access_point
            .children
            .iter()
            .filter(|child| child.name == "Services")
        {
            let services_path = segment(path, services);
            for declared in &services.children {
                if !self.capabilities.executes(&declared.name) {
                    self.report(
                        "SCOPE-002",
                        declared,
                        &segment(&services_path, declared),
                        format!(
                            "the declared service {} is not executed by this server",
                            declared.name
                        ),
                    );
                }
            }
        }
    }

    /// The address parameters of the connections of the IED, from the communication section.
    fn addresses(&mut self, root: &Element, ied: &str) -> Vec<Address> {
        let mut addresses = Vec::new();
        let Some(communication) = root
            .children
            .iter()
            .find(|child| child.name == "Communication")
        else {
            return addresses;
        };
        for subnetwork in communication
            .children
            .iter()
            .filter(|child| child.name == "SubNetwork")
        {
            for connected in subnetwork
                .children
                .iter()
                .filter(|child| child.name == "ConnectedAP")
            {
                if connected.attribute("iedName") != Some(ied) {
                    continue;
                }
                let parameters = connected
                    .children
                    .iter()
                    .filter(|child| child.name == "Address")
                    .flat_map(|address| address.children.iter())
                    .filter(|parameter| parameter.name == "P")
                    .map(|parameter| {
                        (
                            parameter.attribute("type").unwrap_or_default().to_string(),
                            parameter.text.clone(),
                        )
                    })
                    .collect();
                addresses.push(Address {
                    access_point: connected
                        .attribute("apName")
                        .unwrap_or_default()
                        .to_string(),
                    parameters,
                });
            }
        }
        addresses
    }

    /// One logical node: its data objects from its type, the instances, the data sets, and the declared blocks.
    fn logical_node(
        &mut self,
        node: &Element,
        path: &str,
        device_inst: &str,
        templates: &Templates<'_>,
    ) -> Option<LogicalNode> {
        let prefix = node.attribute("prefix").unwrap_or_default();
        let class = node.attribute("lnClass").unwrap_or_default();
        let inst = node.attribute("inst").unwrap_or_default();
        let name = format!("{prefix}{class}{inst}");
        let Some(ln_type) = node
            .attribute("lnType")
            .and_then(|id| templates.ln_types.get(id).copied())
        else {
            self.report(
                "SCL-003",
                node,
                path,
                "the logical node has no LNodeType of DataTypeTemplates".to_string(),
            );
            return None;
        };

        let mut specs = Vec::new();
        for data_object in ln_type.children.iter().filter(|child| child.name == "DO") {
            let object_name = data_object.attribute("name").unwrap_or_default();
            let Some(do_type) = data_object
                .attribute("type")
                .and_then(|id| templates.do_types.get(id).copied())
            else {
                continue;
            };
            let attributes: Vec<&str> = do_type
                .children
                .iter()
                .filter(|child| child.name == "DA" || child.name == "SDO")
                .filter_map(|child| child.attribute("name"))
                .collect();
            specs.push(DataObjectSpec::new(object_name).with_attributes(attributes));
        }
        self.instances(node, path, ln_type, templates);

        let logical = match LogicalNode::new(&name, class, &specs) {
            Ok(logical) => logical,
            Err(error) => {
                self.report(
                    "SCL-003",
                    node,
                    path,
                    format!("the model refuses the logical node {name}: {error}"),
                );
                return None;
            }
        };
        self.members(ln_type, &logical, path, templates);
        self.data_sets(node, path, device_inst, &name);
        self.declared_blocks(node, path);
        Some(logical)
    }

    /// SCL-003: the data objects of the instances are data objects of the type, their attributes are members of
    /// the type, and no instance carries a value (values are not built in this task).
    fn instances(
        &mut self,
        node: &Element,
        path: &str,
        ln_type: &Element,
        templates: &Templates<'_>,
    ) {
        for instance in node.children.iter().filter(|child| child.name == "DOI") {
            let instance_path = segment(path, instance);
            let object_name = instance.attribute("name").unwrap_or_default();
            let Some(data_object) = ln_type
                .children
                .iter()
                .find(|child| child.name == "DO" && child.attribute("name") == Some(object_name))
            else {
                self.report(
                    "SCL-003",
                    instance,
                    &instance_path,
                    format!("DOI {object_name} is not a data object of its LNodeType"),
                );
                continue;
            };
            let do_type = data_object
                .attribute("type")
                .and_then(|id| templates.do_types.get(id).copied());
            for attribute in instance
                .children
                .iter()
                .filter(|child| child.name == "DAI" || child.name == "SDI")
            {
                let attribute_path = segment(&instance_path, attribute);
                let attribute_name = attribute.attribute("name").unwrap_or_default();
                let is_member = do_type.is_some_and(|ty| {
                    ty.children
                        .iter()
                        .any(|member| member.attribute("name") == Some(attribute_name))
                });
                if !is_member {
                    self.report(
                        "SCL-003",
                        attribute,
                        &attribute_path,
                        format!("{attribute_name} is not a member of the type of {object_name}"),
                    );
                }
                if carries_value(attribute) {
                    self.report(
                        "SCL-003",
                        attribute,
                        &attribute_path,
                        format!(
                            "the instance value of {attribute_name} is not built by this server: \
                             the value would be lost"
                        ),
                    );
                }
            }
        }
    }

    /// SCL-003: the template members of each data object are instantiated by the model; a member the catalogue
    /// class does not give is refused, rather than dropped.
    fn members(
        &mut self,
        ln_type: &Element,
        logical: &LogicalNode,
        path: &str,
        templates: &Templates<'_>,
    ) {
        for data_object in ln_type.children.iter().filter(|child| child.name == "DO") {
            let object_name = data_object.attribute("name").unwrap_or_default();
            let Some(do_type) = data_object
                .attribute("type")
                .and_then(|id| templates.do_types.get(id).copied())
            else {
                continue;
            };
            let Some(built) = logical.objects().iter().find(|e| e.name() == object_name) else {
                continue;
            };
            let object_path = format!("{path}.{object_name}");
            if let Node::Branch(members) = built.node() {
                let members = members.clone();
                self.template_members(do_type, &members, &object_path, templates, 0);
            }
        }
    }

    fn template_members(
        &mut self,
        template: &Element,
        built: &[crate::model::Element],
        path: &str,
        templates: &Templates<'_>,
        depth: usize,
    ) {
        for member in template
            .children
            .iter()
            .filter(|child| child.name == "DA" || child.name == "SDO")
        {
            let name = member.attribute("name").unwrap_or_default();
            let member_path = format!("{path}.{name}");
            let Some(found) = built.iter().find(|element| element.name() == name) else {
                self.report(
                    "SCL-003",
                    member,
                    &member_path,
                    format!("{name} is not instantiated by the catalogue class of its data object"),
                );
                continue;
            };
            if member.name == "SDO" && depth < MAX_TEMPLATE_DEPTH {
                let sub_type = member
                    .attribute("type")
                    .and_then(|id| templates.do_types.get(id).copied());
                if let (Some(sub_type), Node::Branch(sub_members)) = (sub_type, found.node()) {
                    let sub_members = sub_members.clone();
                    self.template_members(
                        sub_type,
                        &sub_members,
                        &member_path,
                        templates,
                        depth.saturating_add(1),
                    );
                }
            }
        }
    }

    /// The data sets of a node: each member names a logical device, a node, and a data object (and an attribute).
    fn data_sets(&mut self, node: &Element, path: &str, device_inst: &str, node_name: &str) {
        let mut names = BTreeSet::new();
        for data_set in node.children.iter().filter(|child| child.name == "DataSet") {
            let set_path = segment(path, data_set);
            let name = data_set.attribute("name").unwrap_or_default();
            if !names.insert(name) {
                self.report(
                    "SCL-003",
                    data_set,
                    &set_path,
                    format!("the data set {name} is declared twice in {node_name}"),
                );
                continue;
            }
            let members = data_set
                .children
                .iter()
                .filter(|child| child.name == "FCDA")
                .map(|fcda| Member {
                    device: fcda.attribute("ldInst").unwrap_or(device_inst).to_string(),
                    node: format!(
                        "{}{}{}",
                        fcda.attribute("prefix").unwrap_or_default(),
                        fcda.attribute("lnClass").unwrap_or_default(),
                        fcda.attribute("lnInst").unwrap_or_default()
                    ),
                    functional_constraint: fcda.attribute("fc").unwrap_or_default().to_string(),
                    data_object: fcda.attribute("doName").unwrap_or_default().to_string(),
                    data_attribute: fcda.attribute("daName").map(str::to_string),
                })
                .collect();
            self.pending.push(Pending {
                set: DataSet {
                    device: device_inst.to_string(),
                    node: node_name.to_string(),
                    name: name.to_string(),
                    members,
                    variables: Vec::new(),
                },
                path: set_path.clone(),
                line: data_set.line,
            });
        }
    }

    /// SCOPE-002: the blocks and the input subscriptions the node declares. They are not built by this release, so
    /// a declared one is refused whatever the capabilities say: a block accepted and then not built would be lost.
    fn declared_blocks(&mut self, node: &Element, path: &str) {
        for block in node.children.iter().filter(|child| {
            matches!(
                child.name.as_str(),
                "ReportControl"
                    | "LogControl"
                    | "GSEControl"
                    | "SampledValueControl"
                    | "SettingControl"
                    | "Log"
                    | "Inputs"
            )
        }) {
            let name = block.attribute("name").unwrap_or_default();
            self.report(
                "SCOPE-002",
                block,
                &segment(path, block),
                format!(
                    "the declared {} {name} is not built by this release, so it would be lost",
                    block.name
                ),
            );
        }
    }
}

/// The variables a member selects among the variables of its node. With a data attribute, the variable of that
/// attribute (a structure is one variable). Without one, the leaves below the data object, under its functional
/// constraint: the structures that contain other selected variables are not listed on their own (8-1, FCDA).
fn select(candidates: &[String], prefix: &str, has_attribute: bool) -> Vec<String> {
    if has_attribute {
        return candidates
            .iter()
            .filter(|name| name.as_str() == prefix)
            .cloned()
            .collect();
    }
    let below = format!("{prefix}$");
    let under: Vec<&String> = candidates
        .iter()
        .filter(|name| name.starts_with(&below))
        .collect();
    under
        .iter()
        .filter(|name| {
            !under
                .iter()
                .any(|other| other.len() > name.len() && other.starts_with(&format!("{name}$")))
        })
        .map(|name| (*name).clone())
        .collect()
}

/// The name of the variables a member selects: the logical node, the functional constraint, the data object, and the
/// data attribute (dotted in the SCL, with `$` in the model).
fn member_prefix(member: &Member) -> String {
    let mut name = format!(
        "{}${}${}",
        member.node, member.functional_constraint, member.data_object
    );
    if let Some(attribute) = &member.data_attribute {
        name.push('$');
        name.push_str(&attribute.replace('.', "$"));
    }
    name
}

/// Whether an instance element carries a value: a `Val` below it.
fn carries_value(element: &Element) -> bool {
    element
        .children
        .iter()
        .any(|child| child.name == "Val" || carries_value(child))
}
