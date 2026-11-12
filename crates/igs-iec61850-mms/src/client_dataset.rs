// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The typed API of the client for the dynamic data sets (task D2; DS-001): a persistent data set of a device, and an association-specific
//! data set of the association. A data set is defined by its members, which are the variables of the served model (a device, and a name
//! within it).

use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, DefineNamedVariableListRequest,
    DefineNamedVariableListRequestListOfVariableItem, DeleteNamedVariableListRequest,
    DeleteNamedVariableListRequestScopeOfDelete, Identifier, ObjectName, ObjectNameDomainSpecific,
    ReadRequest, VariableAccessSpecification, VariableSpecification,
};

use crate::client::{ClientError, IecClient, ItemError};

/// The members of a data set: the (device, variable) pairs that the set selects, in order.
fn members_of(
    members: &[(String, String)],
) -> Result<Vec<DefineNamedVariableListRequestListOfVariableItem>, ClientError> {
    members
        .iter()
        .map(|(device, variable)| {
            Ok(DefineNamedVariableListRequestListOfVariableItem {
                variable_specification: VariableSpecification::Name(ObjectName::DomainSpecific(
                    ObjectNameDomainSpecific {
                        domain_id: Identifier(device.clone()),
                        item_id: Identifier(variable.clone()),
                    },
                )),
                alternate_access: None,
            })
        })
        .collect()
}

/// The name of a persistent data set of a device.
fn persistent(device: &str, set: &str) -> ObjectName {
    ObjectName::DomainSpecific(ObjectNameDomainSpecific {
        domain_id: Identifier(device.to_string()),
        item_id: Identifier(set.to_string()),
    })
}

/// The name of an association-specific data set.
fn association(set: &str) -> ObjectName {
    ObjectName::AaSpecific(Identifier(set.to_string()))
}

impl IecClient {
    /// Defines a persistent data set of a device, with these members (DS-001): the server keeps it until a client deletes it.
    pub async fn define_data_set(
        &self,
        device: &str,
        set: &str,
        members: &[(String, String)],
    ) -> Result<(), ClientError> {
        let request = DefineNamedVariableListRequest {
            variable_list_name: persistent(device, set),
            list_of_variable: members_of(members)?,
        };
        self.client
            .define_named_variable_list(request)
            .await
            .map(|_| ())
            .map_err(ClientError::Mms)
    }

    /// Deletes a persistent data set of a device; the number of the sets deleted (zero or one).
    pub async fn delete_data_set(&self, device: &str, set: &str) -> Result<i64, ClientError> {
        let request = DeleteNamedVariableListRequest {
            scope_of_delete: Some(DeleteNamedVariableListRequestScopeOfDelete::SPECIFIC),
            list_of_variable_list_name: Some(vec![persistent(device, set)]),
            domain_name: None,
        };
        let response = self
            .client
            .delete_named_variable_list(request)
            .await
            .map_err(ClientError::Mms)?;
        Ok(response.number_deleted.0)
    }

    /// Defines an association-specific data set of this association, with these members (DS-001): it is dropped when the association ends.
    pub async fn define_association_data_set(
        &self,
        set: &str,
        members: &[(String, String)],
    ) -> Result<(), ClientError> {
        let request = DefineNamedVariableListRequest {
            variable_list_name: association(set),
            list_of_variable: members_of(members)?,
        };
        self.client
            .define_named_variable_list(request)
            .await
            .map(|_| ())
            .map_err(ClientError::Mms)
    }

    /// Reads an association-specific data set of this association: the value or the error of each member, in order.
    pub async fn read_association_data_set(
        &self,
        set: &str,
    ) -> Result<Vec<Result<Data, ItemError>>, ClientError> {
        let request = ReadRequest {
            specification_with_result: None,
            variable_access_specification: VariableAccessSpecification::VariableListName(
                association(set),
            ),
        };
        let response = self.client.read(request).await.map_err(ClientError::Mms)?;
        Ok(response
            .list_of_access_result
            .into_iter()
            .map(|result| match result {
                AccessResult::Success(data) => Ok(data),
                AccessResult::Failure(error) => Err(ItemError::from_read(&error)),
            })
            .collect())
    }

    /// Deletes an association-specific data set of this association; the number of the sets deleted (zero or one).
    pub async fn delete_association_data_set(&self, set: &str) -> Result<i64, ClientError> {
        let request = DeleteNamedVariableListRequest {
            scope_of_delete: Some(DeleteNamedVariableListRequestScopeOfDelete::SPECIFIC),
            list_of_variable_list_name: Some(vec![association(set)]),
            domain_name: None,
        };
        let response = self
            .client
            .delete_named_variable_list(request)
            .await
            .map_err(ClientError::Mms)?;
        Ok(response.number_deleted.0)
    }
}
