// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The embedded SNTP client over UDP on the loopback interface (TIME-002, TIME-003; task O5; D-052): a server answers the
//! requests of the client, and the clock follows the replies that pass the checks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use igs_iec61850_mms::sntp::SntpClock;
use igs_iec61850_mms_core::sntp::NtpTime;
use igs_iec61850_mms_core::time::ClockSource;
use igs_iso9506_mms_codec::mms::{TimeAccuracy, UtcTimestamp};
use tokio::net::UdpSocket;
use tokio::time::{sleep, timeout};

/// The local time as an NTP timestamp, the way the server stamps its reply.
fn ntp_now() -> NtpTime {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    NtpTime::from_unix(since.as_secs(), since.subsec_nanos()).unwrap()
}

/// The reply to a request, in the layout of RFC 5905 §7.3: the origin is the transmit timestamp of the request, unless
/// `origin` replaces it. The server's receive and transmit timestamps are its own clock.
fn reply(
    request: &[u8],
    first: u8,
    stratum: u8,
    root_dispersion: u32,
    origin: Option<[u8; 8]>,
) -> Vec<u8> {
    let mut bytes = vec![0_u8; 48];
    bytes[0] = first;
    bytes[1] = stratum;
    bytes[8..12].copy_from_slice(&root_dispersion.to_be_bytes());
    let origin = origin.unwrap_or_else(|| request[40..48].try_into().unwrap());
    bytes[24..32].copy_from_slice(&origin);
    let now = ntp_now();
    let stamp = [now.seconds.to_be_bytes(), now.fraction.to_be_bytes()].concat();
    bytes[32..40].copy_from_slice(&stamp);
    bytes[40..48].copy_from_slice(&stamp);
    bytes
}

/// A server on the loopback interface that answers each request with `answer`, or not at all when it returns `None`.
async fn server(answer: fn(&[u8]) -> Option<Vec<u8>>) -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buffer = [0_u8; 512];
        loop {
            let Ok((length, from)) = socket.recv_from(&mut buffer).await else {
                return;
            };
            if let Some(bytes) = buffer.get(..length).and_then(answer) {
                let _ = socket.send_to(&bytes, from).await;
            }
        }
    });
    address
}

/// Waits until the clock reports a synchronised reading, and returns it; `None` when it does not within `limit`.
async fn synchronised_within(clock: &SntpClock, limit: Duration) -> Option<UtcTimestamp> {
    timeout(limit, async {
        loop {
            let stamp = clock.now();
            if !stamp.quality.clock_not_synchronized {
                return stamp;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .ok()
}

#[tokio::test]
async fn the_clock_is_not_synchronised_before_any_reply() {
    let clock = SntpClock::new();
    let stamp = clock.now();
    assert!(stamp.quality.clock_not_synchronized);
    assert_eq!(stamp.quality.accuracy, TimeAccuracy::Unspecified);
}

#[tokio::test]
async fn a_reply_to_the_request_synchronises_the_clock_with_its_accuracy() {
    // The root dispersion is 1/256 s (256 units of 2^-16 s): an accuracy of 16 bits of the fraction.
    let address = server(|request| Some(reply(request, 0x24, 2, 256, None))).await;
    let clock = Arc::new(SntpClock::new());
    let task = tokio::spawn(Arc::clone(&clock).run(address));

    let stamp = synchronised_within(&clock, Duration::from_secs(5))
        .await
        .expect("the clock is synchronised by the reply");
    task.abort();

    assert_eq!(stamp.quality.accuracy, TimeAccuracy::Bits(16));
    assert!(stamp.quality.leap_second_known);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(
        now.abs_diff(u64::from(stamp.seconds)) <= 2,
        "the stamp is the time of the server, which is this host's time"
    );
}

#[tokio::test]
async fn a_reply_with_another_origin_does_not_synchronise_the_clock() {
    let address = server(|request| Some(reply(request, 0x24, 2, 256, Some([1; 8])))).await;
    let clock = Arc::new(SntpClock::new());
    let task = tokio::spawn(Arc::clone(&clock).run(address));

    let synchronised = synchronised_within(&clock, Duration::from_millis(800)).await;
    task.abort();
    assert!(
        synchronised.is_none(),
        "a reply that is not the answer to the request is refused"
    );
}

#[tokio::test]
async fn a_kiss_of_death_does_not_synchronise_the_clock() {
    let address = server(|request| Some(reply(request, 0x24, 0, 0, None))).await;
    let clock = Arc::new(SntpClock::new());
    let task = tokio::spawn(Arc::clone(&clock).run(address));

    let synchronised = synchronised_within(&clock, Duration::from_millis(800)).await;
    task.abort();
    assert!(
        synchronised.is_none(),
        "a kiss-o'-death stops the client from trusting the server"
    );
}

#[tokio::test]
async fn a_server_that_does_not_answer_leaves_the_clock_not_synchronised() {
    let address = server(|_| None).await;
    let clock = Arc::new(SntpClock::new());
    let task = tokio::spawn(Arc::clone(&clock).run(address));

    sleep(Duration::from_millis(600)).await;
    let stamp = clock.now();
    task.abort();
    assert!(stamp.quality.clock_not_synchronized);
}
