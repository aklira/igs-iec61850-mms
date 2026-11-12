// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! A model built in code, served over MMS and read by the MMS client (task A3): the GetNameList and the
//! GetVariableAccessAttributes answers, and the one value behind a node and its flattened names.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::model::{
    is_required, DataObjectSpec, LogicalDevice, LogicalNode, ServerModel,
};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, GetNameListRequest, GetNameListRequestObjectScope,
    GetVariableAccessAttributesRequest, Identifier, ObjectClass, ObjectClassBasicObjectClass,
    ObjectName, ObjectNameDomainSpecific, ReadRequest, TypeDescription,
    VariableAccessSpecification, VariableAccessSpecificationListOfVariableItem,
    VariableSpecification,
};
use igs_iso9506_mms_link::config::LinkConfig;

const DOMAIN: &str = "IED1LD1";

/// The mandatory data objects of a class, and the named ones after them.
fn with(class: &str, named: Vec<DataObjectSpec>) -> Vec<DataObjectSpec> {
    let mut specs: Vec<DataObjectSpec> = catalogue::ln_class(class)
        .expect("the class is in the catalogue")
        .all_data_objects()
        .into_iter()
        .filter(|object| is_required(&object.presence))
        .map(|object| DataObjectSpec::new(object.name))
        .collect();
    specs.extend(named);
    specs
}

/// The model: the device `IED1LD1` with the node `GGIO1` (a status and a measurement) and a node whose name is
/// long enough for its flattened names to pass the identifier limit of the ISO profile.
fn model() -> ServerModel {
    let ggio = LogicalNode::new(
        "GGIO1",
        "GGIO",
        &with(
            "GGIO",
            vec![DataObjectSpec::new("Ind1"), DataObjectSpec::new("AnIn1")],
        ),
    )
    .expect("the node is valid");
    let mmxu = LogicalNode::new(
        "MMXU1234567890123456",
        "MMXU",
        &with("MMXU", vec![DataObjectSpec::new("TotW")]),
    )
    .expect("the node is valid");
    let device = LogicalDevice::new(DOMAIN, vec![ggio, mmxu]).expect("the device is valid");
    let mut server = ServerModel::new();
    server.add_device(device).expect("a new device");
    server
}

fn identity() -> Identity {
    Identity {
        vendor: "igs".to_string(),
        model: "served".to_string(),
        revision: "1".to_string(),
    }
}

fn domain_item(item: &str) -> ObjectName {
    ObjectName::DomainSpecific(ObjectNameDomainSpecific {
        domain_id: Identifier(DOMAIN.to_string()),
        item_id: Identifier(item.to_string()),
    })
}

fn one(object: ObjectName) -> VariableAccessSpecification {
    VariableAccessSpecification::ListOfVariable(vec![
        VariableAccessSpecificationListOfVariableItem {
            variable_specification: VariableSpecification::Name(object),
            alternate_access: None,
        },
    ])
}

/// Starts the served model and connects a client to it, both on the IEC 61850 profile.
async fn start() -> (Server, Client) {
    let served = ServedModel::new(&model(), identity()).expect("the model is served");
    let config = server_config(LinkConfig::default());
    let server = Server::bind_model("127.0.0.1:0", config, served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    let client_config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, _events) = tokio::time::timeout(
        Duration::from_secs(10),
        Client::connect(&address, client_config),
    )
    .await
    .expect("the association is established in time")
    .expect("the association is accepted");
    (server, client)
}

/// Every named variable of the domain, over the pages of GetNameList.
async fn all_names(client: &Client) -> Vec<String> {
    let mut all = Vec::new();
    let mut after: Option<Identifier> = None;
    loop {
        let request = GetNameListRequest {
            object_class: ObjectClass::BasicObjectClass(
                ObjectClassBasicObjectClass::NAMED_VARIABLE,
            ),
            object_scope: GetNameListRequestObjectScope::DomainSpecific(Identifier(
                DOMAIN.to_string(),
            )),
            continue_after: after.clone(),
        };
        let page = client
            .get_name_list(request)
            .await
            .expect("the names are listed");
        all.extend(page.list_of_identifier.iter().map(|name| name.0.clone()));
        if !page.more_follows.unwrap_or(true) {
            break;
        }
        after = page.list_of_identifier.last().cloned();
    }
    all
}

#[tokio::test]
async fn get_name_list_lists_the_node_and_its_flattened_names() {
    let (server, client) = start().await;
    let names = all_names(&client).await;
    assert!(names.contains(&"GGIO1".to_string()));
    assert!(names.contains(&"GGIO1$ST$Ind1$stVal".to_string()));
    assert!(names.contains(&"GGIO1$MX$AnIn1$mag$f".to_string()));
    // Thirty-four characters: beyond the ISO limit, served by the IEC limit of the VMD.
    assert!(names.contains(&"MMXU1234567890123456$MX$TotW$mag$f".to_string()));
    assert!(names.iter().all(|name| name.chars().count() <= 64));
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        names, sorted,
        "the names are listed in the order of the model"
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn get_variable_access_attributes_gives_the_type_of_each_level() {
    let (server, client) = start().await;
    let leaf = client
        .get_variable_access_attributes(GetVariableAccessAttributesRequest::Name(domain_item(
            "GGIO1$ST$Ind1$stVal",
        )))
        .await
        .expect("the attributes are given");
    assert_eq!(leaf.type_description, TypeDescription::Boolean);

    let node = client
        .get_variable_access_attributes(GetVariableAccessAttributesRequest::Name(domain_item(
            "GGIO1",
        )))
        .await
        .expect("the attributes are given");
    let TypeDescription::Structure(structure) = node.type_description else {
        panic!("the node is a structure");
    };
    let functional_constraints: Vec<String> = structure
        .components
        .iter()
        .map(|component| component.component_name.as_ref().unwrap().0.clone())
        .collect();
    // Only the constraints that the node instantiates, in the order of Figure 4 of 8-1.
    assert_eq!(functional_constraints, ["MX", "ST"]);
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_node_and_its_flattened_name_show_the_same_value() {
    let (server, client) = start().await;
    let flattened = client
        .read(ReadRequest {
            specification_with_result: None,
            variable_access_specification: one(domain_item("GGIO1$ST$Ind1$stVal")),
        })
        .await
        .expect("the value is read");
    assert_eq!(
        flattened.list_of_access_result,
        vec![AccessResult::Success(Data::Boolean(false))]
    );
    // The same value, inside the node: its status, then its first object.
    let node = client
        .read(ReadRequest {
            specification_with_result: None,
            variable_access_specification: one(domain_item("GGIO1")),
        })
        .await
        .expect("the node is read");
    let AccessResult::Success(Data::Structure(functional)) = &node.list_of_access_result[0] else {
        panic!("the node is a structure of its functional constraints");
    };
    let Data::Structure(status) = &functional[1] else {
        panic!("the status is a structure");
    };
    let Data::Structure(ind1) = &status[1] else {
        panic!("Ind1 is a structure");
    };
    assert_eq!(ind1[0], Data::Boolean(false));
    client.close().await.unwrap();
    server.shutdown().await;
}
