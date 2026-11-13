// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The embedded SNTP client (TIME-002, TIME-003; task O5; D-052): the packets, the checks of a reply, the offset and
//! the delay of RFC 5905, the accuracy from the root distance, and the client's requests and estimate with controlled time.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms_core::sntp::{
    accuracy_of, check_reply, exchange, parse, request, root_distance, Client, NtpTime, Packet,
    Rejection, MAX_AGE, PACKET_LEN, POLL_INTERVAL, PORT,
};
use igs_iso9506_mms_codec::mms::{TimeAccuracy, UtcTimestamp};

/// The NTP time of `seconds` seconds since 1900, with no fraction.
fn at(seconds: u32) -> NtpTime {
    NtpTime::new(seconds, 0)
}

/// The unix seconds of 2026-11-13 00:00:00 UTC, and the same in NTP seconds.
const UNIX_NOW: u32 = 1_794_528_000;
const NTP_NOW: u32 = UNIX_NOW + 2_208_988_800;

/// A server reply in the wire layout of RFC 5905 §7.3.
#[allow(clippy::too_many_arguments)]
fn reply_bytes(
    first: u8,
    stratum: u8,
    root_delay: u32,
    root_dispersion: u32,
    reference_id: u32,
    origin: NtpTime,
    receive: NtpTime,
    transmit: NtpTime,
) -> [u8; PACKET_LEN] {
    let mut bytes = [0_u8; PACKET_LEN];
    let fields: [(usize, [u8; 4]); 11] = [
        (0, [first, stratum, 0, 0]),
        (4, root_delay.to_be_bytes()),
        (8, root_dispersion.to_be_bytes()),
        (12, reference_id.to_be_bytes()),
        (24, origin.seconds.to_be_bytes()),
        (28, origin.fraction.to_be_bytes()),
        (32, receive.seconds.to_be_bytes()),
        (36, receive.fraction.to_be_bytes()),
        (40, transmit.seconds.to_be_bytes()),
        (44, transmit.fraction.to_be_bytes()),
        (16, [0; 4]),
    ];
    for (at, word) in fields {
        for (offset, value) in word.into_iter().enumerate() {
            if let Some(slot) = bytes.get_mut(at + offset) {
                *slot = value;
            }
        }
    }
    bytes
}

/// A synchronised server reply to a request sent with transmit timestamp `origin`: mode 4, version 4, leap 0.
fn good_reply(
    origin: NtpTime,
    receive: NtpTime,
    transmit: NtpTime,
    root_dispersion: u32,
) -> [u8; PACKET_LEN] {
    reply_bytes(
        (4 << 3) | 4,
        2,
        0,
        root_dispersion,
        0,
        origin,
        receive,
        transmit,
    )
}

/// The packet of a reply that the checks accept, for the checks of the tests.
fn good_packet(origin: NtpTime) -> Packet {
    parse(&good_reply(origin, at(NTP_NOW), at(NTP_NOW), 256)).expect("a complete reply parses")
}

#[test]
fn the_request_is_mode_3_version_4_with_its_transmit_timestamp() {
    let transmit = NtpTime::new(0x1122_3344, 0x5566_7788);
    let bytes = request(transmit);
    assert_eq!(bytes.len(), PACKET_LEN);
    assert_eq!(
        bytes.first().copied(),
        Some(0x23),
        "leap 0, version 4, mode 3"
    );
    assert_eq!(
        bytes.get(40..48),
        Some(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88][..])
    );
    assert_eq!(PORT, 123, "the UDP port of NTP (8-1 §6.4.3.2)");
}

#[test]
fn a_reply_is_parsed_field_by_field() {
    let bytes = reply_bytes(
        0x24,
        3,
        0x0001_0000,
        0x0000_0100,
        0x4C4F_434C,
        at(10),
        at(20),
        NtpTime::new(30, 0x8000_0000),
    );
    let packet = parse(&bytes).expect("a complete reply parses");
    assert_eq!(packet.leap, 0);
    assert_eq!(packet.version, 4);
    assert_eq!(packet.mode, 4);
    assert_eq!(packet.stratum, 3);
    assert_eq!(packet.root_delay, 0x0001_0000);
    assert_eq!(packet.root_dispersion, 0x0000_0100);
    assert_eq!(packet.reference_id, 0x4C4F_434C);
    assert_eq!(packet.origin, at(10));
    assert_eq!(packet.receive, at(20));
    assert_eq!(packet.transmit, NtpTime::new(30, 0x8000_0000));
}

#[test]
fn a_datagram_shorter_than_an_sntp_packet_is_refused() {
    let bytes = [0x24_u8; 47];
    assert_eq!(parse(&bytes), Err(Rejection::TooShort { length: 47 }));
    assert_eq!(parse(&[]), Err(Rejection::TooShort { length: 0 }));
}

#[test]
fn the_offset_and_the_delay_follow_the_rfc_formulas() {
    // T1 = 100 s (client sent), T2 = 107 s (server received), T3 = 108 s (server sent), T4 = 103 s (client received).
    // theta = ((107 - 100) + (108 - 103)) / 2 = 6 s; delta = (103 - 100) - (108 - 107) = 2 s.
    let units = |seconds: i128| seconds * (1_i128 << 32);
    let exchange = exchange(at(100), at(107), at(108), at(103));
    assert_eq!(exchange.offset, units(6));
    assert_eq!(exchange.delay, units(2));
}

#[test]
fn a_negative_delay_from_a_clock_error_is_clamped_to_zero() {
    // The server's processing (T3 - T2) is longer than the local round trip: the delay is not negative.
    let exchange = exchange(at(100), at(101), at(120), at(102));
    assert_eq!(exchange.delay, 0);
}

#[test]
fn the_root_distance_is_the_dispersion_plus_half_the_delay_rounded_up() {
    let mut packet = good_packet(at(1));
    packet.root_delay = 3;
    packet.root_dispersion = 10;
    // LAMBDA = EPSILON + DELTA / 2, in units of 2^-16 s: 10 + ceil(3 / 2) = 12.
    assert_eq!(root_distance(&packet), 12);
}

#[test]
fn the_accuracy_is_the_bits_of_the_fraction_the_distance_covers() {
    // A distance is in units of 2^-16 s; an IEC fraction is in units of 2^-24 s (256 per unit).
    assert_eq!(accuracy_of(0).accuracy, TimeAccuracy::Bits(0));
    assert_eq!(
        accuracy_of(1).accuracy,
        TimeAccuracy::Bits(8),
        "2^-16 s is 256 units: 2^8"
    );
    assert_eq!(
        accuracy_of(2).accuracy,
        TimeAccuracy::Bits(9),
        "512 units is exactly 2^9"
    );
    assert_eq!(
        accuracy_of(3).accuracy,
        TimeAccuracy::Bits(10),
        "768 units rounds up to 2^10"
    );
    assert_eq!(
        accuracy_of(256).accuracy,
        TimeAccuracy::Bits(16),
        "2^-8 s is 2^16 units"
    );
    assert_eq!(
        accuracy_of(257).accuracy,
        TimeAccuracy::Bits(17),
        "65 792 units rounds up to 2^17"
    );
    assert_eq!(
        accuracy_of(65_536).accuracy,
        TimeAccuracy::Bits(24),
        "one second is the whole fraction"
    );
    let beyond = accuracy_of(65_537);
    assert!(
        beyond.clock_not_synchronized,
        "more than one second does not fit in the fraction"
    );
    assert_eq!(beyond.accuracy, TimeAccuracy::Unspecified);
}

#[test]
fn a_reply_that_is_not_for_the_request_is_refused() {
    let sent = at(500);
    let good = good_packet(sent);
    assert_eq!(check_reply(sent, &good), Ok(()));

    let mut packet = good;
    packet.mode = 3;
    assert_eq!(
        check_reply(sent, &packet),
        Err(Rejection::NotServer { mode: 3 })
    );

    let mut packet = good;
    packet.version = 2;
    assert_eq!(
        check_reply(sent, &packet),
        Err(Rejection::UnsupportedVersion { version: 2 })
    );

    assert_eq!(
        check_reply(at(501), &good),
        Err(Rejection::OriginMismatch),
        "another request's origin"
    );

    let mut packet = good;
    packet.transmit = NtpTime::ZERO;
    assert_eq!(check_reply(sent, &packet), Err(Rejection::ZeroTransmit));

    let mut packet = good;
    packet.leap = 3;
    assert_eq!(check_reply(sent, &packet), Err(Rejection::Unsynchronised));

    let mut packet = good;
    packet.stratum = 0;
    packet.reference_id = 0x5249_4E47;
    packet.leap = 3;
    assert_eq!(
        check_reply(sent, &packet),
        Err(Rejection::KissOfDeath { code: 0x5249_4E47 }),
        "a kiss code is reported, not the leap"
    );

    let mut packet = good;
    packet.stratum = 16;
    assert_eq!(
        check_reply(sent, &packet),
        Err(Rejection::InvalidStratum { stratum: 16 })
    );
}

#[test]
fn the_clock_is_not_synchronised_before_a_reply_is_accepted() {
    let mut client = Client::new();
    let stamp = client.stamp(Duration::from_secs(5), at(NTP_NOW));
    assert!(stamp.quality.clock_not_synchronized);
    assert_eq!(stamp.quality.accuracy, TimeAccuracy::Unspecified);
    assert!(!client.is_synchronised(Duration::from_secs(5)));
    assert_eq!(
        client.receive(Duration::from_secs(5), at(NTP_NOW), &[0; PACKET_LEN]),
        Err(Rejection::NoRequest)
    );
}

#[test]
fn the_client_sends_one_request_per_poll_interval() {
    let mut client = Client::new();
    let first = client.poll(Duration::from_secs(0), at(NTP_NOW));
    assert_eq!(first, Some(request(at(NTP_NOW))));
    assert_eq!(
        client.poll(Duration::from_secs(1), at(NTP_NOW + 1)),
        None,
        "not before the interval"
    );
    assert!(
        client.poll(POLL_INTERVAL, at(NTP_NOW + 64)).is_some(),
        "due once the interval has elapsed"
    );
}

#[test]
fn an_accepted_reply_gives_the_server_time_and_its_accuracy() {
    let mut client = Client::new();
    let local = at(NTP_NOW);
    let sent = client
        .poll(Duration::ZERO, local)
        .expect("the first request is due");
    assert_eq!(sent.get(40..48).map(|b| b.len()), Some(8));

    // The server is 2 s ahead of the local clock, and its root dispersion is 1/256 s (256 units of 2^-16 s).
    let server_now = at(NTP_NOW + 2);
    let bytes = good_reply(local, server_now, server_now, 256);
    client
        .receive(Duration::from_millis(40), local, &bytes)
        .expect("the reply is accepted");

    let stamp = client.stamp(Duration::from_millis(50), local);
    assert_eq!(
        stamp.seconds,
        UNIX_NOW + 2,
        "the local time is corrected by the offset of the reply"
    );
    assert!(!stamp.quality.clock_not_synchronized);
    assert_eq!(
        stamp.quality.accuracy,
        TimeAccuracy::Bits(16),
        "a distance of 2^-8 s is 2^16 units"
    );
    assert!(client.is_synchronised(Duration::from_millis(50)));
}

#[test]
fn a_second_reply_to_the_same_request_is_refused() {
    let mut client = Client::new();
    let local = at(NTP_NOW);
    client.poll(Duration::ZERO, local);
    let bytes = good_reply(local, local, local, 256);
    assert_eq!(
        client.receive(Duration::from_millis(1), local, &bytes),
        Ok(())
    );
    assert_eq!(
        client.receive(Duration::from_millis(2), local, &bytes),
        Err(Rejection::NoRequest)
    );
}

#[test]
fn a_refused_reply_leaves_the_estimate_and_the_pending_request() {
    let mut client = Client::new();
    let local = at(NTP_NOW);
    client.poll(Duration::ZERO, local);
    let foreign = good_reply(at(7), local, local, 256);
    assert_eq!(
        client.receive(Duration::from_millis(1), local, &foreign),
        Err(Rejection::OriginMismatch)
    );
    assert!(
        !client.is_synchronised(Duration::from_millis(1)),
        "no estimate from a refused reply"
    );
    let good = good_reply(local, local, local, 256);
    assert_eq!(
        client.receive(Duration::from_millis(2), local, &good),
        Ok(()),
        "the request is still pending"
    );
}

#[test]
fn the_estimate_ages_out_after_max_age() {
    let mut client = Client::new();
    let local = at(NTP_NOW);
    client.poll(Duration::ZERO, local);
    client
        .receive(
            Duration::from_secs(1),
            local,
            &good_reply(local, local, local, 256),
        )
        .expect("accepted");

    let fresh = Duration::from_secs(1) + MAX_AGE;
    assert!(client.is_synchronised(fresh));
    assert!(!client.stamp(fresh, local).quality.clock_not_synchronized);

    let stale = fresh + Duration::from_millis(1);
    let stamp = client.stamp(stale, local);
    assert!(
        stamp.quality.clock_not_synchronized,
        "an estimate older than MAX_AGE is not extrapolated"
    );
    assert_eq!(stamp.seconds, UNIX_NOW);
}

#[test]
fn a_reply_with_a_root_distance_above_one_second_is_not_synchronised() {
    let mut client = Client::new();
    let local = at(NTP_NOW);
    client.poll(Duration::ZERO, local);
    client
        .receive(
            Duration::from_secs(1),
            local,
            &good_reply(local, local, local, 65_537),
        )
        .expect("accepted");
    let stamp = client.stamp(Duration::from_secs(1), local);
    assert!(stamp.quality.clock_not_synchronized);
    assert_eq!(stamp.quality.accuracy, TimeAccuracy::Unspecified);
}

#[test]
fn a_stamp_before_1970_is_at_the_epoch_and_not_synchronised() {
    let client = Client::new();
    let stamp: UtcTimestamp = client.stamp(Duration::ZERO, at(1_000));
    assert_eq!(stamp.seconds, 0);
    assert!(stamp.quality.clock_not_synchronized);
}
