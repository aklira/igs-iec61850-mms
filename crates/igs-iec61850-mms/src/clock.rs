// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The host clock (TIME-001, TIME-002; task A5).
//!
//! The host clock reads the system time, and it reports the quality that the operator declares for the host (PIXIT):
//! whether the host is synchronised by a service such as chrony, the accuracy of that service, and the leap-second
//! knowledge. The declared quality is a statement of the operator, not a measurement: the crate has no way to check the
//! synchronisation of a host without unsafe code or a platform interface. The default is not synchronised, so a timestamp
//! from a host whose state was not declared says so.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use igs_iec61850_mms_core::time::{check_quality, ClockError, ClockSource};
use igs_iso9506_mms_codec::mms::{TimeAccuracy, TimeQuality, UtcTimestamp, MAX_FRACTION};

/// A clock of the host, with the quality the operator declares for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostClock {
    quality: TimeQuality,
}

impl HostClock {
    /// A host clock with this quality. A quality that a timestamp cannot carry is refused.
    pub fn new(quality: TimeQuality) -> Result<Self, ClockError> {
        check_quality(&quality)?;
        Ok(Self { quality })
    }

    /// A host clock that is synchronised, with the accuracy declared for its synchronisation.
    pub fn synchronised(
        leap_second_known: bool,
        accuracy: TimeAccuracy,
    ) -> Result<Self, ClockError> {
        Self::new(TimeQuality {
            leap_second_known,
            clock_failure: false,
            clock_not_synchronized: false,
            accuracy,
        })
    }

    /// The quality this clock reports.
    pub fn quality(&self) -> TimeQuality {
        self.quality
    }
}

impl Default for HostClock {
    /// A host clock that is not synchronised, with no declared accuracy: the state of a host that nothing declared.
    fn default() -> Self {
        Self {
            quality: TimeQuality {
                leap_second_known: false,
                clock_failure: false,
                clock_not_synchronized: true,
                accuracy: TimeAccuracy::Unspecified,
            },
        }
    }
}

impl ClockSource for HostClock {
    fn now(&self) -> UtcTimestamp {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        let seconds = u32::try_from(since_epoch.as_secs()).unwrap_or(u32::MAX);
        UtcTimestamp {
            seconds,
            fraction: fraction_of(since_epoch.subsec_nanos()),
            quality: self.quality,
        }
    }
}

/// The fraction of a second, in units of 2^-24 s, of a count of nanoseconds (rounded down).
fn fraction_of(nanos: u32) -> u32 {
    const UNITS: u64 = 16_777_216; // 2^24: the units of the fraction
    const NANOS_PER_SECOND: u64 = 1_000_000_000;
    let scaled = u64::from(nanos).saturating_mul(UNITS) / NANOS_PER_SECOND;
    u32::try_from(scaled)
        .unwrap_or(MAX_FRACTION)
        .min(MAX_FRACTION)
}
