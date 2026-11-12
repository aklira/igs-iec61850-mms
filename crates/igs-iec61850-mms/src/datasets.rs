// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Dynamic data sets (task D2; DS-001, DS-002): the data sets that a client defines, persistent in the device or
//! association-specific.
//!
//! A persistent data set is a named variable list of a device, defined by a client with a domain-specific name: the generic VMD
//! holds it, and it lasts until a client deletes it. An association-specific data set has an application association-specific
//! name (`ObjectName::AaSpecific`), which the generic VMD does not hold: the served model keeps it for the association that
//! defined it, and drops it when the association is released. A list is read and written through its members, with the same rules
//! as a read or a write of the members (the controls, the block attributes and the variables of the model).
//!
//! A static data set (task D1) cannot be deleted or defined again by a client. A dynamic data set cannot be referenced by a
//! report block in this release: the blocks reference the data sets of their configuration (a gap recorded in D-030).

use std::collections::BTreeMap;

use igs_iso9506_mms::server::{Peer, PeerId};
use igs_iso9506_mms::vmd::Target;
use igs_iso9506_mms_codec::generated::types::{
    ConfirmedServiceRequest, ConfirmedServiceResponse, DataAccessError,
    DefineNamedVariableListResponse, DeleteNamedVariableListRequest,
    DeleteNamedVariableListRequestScopeOfDelete, DeleteNamedVariableListResponse,
    GetNamedVariableListAttributesResponse,
    GetNamedVariableListAttributesResponseListOfVariableItem, Identifier, ObjectName, ReadRequest,
    ServiceError, ServiceErrorErrorClass, ServiceErrorErrorClassAccess,
    ServiceErrorErrorClassDefinition, Unsigned32, VariableAccessSpecification,
    VariableAccessSpecificationListOfVariableItem, VariableSpecification, WriteRequest,
};

use crate::model::ServedModel;

/// The association-specific data sets of the associations: by association, the name and the members of each set.
pub(crate) type AaLists = BTreeMap<PeerId, BTreeMap<String, Vec<ObjectName>>>;

/// The name of an association-specific object, if the name is one.
fn aa_name(name: &ObjectName) -> Option<&str> {
    match name {
        ObjectName::AaSpecific(Identifier(text)) => Some(text),
        _ => None,
    }
}

/// Whether a request is about an association-specific data set: its name, or its list, is an application association-specific name.
pub(crate) fn is_association_request(service: &ConfirmedServiceRequest) -> bool {
    match service {
        ConfirmedServiceRequest::DefineNamedVariableList(request) => {
            aa_name(&request.variable_list_name).is_some()
        }
        ConfirmedServiceRequest::GetNamedVariableListAttributes(request) => {
            aa_name(&request.0).is_some()
        }
        ConfirmedServiceRequest::DeleteNamedVariableList(request) => request
            .list_of_variable_list_name
            .iter()
            .flatten()
            .any(|name| aa_name(name).is_some()),
        ConfirmedServiceRequest::Read(request) => matches!(
            &request.variable_access_specification,
            VariableAccessSpecification::VariableListName(name) if aa_name(name).is_some()
        ),
        ConfirmedServiceRequest::Write(request) => matches!(
            &request.variable_access_specification,
            VariableAccessSpecification::VariableListName(name) if aa_name(name).is_some()
        ),
        _ => false,
    }
}

/// The MMS error of a definition or an access: the error class with its code.
fn error_of(class: ServiceErrorErrorClass) -> ServiceError {
    ServiceError {
        error_class: class,
        additional_code: None,
        additional_description: None,
        service_specific_info: None,
    }
}

fn exists() -> ServiceError {
    error_of(ServiceErrorErrorClass::Definition(
        ServiceErrorErrorClassDefinition::OBJECT_EXISTS,
    ))
}

fn non_existent() -> ServiceError {
    error_of(ServiceErrorErrorClass::Access(
        ServiceErrorErrorClassAccess::OBJECT_NON_EXISTENT,
    ))
}

fn unsupported() -> ServiceError {
    error_of(ServiceErrorErrorClass::Access(
        ServiceErrorErrorClassAccess::OBJECT_ACCESS_UNSUPPORTED,
    ))
}

impl ServedModel {
    /// The services about an association-specific data set, answered from the association's own sets, or from the variables they
    /// name through the normal read and write paths.
    pub(crate) fn dispatch_association(
        &mut self,
        peer: &Peer,
        service: ConfirmedServiceRequest,
    ) -> Result<ConfirmedServiceResponse, ServiceError> {
        let now = self.origin.elapsed();
        match service {
            ConfirmedServiceRequest::DefineNamedVariableList(request) => {
                let name = aa_name(&request.variable_list_name)
                    .ok_or_else(unsupported)?
                    .to_string();
                if self
                    .aa_lists
                    .get(&peer.id)
                    .is_some_and(|sets| sets.contains_key(&name))
                {
                    return Err(exists());
                }
                let mut members = Vec::with_capacity(request.list_of_variable.len());
                for item in &request.list_of_variable {
                    let VariableSpecification::Name(object) = &item.variable_specification else {
                        return Err(unsupported());
                    };
                    if item.alternate_access.is_some() || !self.serves(object) {
                        return Err(non_existent());
                    }
                    members.push(object.clone());
                }
                self.aa_lists
                    .entry(peer.id)
                    .or_default()
                    .insert(name, members);
                Ok(ConfirmedServiceResponse::DefineNamedVariableList(
                    DefineNamedVariableListResponse(()),
                ))
            }
            ConfirmedServiceRequest::GetNamedVariableListAttributes(request) => {
                let members = self.association_members(peer, &request.0)?;
                Ok(ConfirmedServiceResponse::GetNamedVariableListAttributes(
                    GetNamedVariableListAttributesResponse {
                        mms_deletable: false,
                        list_of_variable: members
                            .into_iter()
                            .map(|member| {
                                GetNamedVariableListAttributesResponseListOfVariableItem {
                                    variable_specification: VariableSpecification::Name(member),
                                    alternate_access: None,
                                }
                            })
                            .collect(),
                        access_control_list: None,
                    },
                ))
            }
            ConfirmedServiceRequest::DeleteNamedVariableList(request) => {
                self.delete_association(peer, &request)
            }
            ConfirmedServiceRequest::Read(request) => {
                let members = self.association_members_of(peer, &request)?;
                let items = members
                    .into_iter()
                    .map(|member| VariableAccessSpecificationListOfVariableItem {
                        variable_specification: VariableSpecification::Name(member),
                        alternate_access: None,
                    })
                    .collect();
                let read = ReadRequest {
                    specification_with_result: request.specification_with_result,
                    variable_access_specification: VariableAccessSpecification::ListOfVariable(
                        items,
                    ),
                };
                self.read_with_controls(peer, now, read)
            }
            ConfirmedServiceRequest::Write(request) => {
                let members = self.association_members_of_write(peer, &request)?;
                let items = members
                    .into_iter()
                    .map(|member| VariableAccessSpecificationListOfVariableItem {
                        variable_specification: VariableSpecification::Name(member),
                        alternate_access: None,
                    })
                    .collect();
                let write = WriteRequest {
                    variable_access_specification: VariableAccessSpecification::ListOfVariable(
                        items,
                    ),
                    list_of_data: request.list_of_data,
                };
                self.write_with_controls(peer, now, write)
            }
            other => self.dispatch_vmd(peer, other),
        }
    }

    /// Whether the served model holds a variable of this domain-specific name.
    fn serves(&self, object: &ObjectName) -> bool {
        match Target::try_from(object) {
            Ok(target) => match self.vmd.read(&target) {
                Ok(_) => true,
                Err(error) => error != DataAccessError::OBJECT_NON_EXISTENT,
            },
            Err(_) => false,
        }
    }

    fn association_members(
        &self,
        peer: &Peer,
        name: &ObjectName,
    ) -> Result<Vec<ObjectName>, ServiceError> {
        let text = aa_name(name).ok_or_else(unsupported)?;
        self.aa_lists
            .get(&peer.id)
            .and_then(|sets| sets.get(text))
            .cloned()
            .ok_or_else(non_existent)
    }

    fn association_members_of(
        &self,
        peer: &Peer,
        request: &ReadRequest,
    ) -> Result<Vec<ObjectName>, ServiceError> {
        match &request.variable_access_specification {
            VariableAccessSpecification::VariableListName(name) => {
                self.association_members(peer, name)
            }
            _ => Err(unsupported()),
        }
    }

    fn association_members_of_write(
        &self,
        peer: &Peer,
        request: &WriteRequest,
    ) -> Result<Vec<ObjectName>, ServiceError> {
        match &request.variable_access_specification {
            VariableAccessSpecification::VariableListName(name) => {
                self.association_members(peer, name)
            }
            _ => Err(unsupported()),
        }
    }

    /// DeleteNamedVariableList of association-specific sets: each set of the association that exists is deleted, and the count of
    /// the matched and of the deleted sets is answered. A static set is never among them: it is not an association-specific name.
    fn delete_association(
        &mut self,
        peer: &Peer,
        request: &DeleteNamedVariableListRequest,
    ) -> Result<ConfirmedServiceResponse, ServiceError> {
        let scope = request.scope_of_delete.as_ref().map_or(
            DeleteNamedVariableListRequestScopeOfDelete::SPECIFIC.0,
            |scope| scope.0,
        );
        if scope != DeleteNamedVariableListRequestScopeOfDelete::SPECIFIC.0 {
            return Err(unsupported());
        }
        let mut matched = 0i64;
        let mut deleted = 0i64;
        for name in request.list_of_variable_list_name.iter().flatten() {
            let text = aa_name(name).ok_or_else(unsupported)?;
            matched = matched.saturating_add(1);
            let removed = self
                .aa_lists
                .get_mut(&peer.id)
                .and_then(|sets| sets.remove(text))
                .is_some();
            if removed {
                deleted = deleted.saturating_add(1);
            }
        }
        Ok(ConfirmedServiceResponse::DeleteNamedVariableList(
            DeleteNamedVariableListResponse {
                number_matched: Unsigned32(matched),
                number_deleted: Unsigned32(deleted),
            },
        ))
    }
}
