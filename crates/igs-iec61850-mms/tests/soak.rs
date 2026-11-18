// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The soak test of the server (task X8; RUN-002): many associations are opened and ended in three ways (an orderly close, an abort,
//! and a client dropped without a word), and the server must keep serving well-behaved clients throughout. The number of cycles is
//! `IGS_SOAK_CYCLES` (default 300); a longer run is a campaign for the maintainer, and its result is recorded in `docs/robustness.md`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::demo::demo_model;
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::Client;
use igs_iso9506_mms_link::config::LinkConfig;

const TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_CYCLES: usize = 300;
/// A well-behaved client is checked every this many cycles.
const CHECK_EVERY: usize = 25;

async fn start() -> Server {
    let served = ServedModel::new(&demo_model().unwrap(), server_identity()).unwrap();
    Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds")
}

/// Whether a new association is accepted and its Identify is answered.
async fn served(address: &str) -> bool {
    let connected = tokio::time::timeout(
        TIMEOUT,
        Client::connect(address, client_config(LinkConfig::default())),
    )
    .await;
    let Ok(Ok((client, _events))) = connected else {
        return false;
    };
    let answered = matches!(
        tokio::time::timeout(TIMEOUT, client.identify()).await,
        Ok(Ok(_))
    );
    let _ = client.close().await;
    answered
}

fn cycles() -> usize {
    std::env::var("IGS_SOAK_CYCLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_CYCLES)
}

#[tokio::test]
async fn the_server_keeps_serving_through_repeated_associations_and_losses() {
    let server = start().await;
    let address = server.local_address().to_string();
    let total = cycles();
    let mut failures = 0_usize;
    let mut checks = 0_usize;

    for cycle in 0..total {
        if let Ok(Ok((client, _events))) = tokio::time::timeout(
            TIMEOUT,
            Client::connect(&address, client_config(LinkConfig::default())),
        )
        .await
        {
            let _ = tokio::time::timeout(TIMEOUT, client.identify()).await;
            match cycle % 3 {
                0 => {
                    let _ = client.close().await;
                }
                1 => {
                    let _ = client.abort().await;
                }
                // The client is dropped without a word: the association is lost on the server's side.
                _ => drop(client),
            }
        } else {
            failures += 1;
        }

        if cycle % CHECK_EVERY == 0 {
            checks += 1;
            if !served(&address).await {
                failures += 1;
            }
        }
    }

    println!("soak: {total} cycles, {checks} checks, {failures} failures");
    server.shutdown().await;
    assert_eq!(failures, 0, "every association is accepted and served");
}
