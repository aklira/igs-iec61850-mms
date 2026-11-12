// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The model served over MMS (task A3): the logical devices of a [`ServerModel`] as the domains of an MMS VMD, and
//! each logical node as a named variable with its flattened names (IEC 61850-8-1 §7.3).
//!
//! The value of a logical node is one structure, held once: its named variable and each of its flattened names are
//! views of it, by path, so that a name and the node always show the same value. The variables are read-only until
//! the write guard of the data access (task A4, MODEL-005) decides which attributes accept a write.

use std::fmt;
use std::sync::{Arc, Mutex};

use igs_iec61850_mms_core::model::layout::{layout, IDENTIFIER_LIMIT};
use igs_iec61850_mms_core::model::{ModelError, ServerModel};
use igs_iso9506_mms::client::{in_scope_capabilities, ClientConfig};
use igs_iso9506_mms::server::{Model, Peer};
use igs_iso9506_mms::vmd::{Error as VmdError, Identity, NameError, Target, Variable, Vmd};
use igs_iso9506_mms::ServerConfig;
use igs_iso9506_mms_codec::generated::types::{
    ConfirmedServiceRequest, ConfirmedServiceResponse, Data, DataAccessError, ServiceError,
    TypeDescription,
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
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => write!(f, "the model is not valid: {error}"),
            Self::Name(error) => write!(f, "a name cannot be served: {error}"),
            Self::Vmd(_) => write!(f, "the VMD refused an object of the model"),
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

/// A named variable of a logical node: a path into the value of the node.
struct NodeVariable {
    root: Arc<Mutex<Data>>,
    path: Vec<usize>,
    ty: TypeDescription,
}

impl Variable for NodeVariable {
    fn type_description(&self) -> &TypeDescription {
        &self.ty
    }

    fn is_writable(&self) -> bool {
        false
    }

    fn read(&self) -> Result<Data, DataAccessError> {
        let root = self
            .root
            .lock()
            .map_err(|_| DataAccessError::TEMPORARILY_UNAVAILABLE)?;
        value_at(&root, &self.path)
            .cloned()
            .ok_or(DataAccessError::OBJECT_VALUE_INVALID)
    }

    fn write(&mut self, _value: Data) -> Result<(), DataAccessError> {
        Err(DataAccessError::OBJECT_ACCESS_DENIED)
    }
}

/// The member of a value at a path of component indexes. `None` when the path leaves the structure.
fn value_at<'a>(root: &'a Data, path: &[usize]) -> Option<&'a Data> {
    let mut current = root;
    for index in path {
        match current {
            Data::Structure(members) => current = members.get(*index)?,
            _ => return None,
        }
    }
    Some(current)
}

/// The VMD of a server model: one domain per logical device, and per logical node its named variable and its
/// flattened names, all within the identifier limit of the IEC 61850 profile.
pub fn serve(server: &ServerModel, identity: Identity) -> Result<Vmd, ServeError> {
    let mut vmd = Vmd::new(identity).with_identifier_limit(IDENTIFIER_LIMIT)?;
    for device in server.devices() {
        let domain = vmd.name(device.name().as_str())?;
        vmd.add_domain(domain.clone())?;
        for node in device.nodes() {
            let node_layout = layout(node, IDENTIFIER_LIMIT)?;
            let root = Arc::new(Mutex::new(node_layout.root_value));
            for variable in node_layout.variables {
                let item = vmd.name(&variable.name)?;
                let target = Target::in_domain(domain.clone(), item);
                let view = NodeVariable {
                    root: Arc::clone(&root),
                    path: variable.path,
                    ty: variable.ty,
                };
                vmd.add_variable(&target, Box::new(view))?;
            }
        }
    }
    Ok(vmd)
}

/// A server model served on MMS: the VMD of [`serve`], answering the services of its associations.
pub struct ServedModel {
    vmd: Vmd,
}

impl ServedModel {
    /// The served model of a server model.
    pub fn new(server: &ServerModel, identity: Identity) -> Result<Self, ServeError> {
        Ok(Self {
            vmd: serve(server, identity)?,
        })
    }

    /// The VMD that answers the services.
    pub fn vmd(&self) -> &Vmd {
        &self.vmd
    }
}

impl Model for ServedModel {
    fn dispatch(
        &mut self,
        peer: &Peer,
        service: ConfirmedServiceRequest,
    ) -> Result<ConfirmedServiceResponse, ServiceError> {
        <Vmd as Model>::dispatch(&mut self.vmd, peer, service)
    }
}
