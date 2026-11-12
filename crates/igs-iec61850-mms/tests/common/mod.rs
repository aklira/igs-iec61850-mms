// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The served model of the tests: a model built in code, a server on the loopback interface, and a client.

#![allow(
    dead_code,
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
use igs_iso9506_mms::{Client, Reconnect};
use igs_iso9506_mms_codec::generated::types::{
    GetNameListRequest, GetNameListRequestObjectScope, Identifier, ObjectClass,
    ObjectClassBasicObjectClass, ObjectName, ObjectNameDomainSpecific, VariableAccessSpecification,
    VariableAccessSpecificationListOfVariableItem, VariableSpecification, WriteRequest,
};
use igs_iso9506_mms_link::config::LinkConfig;

pub const DOMAIN: &str = "IED1LD1";

/// The mandatory data objects of a class, and the named ones after them.
pub fn with(class: &str, named: Vec<DataObjectSpec>) -> Vec<DataObjectSpec> {
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

/// The model: the device `IED1LD1`, with the node `GGIO1` (a status, a measurement, a substitution and an enumerated
/// status with its substitution), and a node whose flattened names pass the identifier limit of the ISO profile.
pub fn model() -> ServerModel {
    let ggio = LogicalNode::new(
        "GGIO1",
        "GGIO",
        &with(
            "GGIO",
            vec![
                DataObjectSpec::new("Ind1").with_attributes(["subVal", "subEna"]),
                DataObjectSpec::new("AnIn1"),
                DataObjectSpec::new("Health").with_attributes(["subVal"]),
            ],
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

pub fn identity() -> Identity {
    Identity {
        vendor: "igs".to_string(),
        model: "served".to_string(),
        revision: "1".to_string(),
    }
}

/// Starts the served model and connects a client to it, both on the IEC 61850 profile.
pub async fn start() -> (Server, Client) {
    let served = ServedModel::new(&model(), identity()).expect("the model is served");
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    let config = never_reconnect(client_config(LinkConfig::default()));
    let (client, _events) =
        tokio::time::timeout(Duration::from_secs(10), Client::connect(&address, config))
            .await
            .expect("the association is established in time")
            .expect("the association is accepted");
    (server, client)
}

fn never_reconnect(config: igs_iso9506_mms::ClientConfig) -> igs_iso9506_mms::ClientConfig {
    igs_iso9506_mms::ClientConfig {
        reconnect: Reconnect::Never,
        ..config
    }
}

/// Every named variable of the domain, over the pages of GetNameList.
pub async fn all_names(client: &Client) -> Vec<String> {
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

pub fn domain_item(item: &str) -> ObjectName {
    ObjectName::DomainSpecific(ObjectNameDomainSpecific {
        domain_id: Identifier(DOMAIN.to_string()),
        item_id: Identifier(item.to_string()),
    })
}

pub fn one(object: ObjectName) -> VariableAccessSpecification {
    VariableAccessSpecification::ListOfVariable(vec![
        VariableAccessSpecificationListOfVariableItem {
            variable_specification: VariableSpecification::Name(object),
            alternate_access: None,
        },
    ])
}

/// A Write of several items, one value each.
pub fn write_of(items: Vec<&str>, values: Vec<igs_iso9506_mms_codec::mms::Data>) -> WriteRequest {
    WriteRequest {
        variable_access_specification: VariableAccessSpecification::ListOfVariable(
            items
                .into_iter()
                .map(|item| VariableAccessSpecificationListOfVariableItem {
                    variable_specification: VariableSpecification::Name(domain_item(item)),
                    alternate_access: None,
                })
                .collect(),
        ),
        list_of_data: values,
    }
}
