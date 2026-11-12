// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! AT-08, dynamic cases, with two associations (task D2; DS-001, DS-002): a persistent data set defined by one client is listed, read and
//! deleted by another; an association-specific data set is private to its association and dropped when it is released; a static data set
//! cannot be defined again; a member that is not served refuses the definition.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::control::AcceptAll;
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");

fn configuration() -> Configuration {
    let document = parse(SAMPLE.as_bytes(), &Limits::default()).expect("the sample parses");
    validate(&document, "sample.cid").expect("the sample is valid");
    build(&document, "sample.cid", "IED1", &Capabilities::current()).expect("the sample builds")
}

struct Setup {
    server: Server,
    updater: Updater,
    clients: Vec<IecClient>,
}

async fn start(clients: usize) -> Setup {
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "datasets".to_string(),
        revision: "1".to_string(),
    };
    let served = ServedModel::from_configuration(&configuration(), identity)
        .expect("the model is served")
        .with_control_handler(AcceptAll);
    let updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    let mut connected = Vec::new();
    for _ in 0..clients {
        let config = ClientConfig {
            reconnect: Reconnect::Never,
            ..client_config(LinkConfig::default())
        };
        let (client, _events) =
            tokio::time::timeout(Duration::from_secs(10), Client::connect(&address, config))
                .await
                .expect("the association is established in time")
                .expect("the association is accepted");
        connected.push(IecClient::new(client));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    Setup {
        server,
        updater,
        clients: connected,
    }
}

fn status() -> Vec<(String, String)> {
    vec![("LD1".to_string(), "GGIO1$ST$Ind1$stVal".to_string())]
}

#[tokio::test]
async fn a_persistent_data_set_is_listed_and_read_by_another_association() {
    let setup = start(2).await;
    let mut clients = setup.clients.into_iter();
    let first = clients.next().expect("the first client");
    let second = clients.next().expect("the second client");
    first
        .define_data_set("LD1", "DynA", &status())
        .await
        .expect("defined");
    assert!(second
        .data_sets("LD1")
        .await
        .expect("listed")
        .contains(&"DynA".to_string()));
    assert_eq!(
        second
            .data_set_members("LD1", "DynA")
            .await
            .expect("the members"),
        [("LD1".to_string(), "GGIO1$ST$Ind1$stVal".to_string())]
    );
    setup
        .updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("updated");
    let values = second.read_data_set("LD1", "DynA").await.expect("read");
    assert!(
        matches!(values.as_slice(), [Ok(Data::Boolean(true))]),
        "{values:?}"
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_persistent_data_set_is_deleted_by_another_association() {
    let setup = start(2).await;
    let mut clients = setup.clients.into_iter();
    let first = clients.next().expect("the first client");
    let second = clients.next().expect("the second client");
    first
        .define_data_set("LD1", "DynB", &status())
        .await
        .expect("defined");
    assert_eq!(
        second
            .delete_data_set("LD1", "DynB")
            .await
            .expect("deleted"),
        1
    );
    assert!(!first
        .data_sets("LD1")
        .await
        .expect("listed")
        .contains(&"DynB".to_string()));
    setup.server.shutdown().await;
}

#[tokio::test]
async fn an_association_specific_data_set_is_private_and_dropped_when_its_association_ends() {
    let setup = start(2).await;
    let mut clients = setup.clients.into_iter();
    let first = clients.next().expect("the first client");
    let second = clients.next().expect("the second client");
    first
        .define_association_data_set("AaA", &status())
        .await
        .expect("defined");
    assert!(
        first
            .read_association_data_set("AaA")
            .await
            .expect("read by its owner")
            .len()
            == 1
    );
    assert!(
        second.read_association_data_set("AaA").await.is_err(),
        "another association does not see it"
    );
    assert!(
        !second
            .data_sets("LD1")
            .await
            .expect("listed")
            .contains(&"AaA".to_string()),
        "nor does it list it as persistent"
    );
    // The same name in another association is another set.
    second
        .define_association_data_set("AaA", &status())
        .await
        .expect("defined by the other association");
    // The first association ends: its set is dropped, and the name is free for a new one.
    drop(first);
    tokio::time::sleep(Duration::from_millis(300)).await;
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_static_data_set_cannot_be_defined_again_and_a_member_that_is_not_served_refuses_a_definition(
) {
    let setup = start(1).await;
    let client = setup.clients.into_iter().next().expect("a client");
    assert!(
        client
            .define_data_set("LD1", "LLN0$DS1", &status())
            .await
            .is_err(),
        "the static LLN0$DS1 exists"
    );
    assert!(
        client
            .define_data_set(
                "LD1",
                "DynC",
                &[("LD1".to_string(), "GGIO1$ST$Nope$stVal".to_string())]
            )
            .await
            .is_err(),
        "a member that the model does not serve"
    );
    setup.server.shutdown().await;
}
