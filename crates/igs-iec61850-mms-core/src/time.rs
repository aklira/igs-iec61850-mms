// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Clock sources of the timestamps (IEC 61850-7-2 TimeStamp, IEC 61850-8-1 §8.1.3.7; TIME-001 and TIME-002; task A5).
//!
//! A timestamp is a UTC time together with its quality (leap-second knowledge, clock failure, clock not
//! synchronised, accuracy), so that a value stamped by a clock that lost its synchronisation says so. The library
//! takes its time from a [`ClockSource`] that the application supplies: it stamps nothing with a clock whose state is
//! not reported. The host clock is in the async crate, which owns the system calls.

use std::sync::Mutex;

use std::fmt;

use igs_iso9506_mms_codec::mms::{TimeAccuracy, TimeQuality, UtcTimestamp};

/// Why a clock reading is refused: a quality that the octet of the timestamp cannot carry (8-1 Annex F.3.1; the
/// accuracy is 0 to 24 bits, or unspecified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClockError {
    /// The accuracy is not one of the values of the quality octet.
    InvalidAccuracy(u8),
}

impl fmt::Display for ClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAccuracy(bits) => {
                write!(f, "the accuracy of {bits} bits is not a time quality")
            }
        }
    }
}

impl std::error::Error for ClockError {}

/// Checks that a time quality can be carried by a timestamp: the accuracy is 0 to 24 bits, or unspecified.
pub fn check_quality(quality: &TimeQuality) -> Result<(), ClockError> {
    match quality.accuracy {
        TimeAccuracy::Bits(bits) if bits > 24 => Err(ClockError::InvalidAccuracy(bits)),
        _ => Ok(()),
    }
}

/// A source of UTC time with its quality (TIME-001). Each reading carries the quality of the moment it is taken.
pub trait ClockSource: Send + Sync {
    /// The current time, with its quality.
    fn now(&self) -> UtcTimestamp;
}

/// A clock whose reading its owner sets: a test clock, or a clock whose state the application follows. Setting a
/// reading with a quality that is not synchronised, and then a synchronised one, models the loss of synchronisation
/// and its recovery: the next stamp carries the state of the reading that is current when it is taken.
#[derive(Debug)]
pub struct FixedClock {
    current: Mutex<UtcTimestamp>,
}

impl FixedClock {
    /// A clock whose reading is `time` until it is set again.
    pub fn new(time: UtcTimestamp) -> Self {
        Self {
            current: Mutex::new(time),
        }
    }

    /// Sets the reading: the time, and its quality. A quality that a timestamp cannot carry is refused, and the
    /// reading stays as it was.
    pub fn set(&self, time: UtcTimestamp) -> Result<(), ClockError> {
        check_quality(&time.quality)?;
        match self.current.lock() {
            Ok(mut current) => *current = time,
            Err(poisoned) => *poisoned.into_inner() = time,
        }
        Ok(())
    }
}

impl ClockSource for FixedClock {
    fn now(&self) -> UtcTimestamp {
        match self.current.lock() {
            Ok(current) => *current,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }
}
