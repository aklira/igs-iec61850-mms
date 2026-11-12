// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! AT-08, static cases (task D1): the data sets of a configuration are listed, keep their member order, are read in that
//! order, cannot be deleted or defined again by a client, and refuse a write to a read-only member item by item.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::scl::build::{build, Capabilities};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::{
    Data, DeleteNamedVariableListRequest, DeleteNamedVariableListRequestScopeOfDelete, Identifier,
    ObjectName, ObjectNameDomainSpecific, VariableAccessSpecification, WriteRequest,
};
use igs_iso9506_mms_link::config::LinkConfig;

/// The sample configuration of the core crate's tests: the data set DS1 of the device LD1.
const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");

/// The served sample, a connected client, and the updater of the application.
async fn start() -> (Server, Client, Updater) {
    let document = parse(SAMPLE.as_bytes(), &Limits::default()).expect("the sample parses");
    validate(&document, "sample.cid").expect("the sample is valid");
    let configuration = build(&document, "sample.cid", "IED1", &Capabilities::current())
        .expect("the sample builds");
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "sample".to_string(),
        revision: "1".to_string(),
    };
    let served = ServedModel::from_configuration(&configuration, identity)
        .expect("the configuration is served");
    let updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    let config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, _events) =
        tokio::time::timeout(Duration::from_secs(10), Client::connect(&address, config))
            .await
            .expect("the association is established in time")
            .expect("the association is accepted");
    (server, client, updater)
}

/// The MMS name of the data set DS1 of the device LD1.
fn ds1() -> ObjectName {
    ObjectName::DomainSpecific(ObjectNameDomainSpecific {
        domain_id: Identifier("LD1".to_string()),
        item_id: Identifier("LLN0$DS1".to_string()),
    })
}

#[tokio::test]
async fn the_data_sets_of_the_device_are_listed() {
    let (server, client, _updater) = start().await;
    let client = IecClient::new(client);
    assert_eq!(
        client.data_sets("LD1").await.unwrap(),
        ["LLN0$DS1".to_string()]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn the_members_keep_their_document_order() {
    let (server, client, _updater) = start().await;
    let client = IecClient::new(client);
    let members = client.data_set_members("LD1", "LLN0$DS1").await.unwrap();
    assert_eq!(
        members,
        [
            ("LD1".to_string(), "LLN0$ST$Mod$stVal".to_string()),
            ("LD1".to_string(), "GGIO1$ST$Ind1$stVal".to_string()),
        ]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn the_data_set_reads_its_values_in_the_order_of_its_members() {
    let (server, client, updater) = start().await;
    updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("the value is updated");
    let client = IecClient::new(client);
    let values = client.read_data_set("LD1", "LLN0$DS1").await.unwrap();
    assert_eq!(values.len(), 2);
    assert!(matches!(values[1], Ok(Data::Boolean(true))), "{values:?}");
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_static_data_set_is_not_deleted_by_a_client() {
    let (server, client, _updater) = start().await;
    let specific = DeleteNamedVariableListRequest {
        scope_of_delete: Some(DeleteNamedVariableListRequestScopeOfDelete::SPECIFIC),
        list_of_variable_list_name: Some(vec![ds1()]),
        domain_name: None,
    };
    assert!(client.delete_named_variable_list(specific).await.is_err());
    let domain = DeleteNamedVariableListRequest {
        scope_of_delete: Some(DeleteNamedVariableListRequestScopeOfDelete::DOMAIN),
        list_of_variable_list_name: None,
        domain_name: Some(Identifier("LD1".to_string())),
    };
    assert!(client.delete_named_variable_list(domain).await.is_err());
    let client = IecClient::new(client);
    assert_eq!(
        client.data_sets("LD1").await.unwrap(),
        ["LLN0$DS1".to_string()]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_write_to_a_read_only_member_fails_that_item() {
    let (server, client, _updater) = start().await;
    let request = WriteRequest {
        variable_access_specification: VariableAccessSpecification::VariableListName(ds1()),
        list_of_data: vec![Data::Boolean(false), Data::Boolean(false)],
    };
    let response = client.write(request).await.expect("the write is answered");
    assert_eq!(response.0.len(), 2);
    for item in &response.0 {
        assert!(
            matches!(
                item,
                igs_iso9506_mms_codec::generated::types::WriteResponseItemItem::Failure(_)
            ),
            "a status member is read-only: {item:?}"
        );
    }
    client.close().await.unwrap();
    server.shutdown().await;
}
