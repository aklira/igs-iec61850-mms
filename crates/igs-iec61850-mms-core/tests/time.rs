// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The timestamps and their quality (task A5; TIME-001, MMS-003, AT-17): each flag of the quality is at its bit,
//! the accuracy is bounded, and a clock that loses and regains its synchronisation stamps what it reports.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use igs_iec61850_mms_core::acsi::{timestamp_from_data, timestamp_to_data};
use igs_iec61850_mms_core::time::{check_quality, ClockError, ClockSource, FixedClock};
use igs_iso9506_mms_codec::mms::{TimeAccuracy, TimeQuality, UtcTimestamp, MAX_FRACTION};

fn quality(
    leap: bool,
    failure: bool,
    not_synchronised: bool,
    accuracy: TimeAccuracy,
) -> TimeQuality {
    TimeQuality {
        leap_second_known: leap,
        clock_failure: failure,
        clock_not_synchronized: not_synchronised,
        accuracy,
    }
}

#[test]
fn each_quality_flag_is_at_its_bit_of_the_octet() {
    // Leap-second knowledge is bit 7, clock failure bit 6, not synchronised bit 5, and the accuracy the low five bits.
    let synchronised = quality(false, false, false, TimeAccuracy::Bits(0));
    assert_eq!(synchronised.to_octet(), 0x00);
    assert_eq!(
        quality(true, false, false, TimeAccuracy::Bits(0)).to_octet(),
        0x80
    );
    assert_eq!(
        quality(false, true, false, TimeAccuracy::Bits(0)).to_octet(),
        0x40
    );
    assert_eq!(
        quality(false, false, true, TimeAccuracy::Bits(0)).to_octet(),
        0x20
    );
    assert_eq!(
        quality(false, false, false, TimeAccuracy::Bits(10)).to_octet(),
        0x0A
    );
    assert_eq!(
        quality(false, false, false, TimeAccuracy::Unspecified).to_octet(),
        0x1F
    );
}

#[test]
fn a_timestamp_round_trips_with_each_quality_flag() {
    for leap in [false, true] {
        for failure in [false, true] {
            for not_synchronised in [false, true] {
                for accuracy in [
                    TimeAccuracy::Bits(24),
                    TimeAccuracy::Bits(0),
                    TimeAccuracy::Unspecified,
                ] {
                    let stamp = UtcTimestamp::new(
                        1_700_000_000,
                        MAX_FRACTION,
                        quality(leap, failure, not_synchronised, accuracy),
                    )
                    .expect("the timestamp is valid");
                    let back = timestamp_from_data(&timestamp_to_data(stamp))
                        .expect("the timestamp reads back");
                    assert_eq!(back, stamp);
                }
            }
        }
    }
}

#[test]
fn the_accuracy_is_bounded_to_24_bits_or_unspecified() {
    let at = |accuracy| quality(false, false, false, accuracy);
    assert!(check_quality(&at(TimeAccuracy::Bits(24))).is_ok());
    assert!(check_quality(&at(TimeAccuracy::Unspecified)).is_ok());
    assert_eq!(
        check_quality(&at(TimeAccuracy::Bits(25))),
        Err(ClockError::InvalidAccuracy(25))
    );
    // The octet of the timestamp refuses 25 to 30 as well: a reader gets an error, not a stamp.
    assert!(TimeQuality::from_octet(25).is_err());
    assert!(TimeQuality::from_octet(30).is_err());
    // The fraction is at most 24 bits.
    assert!(UtcTimestamp::new(0, MAX_FRACTION + 1, at(TimeAccuracy::Unspecified)).is_err());
}

#[test]
fn a_clock_refuses_a_quality_it_cannot_carry() {
    let clock = FixedClock::new(
        UtcTimestamp::new(100, 0, quality(false, false, false, TimeAccuracy::Bits(3))).unwrap(),
    );
    let invalid = UtcTimestamp {
        seconds: 101,
        fraction: 0,
        quality: quality(false, false, false, TimeAccuracy::Bits(25)),
    };
    assert_eq!(clock.set(invalid), Err(ClockError::InvalidAccuracy(25)));
    assert_eq!(clock.now().seconds, 100, "the reading stays as it was");
}

#[test]
fn a_clock_that_loses_and_regains_synchronisation_stamps_its_state() {
    let synchronised = quality(false, false, false, TimeAccuracy::Bits(20));
    let lost = quality(false, false, true, TimeAccuracy::Unspecified);
    let clock = FixedClock::new(UtcTimestamp::new(100, 0, synchronised).unwrap());
    assert_eq!(clock.now().quality, synchronised);

    clock.set(UtcTimestamp::new(101, 0, lost).unwrap()).unwrap();
    assert_eq!(clock.now().quality, lost, "the loss is in the next stamp");

    clock
        .set(UtcTimestamp::new(102, 0, synchronised).unwrap())
        .unwrap();
    assert_eq!(
        clock.now().quality,
        synchronised,
        "the recovery is in the next stamp"
    );
    assert_eq!(clock.now().seconds, 102);
}
