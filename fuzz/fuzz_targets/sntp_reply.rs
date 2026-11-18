// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use std::time::Duration;

use igs_iec61850_mms_core::sntp::{parse, Client, NtpTime};
use igs_iso9506_mms_codec::mms::MAX_FRACTION;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The first 16 octets give the local UTC time, the monotonic instant of the poll, and the instant of the reply; the rest is the
    // datagram that the server sent. Every input answers with a value or a refusal, never a panic.
    if data.len() < 16 {
        return;
    }
    let (head, datagram) = data.split_at(16);
    let word = |at: usize| -> u32 {
        u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]])
    };
    let wall = NtpTime::new(word(0), word(4));
    let poll = Duration::new(u64::from(word(8)), word(12) % 1_000_000_000);
    let reply = poll.saturating_add(Duration::from_secs(u64::from(word(0) % 4096)));

    let _ = parse(datagram);
    let mut client = Client::new();
    let _ = client.poll(poll, wall);
    let _ = client.receive(reply, wall, datagram);

    // The clock that a reply leaves: a stamp is never out of its range, and it is not synchronised unless the estimate is fresh.
    let stamp = client.stamp(reply, wall);
    assert!(stamp.fraction <= MAX_FRACTION);
    if !client.is_synchronised(reply) {
        assert!(stamp.quality.clock_not_synchronized);
    }
});
