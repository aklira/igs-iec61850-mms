// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The sample configuration (task S3) builds a server that the typed client discovers (AT-12 and AT-07 together): the
//! device, the logical nodes, the functional groups and the named variables of the built model.

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
use igs_iec61850_mms_core::scl::build::{build, Capabilities};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_link::config::LinkConfig;

/// The sample configuration of the core crate's tests.
const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");

#[tokio::test]
async fn the_sample_configuration_is_discovered_by_the_client() {
    let document = parse(SAMPLE.as_bytes(), &Limits::default()).expect("the sample parses");
    validate(&document, "sample.cid").expect("the sample is valid");
    let configuration = build(&document, "sample.cid", "IED1", &Capabilities::current())
        .expect("the sample builds");

    let identity = Identity {
        vendor: "igs".to_string(),
        model: "sample".to_string(),
        revision: "1".to_string(),
    };
    let served = ServedModel::new(&configuration.model, identity).expect("the model is served");
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
    let client = IecClient::new(client);

    assert_eq!(client.domains().await.unwrap(), vec!["LD1".to_string()]);
    let mut nodes = client.logical_nodes("LD1").await.unwrap();
    nodes.sort();
    assert_eq!(nodes, ["GGIO1".to_string(), "LLN0".to_string()]);

    let groups = client.functional_groups("LD1", "GGIO1").await.unwrap();
    let status = groups
        .iter()
        .find(|group| group.functional_constraint == "ST")
        .expect("the status group is discovered");
    assert!(status.objects.contains(&"Ind1".to_string()));

    let names = client.named_variables("LD1").await.unwrap();
    assert!(
        names.contains(&"GGIO1$ST$Ind1$stVal".to_string()),
        "{names:?}"
    );

    client.close().await.unwrap();
    server.shutdown().await;
}
