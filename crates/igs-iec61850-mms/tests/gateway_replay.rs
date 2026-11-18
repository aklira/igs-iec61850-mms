// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The gateway's replay of a downstream journal (task X2; 8-1 §17.3): the gateway reads the downstream journal after the last entry it
//! replayed, and appends the missed entries to its upstream journal. Over two real servers: the gateway's association is closed, the
//! downstream goes on recording, and the replay after the reconnection brings only the new entries.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::gateway::replay_journal;
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

const LOGS: &str = include_str!("data/logs.cid");
const STATUS: &str = "IED1LD1/GGIO1$ST$Ind1$stVal";
const TIMEOUT: Duration = Duration::from_secs(10);

fn configuration(text: &str) -> Configuration {
    let document = parse(text.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "logs.cid").expect("the document is valid");
    build(&document, "logs.cid", "IED1", &Capabilities::current()).expect("the document builds")
}

async fn serve(text: &str) -> (Server, Updater) {
    let served = ServedModel::from_configuration(&configuration(text), server_identity())
        .expect("the model is served");
    let updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    (server, updater)
}

async fn connect(address: &str) -> IecClient {
    let (client, _events) = tokio::time::timeout(
        TIMEOUT,
        IecClient::connect(address, client_config(LinkConfig::default())),
    )
    .await
    .expect("the association is in time")
    .expect("the association is accepted");
    client
}

async fn identifiers(client: &IecClient) -> Vec<Vec<u8>> {
    client
        .journal_entries("IED1LD1", "LLN0$C1", None)
        .await
        .expect("the journal is read")
        .into_iter()
        .map(|entry| entry.entry_identifier)
        .collect()
}

#[tokio::test]
async fn a_gateway_replays_only_the_downstream_entries_it_missed() {
    // The downstream records; the upstream journal is fed by the replay only (its own block is disabled, so that the identifiers of
    // the two journals do not come from two counters).
    let (downstream, downstream_updates) = serve(LOGS).await;
    let upstream_text = LOGS.replace("logEna=\"true\"", "logEna=\"false\"");
    let (upstream, upstream_updates) = serve(&upstream_text).await;
    let downstream_address = downstream.local_address().to_string();
    let upstream_address = upstream.local_address().to_string();

    // The gateway's first association replays everything the downstream holds: the transition at its start.
    let gateway = connect(&downstream_address).await;
    let mut after: Option<Vec<u8>> = None;
    let replayed = replay_journal(
        &gateway,
        &upstream_updates,
        ("IED1LD1", "LLN0$C1"),
        ("IED1LD1", "LLN0$C1"),
        &mut after,
    )
    .await
    .expect("the replay runs");
    assert_eq!(
        replayed, 1,
        "the transition of the downstream block at its start"
    );
    gateway.close().await.expect("the association closes");

    // The association is lost; the downstream goes on recording two changes.
    downstream_updates
        .update(&[Change::new(STATUS, Data::Boolean(true))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;
    downstream_updates
        .update(&[Change::new(STATUS, Data::Boolean(false))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // After the reconnection the replay brings the two new entries, and nothing that was replayed before.
    let gateway = connect(&downstream_address).await;
    let replayed = replay_journal(
        &gateway,
        &upstream_updates,
        ("IED1LD1", "LLN0$C1"),
        ("IED1LD1", "LLN0$C1"),
        &mut after,
    )
    .await
    .expect("the replay runs");
    assert_eq!(
        replayed, 2,
        "the two changes made while the association was lost"
    );
    let again = replay_journal(
        &gateway,
        &upstream_updates,
        ("IED1LD1", "LLN0$C1"),
        ("IED1LD1", "LLN0$C1"),
        &mut after,
    )
    .await
    .expect("the replay runs");
    assert_eq!(again, 0, "nothing new: nothing is replayed twice");

    // The upstream journal holds the downstream entries, in their order, once each.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let upstream_client = connect(&upstream_address).await;
    let downstream_ids = identifiers(&gateway).await;
    let upstream_ids = identifiers(&upstream_client).await;
    assert_eq!(
        downstream_ids.len(),
        3,
        "the transition and the two changes"
    );
    assert_eq!(
        upstream_ids, downstream_ids,
        "the upstream holds what the downstream holds"
    );

    gateway.close().await.unwrap();
    upstream_client.close().await.unwrap();
    downstream.shutdown().await;
    upstream.shutdown().await;
}
