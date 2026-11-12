// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The typed client of the discovery and of the data access (task A6; ACSI-001, ACSI-002, MODEL-002).
//!
//! The client answers with the objects of the server: the logical devices, the logical nodes, the functional groups and
//! data objects of a node, and the definition of each data object, rebuilt from the MMS type description. Its reads and
//! writes answer one result per item, in the order of the request, with the errors of 8-1 Tables 26 and 27. A listing is
//! read page by page with `continueAfter`, and each page is checked: a name that repeats or is misordered, or a listing
//! that stalls, is an error, so no object is skipped or counted twice.

use std::fmt;

use igs_iec61850_mms_core::discovery::{
    define, functional_groups, logical_nodes, Definition, DefinitionError, Group, Listing,
    PagingError,
};
use igs_iec61850_mms_core::names::{MmsIdentifier, MmsName, Name, NameError, Scope};
use igs_iso9506_mms::vmd::conforms;
use igs_iso9506_mms::{Client, ClientConfig, Error as MmsError, Events};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, DataAccessError, GetNameListRequest, GetNameListRequestObjectScope,
    GetNamedVariableListAttributesRequest, GetVariableAccessAttributesRequest, Identifier,
    ObjectClass, ObjectClassBasicObjectClass, ObjectName, ObjectNameDomainSpecific, ReadRequest,
    TypeDescription, VariableAccessSpecification, VariableAccessSpecificationListOfVariableItem,
    VariableSpecification, WriteRequest, WriteResponseItemItem,
};

/// Why a client operation fails as a whole. The results of individual items are [`ItemError`]s instead.
#[derive(Debug)]
#[non_exhaustive]
pub enum ClientError {
    /// The association or the service failed.
    Mms(MmsError),
    /// A name given to the client is not a valid IEC 61850 or MMS name.
    Name(NameError),
    /// The description of an object breaks a rule of the protocol or of the mapping.
    Definition(DefinitionError),
    /// A listing is not in order, or stalls.
    Paging(PagingError),
    /// The server answered with a different number of results than items were asked.
    Response(&'static str),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mms(error) => write!(f, "the MMS service failed: {error}"),
            Self::Name(error) => write!(f, "a name is not valid: {error}"),
            Self::Definition(error) => write!(f, "a definition is malformed: {error}"),
            Self::Paging(error) => write!(f, "a listing is not valid: {error}"),
            Self::Response(why) => write!(f, "the server answer is not valid: {why}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<NameError> for ClientError {
    fn from(error: NameError) -> Self {
        Self::Name(error)
    }
}

/// The error of one item of a read or of a write, by the ACSI error it corresponds to (8-1 Tables 26 and 27).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ItemError {
    /// The item is not accessible to the client: `access-violation`.
    AccessViolation,
    /// The item does not exist: `instance-not-available`.
    InstanceNotAvailable,
    /// The item cannot be accessed in the current state of the server, or it is held: `access-not-allowed-in-current-state`.
    /// Table 27 maps this error to two ACSI errors; the one of the current state is taken (D-016).
    AccessNotAllowedInCurrentState,
    /// A value of another type than the item's: `type-conflict` (writes only).
    TypeConflict,
    /// A value that the item does not allow: `parameter-value-inconsistent` (writes only).
    ParameterValueInconsistent,
    /// Any other data-access error: `failed-due-to-communications-constraint`.
    FailedDueToCommunicationsConstraint,
}

impl ItemError {
    /// The error of a read item (8-1 Table 26).
    pub fn from_read(error: &DataAccessError) -> Self {
        if *error == DataAccessError::OBJECT_ACCESS_DENIED {
            Self::AccessViolation
        } else if *error == DataAccessError::OBJECT_NON_EXISTENT {
            Self::InstanceNotAvailable
        } else if *error == DataAccessError::TEMPORARILY_UNAVAILABLE {
            Self::AccessNotAllowedInCurrentState
        } else {
            Self::FailedDueToCommunicationsConstraint
        }
    }

    /// The error of a written item (8-1 Table 27).
    pub fn from_write(error: &DataAccessError) -> Self {
        if *error == DataAccessError::TYPE_INCONSISTENT {
            Self::TypeConflict
        } else if *error == DataAccessError::OBJECT_VALUE_INVALID {
            Self::ParameterValueInconsistent
        } else {
            Self::from_read(error)
        }
    }
}

/// The definition of a data object, with the MMS type it was rebuilt from, so that values can be checked against it.
#[derive(Debug, Clone, PartialEq)]
pub struct DataDefinition {
    ty: TypeDescription,
    definition: Definition,
}

impl DataDefinition {
    /// The definition: a structure of the attributes of the data object, or a leaf.
    pub fn definition(&self) -> &Definition {
        &self.definition
    }

    /// Whether a value has the type of the object: the same structure, and values within the sizes.
    pub fn accepts(&self, value: &Data) -> bool {
        conforms(&self.ty, value)
    }
}

/// A client of the served model: the discovery and the data access, over one MMS association.
pub struct IecClient {
    pub(crate) client: Client,
}

impl IecClient {
    /// The typed client over an MMS client that is already connected.
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Connects to a server. The events of the association are returned with the client.
    pub async fn connect(
        address: &str,
        config: ClientConfig,
    ) -> Result<(Self, Events), ClientError> {
        let (client, events) = Client::connect(address, config)
            .await
            .map_err(ClientError::Mms)?;
        Ok((Self { client }, events))
    }

    /// Closes the association.
    pub async fn close(&self) -> Result<(), ClientError> {
        self.client.close().await.map_err(ClientError::Mms)
    }

    /// The logical devices of the server (the domains of its VMD).
    pub async fn domains(&self) -> Result<Vec<String>, ClientError> {
        self.list(
            ObjectClassBasicObjectClass::DOMAIN,
            GetNameListRequestObjectScope::VmdSpecific,
        )
        .await
    }

    /// The named variables of a logical device: the logical nodes and the flattened names of their attributes.
    pub async fn named_variables(&self, device: &str) -> Result<Vec<String>, ClientError> {
        let device = Name::new(device)?;
        self.list(
            ObjectClassBasicObjectClass::NAMED_VARIABLE,
            GetNameListRequestObjectScope::DomainSpecific(Identifier(device.as_str().to_string())),
        )
        .await
    }

    /// The logical nodes of a logical device, in the order of the server.
    pub async fn logical_nodes(&self, device: &str) -> Result<Vec<String>, ClientError> {
        Ok(logical_nodes(&self.named_variables(device).await?))
    }

    /// The functional constraints of a logical node, with the data objects that have attributes of each.
    pub async fn functional_groups(
        &self,
        device: &str,
        node: &str,
    ) -> Result<Vec<Group>, ClientError> {
        let item = item_name(device, node, None)?;
        let ty = self.type_of(&item).await?;
        functional_groups(&ty).map_err(ClientError::Definition)
    }

    /// The definition of a data object of a logical node, under its functional constraint.
    pub async fn definition(
        &self,
        device: &str,
        node: &str,
        functional_constraint: &str,
        object: &str,
    ) -> Result<DataDefinition, ClientError> {
        let item = item_name(device, node, Some((functional_constraint, object)))?;
        let ty = self.type_of(&item).await?;
        let definition = define(&ty, object).map_err(ClientError::Definition)?;
        Ok(DataDefinition { ty, definition })
    }

    /// The data sets of a logical device: the named variable lists of its domain, in the order of the server.
    pub async fn data_sets(&self, device: &str) -> Result<Vec<String>, ClientError> {
        let device = Name::new(device)?;
        self.list(
            ObjectClassBasicObjectClass::NAMED_VARIABLE_LIST,
            GetNameListRequestObjectScope::DomainSpecific(Identifier(device.as_str().to_string())),
        )
        .await
    }

    /// The members of a data set, in their order: the (device, variable) pairs that the list selects
    /// (GetDataSetDirectory, ISO 9506-2 §14.14).
    pub async fn data_set_members(
        &self,
        device: &str,
        set: &str,
    ) -> Result<Vec<(String, String)>, ClientError> {
        let response = self
            .client
            .get_named_variable_list_attributes(GetNamedVariableListAttributesRequest(data_set(
                device, set,
            )?))
            .await
            .map_err(ClientError::Mms)?;
        response
            .list_of_variable
            .into_iter()
            .map(|item| match item.variable_specification {
                VariableSpecification::Name(ObjectName::DomainSpecific(member)) => {
                    Ok((member.domain_id.0, member.item_id.0))
                }
                _ => Err(ClientError::Response(
                    "a data set member is not a domain-specific name",
                )),
            })
            .collect()
    }

    /// Reads a data set in one request: the value or the error of each member, in the order of the members
    /// (GetDataSetValues, ISO 9506-2 §14.11).
    pub async fn read_data_set(
        &self,
        device: &str,
        set: &str,
    ) -> Result<Vec<Result<Data, ItemError>>, ClientError> {
        let request = ReadRequest {
            specification_with_result: None,
            variable_access_specification: VariableAccessSpecification::VariableListName(data_set(
                device, set,
            )?),
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

    /// Reads the items, in one request. Each result is the value or the error of its item, in the order of the items.
    pub async fn read(
        &self,
        items: &[MmsName],
    ) -> Result<Vec<Result<Data, ItemError>>, ClientError> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let request = ReadRequest {
            specification_with_result: None,
            variable_access_specification: specification(items),
        };
        let response = self.client.read(request).await.map_err(ClientError::Mms)?;
        if response.list_of_access_result.len() != items.len() {
            return Err(ClientError::Response(
                "the read answers another number of items",
            ));
        }
        Ok(response
            .list_of_access_result
            .into_iter()
            .map(|result| match result {
                AccessResult::Success(data) => Ok(data),
                AccessResult::Failure(error) => Err(ItemError::from_read(&error)),
            })
            .collect())
    }

    /// Writes the items, in one request. Each result is the success or the error of its item, in the order of the items.
    pub async fn write(
        &self,
        items: &[(MmsName, Data)],
    ) -> Result<Vec<Result<(), ItemError>>, ClientError> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let names: Vec<MmsName> = items.iter().map(|(name, _)| name.clone()).collect();
        let request = WriteRequest {
            variable_access_specification: specification(&names),
            list_of_data: items.iter().map(|(_, value)| value.clone()).collect(),
        };
        let response = self.client.write(request).await.map_err(ClientError::Mms)?;
        if response.0.len() != items.len() {
            return Err(ClientError::Response(
                "the write answers another number of items",
            ));
        }
        Ok(response
            .0
            .into_iter()
            .map(|result| match result {
                WriteResponseItemItem::Success => Ok(()),
                WriteResponseItemItem::Failure(error) => Err(ItemError::from_write(&error)),
            })
            .collect())
    }

    /// A listing of names, read page by page with `continueAfter`. Each page is checked before its names are kept.
    async fn list(
        &self,
        class: ObjectClassBasicObjectClass,
        scope: GetNameListRequestObjectScope,
    ) -> Result<Vec<String>, ClientError> {
        let mut listing = Listing::new();
        loop {
            let request = GetNameListRequest {
                object_class: ObjectClass::BasicObjectClass(class.clone()),
                object_scope: scope.clone(),
                continue_after: listing.last().map(|last| Identifier(last.to_string())),
            };
            let response = self
                .client
                .get_name_list(request)
                .await
                .map_err(ClientError::Mms)?;
            let page: Vec<String> = response
                .list_of_identifier
                .into_iter()
                .map(|identifier| identifier.0)
                .collect();
            let more = response.more_follows.unwrap_or(true);
            listing.add_page(&page, more).map_err(ClientError::Paging)?;
            if !more {
                return Ok(listing.into_names());
            }
        }
    }

    /// The type description of an item.
    async fn type_of(&self, item: &MmsName) -> Result<TypeDescription, ClientError> {
        let response = self
            .client
            .get_variable_access_attributes(GetVariableAccessAttributesRequest::Name(object_name(
                item,
            )))
            .await
            .map_err(ClientError::Mms)?;
        Ok(response.type_description)
    }
}

/// The MMS name of a logical node (`device/node`) or of a data object under a functional constraint
/// (`device/node$fc$object`).
fn item_name(
    device: &str,
    node: &str,
    object: Option<(&str, &str)>,
) -> Result<MmsName, ClientError> {
    let device = Name::new(device)?;
    let text = match object {
        None => node.to_string(),
        Some((functional_constraint, object)) => format!("{node}${functional_constraint}${object}"),
    };
    Ok(MmsName::new(
        Scope::Domain(device),
        MmsIdentifier::new(&text)?,
    ))
}

/// The MMS object name of an item, in its scope.
fn object_name(item: &MmsName) -> ObjectName {
    let identifier = item.item().as_str().to_string();
    match item.scope() {
        Scope::Domain(device) => ObjectName::DomainSpecific(ObjectNameDomainSpecific {
            domain_id: Identifier(device.as_str().to_string()),
            item_id: Identifier(identifier),
        }),
        Scope::Vmd => ObjectName::VmdSpecific(Identifier(identifier)),
        Scope::AaSpecific => ObjectName::AaSpecific(Identifier(identifier)),
    }
}

/// The variable access specification of a list of items, one item each.
fn specification(items: &[MmsName]) -> VariableAccessSpecification {
    VariableAccessSpecification::ListOfVariable(
        items
            .iter()
            .map(|item| VariableAccessSpecificationListOfVariableItem {
                variable_specification: VariableSpecification::Name(object_name(item)),
                alternate_access: None,
            })
            .collect(),
    )
}

/// The MMS name of a data set of a logical device: a domain-specific name, in the device's domain.
fn data_set(device: &str, set: &str) -> Result<ObjectName, ClientError> {
    let device = Name::new(device)?;
    let set = Name::new(set)?;
    Ok(ObjectName::DomainSpecific(ObjectNameDomainSpecific {
        domain_id: Identifier(device.as_str().to_string()),
        item_id: Identifier(set.as_str().to_string()),
    }))
}
