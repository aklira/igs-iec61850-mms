// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Robustness on peer input (task X8; RUN-002): a peer that sends garbage, truncated frames or an absurd length does not stop the
//! server. Each hostile connection is closed by the server, and a well-behaved client is served afterwards.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use igs_iec61850_mms::demo::demo_model;
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::Client;
use igs_iso9506_mms_link::config::LinkConfig;

const TIMEOUT: Duration = Duration::from_secs(10);

async fn start() -> Server {
    let served = ServedModel::new(&demo_model().unwrap(), server_identity()).unwrap();
    Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds")
}

/// A well-behaved client: its association is accepted, and the server answers its Identify.
async fn served_client(address: &str) -> bool {
    let connected = tokio::time::timeout(
        TIMEOUT,
        Client::connect(address, client_config(LinkConfig::default())),
    )
    .await;
    let Ok(Ok((client, _events))) = connected else {
        return false;
    };
    let identified = tokio::time::timeout(TIMEOUT, client.identify()).await;
    let answered = matches!(identified, Ok(Ok(_)));
    let _ = client.close().await;
    answered
}

/// Deterministic pseudo-random octets: a linear congruential generator, so that a failure can be reproduced.
fn noise(seed: u64, length: usize) -> Vec<u8> {
    let mut state = seed;
    (0..length)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as u8
        })
        .collect()
}

/// The frames a hostile peer sends, one connection each: noise, an absurd TPKT length, a valid TPKT header with no body, a
/// connection closed at once, and a frame cut in the middle.
fn hostile_frames() -> Vec<Vec<u8>> {
    let mut frames = vec![
        noise(1, 4096),
        noise(2, 17),
        // TPKT (version 3, reserved 0, length 0xFFFF), then nothing.
        vec![0x03, 0x00, 0xFF, 0xFF],
        // TPKT with the length of its own header only.
        vec![0x03, 0x00, 0x00, 0x04],
        // TPKT of a length that does not fit the bytes: the frame is cut in the middle.
        vec![0x03, 0x00, 0x00, 0x20, 0x02, 0xF0],
        Vec::new(),
    ];
    frames.push(noise(3, 65_536));
    frames
}

#[tokio::test]
async fn a_server_that_receives_hostile_frames_keeps_serving_well_behaved_clients() {
    let server = start().await;
    let address = server.local_address().to_string();

    for frame in hostile_frames() {
        let peer = address.clone();
        let length = frame.len();
        let sent = tokio::task::spawn_blocking(move || {
            let Ok(mut stream) = TcpStream::connect(&peer) else {
                return false;
            };
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            // The server may close the connection while the peer still writes: a refused write is the expected end.
            let _ = stream.write_all(&frame);
            let _ = stream.shutdown(std::net::Shutdown::Both);
            true
        })
        .await
        .unwrap();
        assert!(sent, "the hostile peer connects");
        assert!(
            served_client(&address).await,
            "a well-behaved client is served after a hostile frame of {length} octets"
        );
    }

    server.shutdown().await;
}
