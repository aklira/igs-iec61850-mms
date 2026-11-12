// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The host clock (task A5; TIME-001, TIME-002): the quality it reports is the one declared for the host, and a host
//! whose state is not declared reports that it is not synchronised.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use igs_iec61850_mms::clock::HostClock;
use igs_iec61850_mms_core::time::{ClockError, ClockSource};
use igs_iso9506_mms_codec::mms::{TimeAccuracy, TimeQuality, MAX_FRACTION};

#[test]
fn a_host_whose_state_is_not_declared_is_not_synchronised() {
    let quality = HostClock::default().quality();
    assert!(quality.clock_not_synchronized);
    assert!(!quality.clock_failure);
    assert_eq!(quality.accuracy, TimeAccuracy::Unspecified);
}

#[test]
fn a_declared_synchronisation_reports_its_accuracy_and_leap_second_knowledge() {
    let clock =
        HostClock::synchronised(true, TimeAccuracy::Bits(16)).expect("the quality is valid");
    assert_eq!(
        clock.quality(),
        TimeQuality {
            leap_second_known: true,
            clock_failure: false,
            clock_not_synchronized: false,
            accuracy: TimeAccuracy::Bits(16),
        }
    );
}

#[test]
fn an_accuracy_a_timestamp_cannot_carry_is_refused_at_the_clock() {
    assert_eq!(
        HostClock::synchronised(false, TimeAccuracy::Bits(31)).err(),
        Some(ClockError::InvalidAccuracy(31))
    );
}

#[test]
fn the_host_time_is_stamped_with_the_declared_quality() {
    let clock =
        HostClock::synchronised(false, TimeAccuracy::Bits(24)).expect("the quality is valid");
    let stamp = clock.now();
    // After 2020-09-13 (1 600 000 000 s), the time of the host.
    assert!(stamp.seconds > 1_600_000_000);
    assert!(stamp.fraction <= MAX_FRACTION);
    assert_eq!(stamp.quality, clock.quality());
}
