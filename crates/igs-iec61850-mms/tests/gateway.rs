// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The gateway building blocks in one runtime (task G1; GW-001): two downstream servers, two downstream clients, and one upstream server whose
//! model the mirror sets. An upstream client reads the mirrored values; a change of a downstream value reaches the upstream model, and the
//! other downstream value stays as it is.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::gateway::{run_mirror, Mirror};
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::model::{
    is_required, DataObjectSpec, LogicalDevice, LogicalNode, ServerModel,
};
use igs_iec61850_mms_core::names::{MmsIdentifier, MmsName, Name, Scope};
use igs_iec61850_mms_core::scl::build::{build, Capabilities};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");

fn identity(model: &str) -> Identity {
    Identity {
        vendor: "igs".to_string(),
        model: model.to_string(),
        revision: "1".to_string(),
    }
}

/// A downstream server: the sample configuration, served on the loopback interface. Returns the server, its updater, and its address.
async fn downstream(name: &str) -> (Server, Updater, String) {
    let document = parse(SAMPLE.as_bytes(), &Limits::default()).expect("parses");
    validate(&document, "sample.cid").expect("valid");
    let configuration =
        build(&document, "sample.cid", "IED1", &Capabilities::current()).expect("builds");
    let served = ServedModel::from_configuration(&configuration, identity(name)).expect("served");
    let updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("binds");
    let address = server.local_address().to_string();
    (server, updater, address)
}

/// The upstream model: the devices `DOWN1` and `DOWN2`, each with the node GGIO1 of the status Ind1.
fn upstream_model() -> ServerModel {
    let mut server = ServerModel::new();
    for device in ["DOWN1", "DOWN2"] {
        let class = catalogue::ln_class("GGIO").expect("GGIO");
        let mut specs: Vec<DataObjectSpec> = class
            .all_data_objects()
            .into_iter()
            .filter(|object| is_required(&object.presence))
            .map(|object| DataObjectSpec::new(object.name))
            .collect();
        specs.push(DataObjectSpec::new("Ind1"));
        let node = LogicalNode::new("GGIO1", "GGIO", &specs).expect("the node is valid");
        server
            .add_device(LogicalDevice::new(device, vec![node]).expect("the device"))
            .expect("added");
    }
    server
}

async fn connect(address: &str) -> IecClient {
    let config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, _events) =
        tokio::time::timeout(Duration::from_secs(10), Client::connect(address, config))
            .await
            .expect("in time")
            .expect("accepted");
    IecClient::new(client)
}

fn status_of(device: &str) -> MmsName {
    MmsName::new(
        Scope::Domain(Name::new(device).expect("device")),
        MmsIdentifier::new("GGIO1$ST$Ind1$stVal").expect("name"),
    )
}

#[tokio::test]
async fn a_change_downstream_reaches_the_upstream_model_and_the_upstream_client_reads_it() {
    let (down1, updater1, address1) = downstream("down1").await;
    let (down2, _updater2, address2) = downstream("down2").await;

    // The upstream server, in the same runtime as the downstream clients.
    let upstream = ServedModel::new(&upstream_model(), identity("up")).expect("served");
    let upstream_updater = upstream.updater();
    let up = Server::bind_model(
        "127.0.0.1:0",
        server_config(LinkConfig::default()),
        upstream,
    )
    .await
    .expect("binds");
    let up_address = up.local_address().to_string();

    let client1 = connect(&address1).await;
    let client2 = connect(&address2).await;
    let mirrors = |downstream_device: &str, upstream_device: &str| {
        vec![Mirror {
            downstream_device: downstream_device.to_string(),
            downstream_variable: "GGIO1$ST$Ind1$stVal".to_string(),
            upstream_device: upstream_device.to_string(),
            upstream_variable: "GGIO1$ST$Ind1$stVal".to_string(),
        }]
    };
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let task1 = tokio::spawn(run_mirror(
        client1,
        upstream_updater.clone(),
        mirrors("LD1", "DOWN1"),
        Duration::from_millis(50),
        stop_rx.clone(),
    ));
    let task2 = tokio::spawn(run_mirror(
        client2,
        upstream_updater.clone(),
        mirrors("LD1", "DOWN2"),
        Duration::from_millis(50),
        stop_rx.clone(),
    ));

    // The first downstream value changes; the second does not.
    updater1
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("the downstream value is updated");

    let upstream_client = connect(&up_address).await;
    let mut seen_true = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let results = upstream_client
            .read(&[status_of("DOWN1")])
            .await
            .expect("the upstream read");
        if matches!(results.first(), Some(Ok(Data::Boolean(true)))) {
            seen_true = true;
            break;
        }
    }
    assert!(
        seen_true,
        "the change downstream reaches the upstream model"
    );
    let other = upstream_client
        .read(&[status_of("DOWN2")])
        .await
        .expect("read");
    assert!(
        matches!(other.first(), Some(Ok(Data::Boolean(false)))),
        "the other downstream value stays: {other:?}"
    );

    let _ = stop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(5), task1).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), task2).await;
    upstream_client.close().await.expect("closed");
    up.shutdown().await;
    down1.shutdown().await;
    down2.shutdown().await;
}
