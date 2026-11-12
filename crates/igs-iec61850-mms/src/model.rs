// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The model served over MMS (task A3): the logical devices of a [`ServerModel`] as the domains of an MMS VMD, and
//! each logical node as a named variable with its flattened names (IEC 61850-8-1 §7.3).
//!
//! The value of a logical node is one structure, held once: its named variable and each of its flattened names are
//! views of it, by path, so that a name and the node always show the same value. The variables are read-only until
//! the write guard of the data access (task A4, MODEL-005) decides which attributes accept a write.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use igs_iec61850_mms_core::model::layout::{
    defines_enumerations, layout, set_at, value_at, DefinedValues, IDENTIFIER_LIMIT,
};
use igs_iec61850_mms_core::model::{ModelError, ServerModel};

#[cfg(feature = "scl")]
use crate::reports::{Block, Wire};
use crate::reports::{ReportState, Reports};
use crate::updates::{Point, Updater};
use igs_iso9506_mms::client::{in_scope_capabilities, ClientConfig};
use igs_iso9506_mms::server::{Model, Outbound, Peer};
use igs_iso9506_mms::vmd::{Error as VmdError, Identity, NameError, Target, Variable, Vmd};
use igs_iso9506_mms::ServerConfig;
use igs_iso9506_mms_codec::generated::types::{
    ConfirmedServiceRequest, ConfirmedServiceResponse, Data, DataAccessError,
    DeleteNamedVariableListRequest, DeleteNamedVariableListRequestScopeOfDelete, ObjectName,
    ServiceError, ServiceErrorErrorClass, ServiceErrorErrorClassAccess, TypeDescription,
};
use igs_iso9506_mms_codec::mms::profile::{Model as AcsiModel, Profile};
use igs_iso9506_mms_link::config::LinkConfig;

/// The nesting level of the data structures of the IEC 61850 profile with logical nodes (8-1 clause 24).
pub const LOGICAL_NODE_NESTING_LEVEL: u8 = 6;

/// The profile of a model of logical nodes, as the IEC 61850 profile declares it.
pub fn profile() -> Profile {
    Profile::iec61850(AcsiModel {
        logical_nodes: true,
        data_sets: false,
        arrays: false,
    })
}

/// The configuration of a server that serves the model on the IEC 61850 profile.
///
/// The nesting of the link is the one of the profile. `valt` is negotiated: a structure of the model has named
/// components, which the MMS CBB rules tie to `valt` (D-009). Alternate access is refused item by item until
/// its model side is served (task A4).
pub fn server_config(mut link: LinkConfig) -> ServerConfig {
    link.nesting_level = LOGICAL_NODE_NESTING_LEVEL;
    ServerConfig {
        profile: profile(),
        ..ServerConfig::new(link)
    }
    .with_alternate_access()
}

/// The configuration of a client of the served model on the IEC 61850 profile.
pub fn client_config(mut link: LinkConfig) -> ClientConfig {
    link.nesting_level = LOGICAL_NODE_NESTING_LEVEL;
    ClientConfig {
        profile: profile(),
        ..ClientConfig::new(link, in_scope_capabilities())
    }
}

/// Why a model cannot be served.
#[derive(Debug)]
#[non_exhaustive]
pub enum ServeError {
    /// The model is not valid for the namespace (core crate).
    Model(ModelError),
    /// A name of the model is not an MMS identifier of the VMD.
    Name(NameError),
    /// The VMD refused an object (a duplicate, a missing domain).
    Vmd(VmdError),
    /// A report of the configuration refers to a variable that is not served, or to a data set that is not in the configuration.
    Report(String),
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => write!(f, "the model is not valid: {error}"),
            Self::Name(error) => write!(f, "a name cannot be served: {error}"),
            Self::Vmd(_) => write!(f, "the VMD refused an object of the model"),
            Self::Report(name) => write!(
                f,
                "a report refers to {name}, which the model does not serve"
            ),
        }
    }
}

impl std::error::Error for ServeError {}

impl From<ModelError> for ServeError {
    fn from(error: ModelError) -> Self {
        Self::Model(error)
    }
}

impl From<NameError> for ServeError {
    fn from(error: NameError) -> Self {
        Self::Name(error)
    }
}

impl From<VmdError> for ServeError {
    fn from(error: VmdError) -> Self {
        Self::Vmd(error)
    }
}

/// The value of a logical node, shared by its named variable and its flattened names, with the revision of its last
/// change: a value and its revision are read and changed under one lock (MODEL-006).
#[derive(Debug)]
pub(crate) struct NodeValue {
    pub(crate) data: Data,
    pub(crate) revision: u64,
}

/// A named variable of a logical node: a path into the value of the node, and what a write may do to it (A4).
struct NodeVariable {
    root: Arc<Mutex<NodeValue>>,
    path: Vec<usize>,
    ty: TypeDescription,
    writable: bool,
    enumerations: Vec<DefinedValues>,
}

impl Variable for NodeVariable {
    fn type_description(&self) -> &TypeDescription {
        &self.ty
    }

    fn is_writable(&self) -> bool {
        self.writable
    }

    fn read(&self) -> Result<Data, DataAccessError> {
        let state = self
            .root
            .lock()
            .map_err(|_| DataAccessError::TEMPORARILY_UNAVAILABLE)?;
        value_at(&state.data, &self.path)
            .cloned()
            .ok_or(DataAccessError::OBJECT_VALUE_INVALID)
    }

    /// Replaces the value. The write has been checked against the type of the variable by the VMD; here the
    /// enumerations must hold defined values (8-1 §8.1.3.4.4.2), and the write is refused for a read-only variable.
    fn write(&mut self, value: Data) -> Result<(), DataAccessError> {
        if !self.writable {
            return Err(DataAccessError::OBJECT_ACCESS_DENIED);
        }
        if !defines_enumerations(&value, &self.enumerations) {
            return Err(DataAccessError::OBJECT_VALUE_INVALID);
        }
        let mut state = self
            .root
            .lock()
            .map_err(|_| DataAccessError::TEMPORARILY_UNAVAILABLE)?;
        if !set_at(&mut state.data, &self.path, value) {
            return Err(DataAccessError::OBJECT_NON_EXISTENT);
        }
        state.revision = state.revision.saturating_add(1);
        Ok(())
    }
}

/// The served model of a server: the VMD that answers the services, and the updater that the application uses.
pub struct Served {
    /// The VMD that answers the services of the associations.
    pub vmd: Vmd,
    /// The handle of the application's updates to the values, qualities and timestamps (task A5).
    pub updater: Updater,
}

/// The VMD of a server model: one domain per logical device, and per logical node its named variable and its flattened
/// names, all within the identifier limit of the IEC 61850 profile; and the updater over the same values.
pub fn serve(server: &ServerModel, identity: Identity) -> Result<Served, ServeError> {
    let mut vmd = Vmd::new(identity).with_identifier_limit(IDENTIFIER_LIMIT)?;
    let mut points = BTreeMap::new();
    for device in server.devices() {
        let domain = vmd.name(device.name().as_str())?;
        vmd.add_domain(domain.clone())?;
        for node in device.nodes() {
            let node_layout = layout(node, IDENTIFIER_LIMIT)?;
            let root = Arc::new(Mutex::new(NodeValue {
                data: node_layout.root_value,
                revision: 0,
            }));
            let node_name = format!("{}/{}", device.name(), node.name());
            for variable in node_layout.variables {
                let item = vmd.name(&variable.name)?;
                let target = Target::in_domain(domain.clone(), item);
                points.insert(
                    format!("{}/{}", device.name(), variable.name),
                    Point {
                        node: node_name.clone(),
                        root: Arc::clone(&root),
                        path: variable.path.clone(),
                        ty: variable.ty.clone(),
                        enumerations: variable.enumerations.clone(),
                    },
                );
                let view = NodeVariable {
                    root: Arc::clone(&root),
                    path: variable.path,
                    ty: variable.ty,
                    writable: variable.writable,
                    enumerations: variable.enumerations,
                };
                vmd.add_variable(&target, Box::new(view))?;
            }
        }
    }
    Ok(Served {
        vmd,
        updater: Updater::new(points),
    })
}

/// A server model served on MMS: the VMD of [`serve`], answering the services of its associations.
pub struct ServedModel {
    vmd: Vmd,
    updater: Updater,
    /// The unbuffered blocks of the reports, shared with the application's handle.
    reports: Arc<Mutex<ReportState>>,
    /// The static data sets, as (device, name): the named variable lists of the configuration, which no client deletes.
    static_lists: BTreeSet<(String, String)>,
}

impl ServedModel {
    /// The served model of a server model.
    pub fn new(server: &ServerModel, identity: Identity) -> Result<Self, ServeError> {
        let served = serve(server, identity)?;
        Ok(Self {
            vmd: served.vmd,
            updater: served.updater,
            reports: Arc::new(Mutex::new(ReportState::new())),
            static_lists: BTreeSet::new(),
        })
    }

    /// The served model of a configuration built from SCL (task S3): its model, and its data sets as static named
    /// variable lists in their logical devices, in the order of their members (task D1, AT-08). A client reads and
    /// writes them through the lists; it cannot delete them (DS-002), and it cannot define a list of the same name.
    #[cfg(feature = "scl")]
    pub fn from_configuration(
        configuration: &igs_iec61850_mms_core::scl::build::Configuration,
        identity: Identity,
    ) -> Result<Self, ServeError> {
        let Served { mut vmd, updater } = serve(&configuration.model, identity)?;
        let mut static_lists = BTreeSet::new();
        for set in &configuration.data_sets {
            let domain = vmd.name(&set.device)?;
            let list = vmd.name(&set.name)?;
            let mut members = Vec::with_capacity(set.variables.len());
            for variable in &set.variables {
                let device = vmd.name(&variable.device)?;
                let item = vmd.name(&variable.name)?;
                members.push(Target::in_domain(device, item));
            }
            vmd.define_list(&Target::in_domain(domain, list), members)?;
            static_lists.insert((set.device.clone(), set.name.clone()));
        }
        let mut state = ReportState::new();
        for declared in &configuration.reports {
            let set = configuration
                .data_sets
                .iter()
                .find(|set| {
                    set.device == declared.device
                        && set.node == declared.node
                        && Some(set.name.as_str()) == declared.config.data_set.as_deref()
                })
                .ok_or_else(|| ServeError::Report(declared.name.clone()))?;
            let mut wires = Vec::with_capacity(set.variables.len());
            for variable in &set.variables {
                let wire = Wire::new(
                    &updater,
                    &variable.device,
                    &variable.name,
                    variable.member,
                    variable.triggers,
                )
                .ok_or_else(|| ServeError::Report(variable.name.clone()))?;
                wires.push(wire);
            }
            let node = format!("{}/{}", declared.device, declared.node);
            state.add_block(Block::new(
                &declared.name,
                &node,
                declared.config.clone(),
                wires,
            ));
        }
        Ok(Self {
            vmd,
            updater,
            reports: Arc::new(Mutex::new(state)),
            static_lists,
        })
    }

    /// Whether a delete request would delete a static list. Such a request is refused whole: it deletes nothing.
    fn deletes_static(&self, request: &DeleteNamedVariableListRequest) -> bool {
        let scope = request.scope_of_delete.as_ref().map_or(
            DeleteNamedVariableListRequestScopeOfDelete::SPECIFIC.0,
            |scope| scope.0,
        );
        if scope == DeleteNamedVariableListRequestScopeOfDelete::SPECIFIC.0 {
            request
                .list_of_variable_list_name
                .iter()
                .flatten()
                .any(|object| match object {
                    ObjectName::DomainSpecific(object) => self
                        .static_lists
                        .contains(&(object.domain_id.0.clone(), object.item_id.0.clone())),
                    _ => false,
                })
        } else if scope == DeleteNamedVariableListRequestScopeOfDelete::DOMAIN.0 {
            request.domain_name.as_ref().is_some_and(|domain| {
                self.static_lists
                    .iter()
                    .any(|(device, _)| *device == domain.0)
            })
        } else if scope == DeleteNamedVariableListRequestScopeOfDelete::VMD.0 {
            !self.static_lists.is_empty()
        } else {
            false
        }
    }

    /// The VMD that answers the services.
    pub fn vmd(&self) -> &Vmd {
        &self.vmd
    }

    /// The handle of the application's updates. It can be cloned and used from other tasks.
    pub fn updater(&self) -> Updater {
        self.updater.clone()
    }
}

impl ServedModel {
    /// The handle of the reports of the model: the application writes to the unbuffered blocks through it (task D4).
    pub fn reports(&self) -> Reports {
        Reports::new(Arc::clone(&self.reports))
    }
}

impl Model for ServedModel {
    fn admit(&mut self, peer: &Peer) -> bool {
        if let Ok(mut state) = self.reports.lock() {
            state.admitted(peer.id);
        }
        true
    }

    fn released(&mut self, peer: &Peer) {
        if let Ok(mut state) = self.reports.lock() {
            state.released(peer.id);
        }
    }

    fn poll(&mut self, _now: Duration) -> Vec<Outbound> {
        let (notices, dropped) = self.updater.take_notices();
        match self.reports.lock() {
            Ok(mut state) => state.poll(notices, dropped),
            Err(_) => Vec::new(),
        }
    }

    fn dispatch(
        &mut self,
        peer: &Peer,
        service: ConfirmedServiceRequest,
    ) -> Result<ConfirmedServiceResponse, ServiceError> {
        if let ConfirmedServiceRequest::DeleteNamedVariableList(request) = &service {
            if self.deletes_static(request) {
                return Err(ServiceError {
                    error_class: ServiceErrorErrorClass::Access(
                        ServiceErrorErrorClassAccess::OBJECT_ACCESS_DENIED,
                    ),
                    additional_code: None,
                    additional_description: None,
                    service_specific_info: None,
                });
            }
        }
        <Vmd as Model>::dispatch(&mut self.vmd, peer, service)
    }
}
