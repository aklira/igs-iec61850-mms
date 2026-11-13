// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The embedded SNTP client (IEC 61850-8-1 §6.4.2.2 and §6.4.3.2; the SNTP subset of RFC 5905; TIME-002 and TIME-003;
//! task O5; D-052).
//!
//! The client is sans-I/O: the caller gives the monotonic instant of each event (durations, RUN-003) and the local UTC
//! time (the origin of a request and the arrival of a reply), and the client returns the request to send. The UDP
//! socket is in the async crate. The client keeps the last reply that passed the checks: the clock follows it, and its
//! quality is the root distance of that reply, as the standard asks for TimeQuality.TimeAccuracy. A reply that is not
//! checked, or an estimate older than [`MAX_AGE`], gives a time that is not synchronised.

use std::fmt;
use std::time::Duration;

use igs_iso9506_mms_codec::mms::{TimeAccuracy, TimeQuality, UtcTimestamp};

/// The UDP port of NTP (8-1 §6.4.3.2).
pub const PORT: u16 = 123;

/// The length of an SNTP packet without extension fields (RFC 5905 §7.3).
pub const PACKET_LEN: usize = 48;

/// The interval between two requests of the client. A request is sent at each poll, not more often.
pub const POLL_INTERVAL: Duration = Duration::from_secs(64);

/// The age after which the last reply no longer keeps the clock synchronised. The estimate is not extrapolated: a clock
/// whose last reply is older than this is reported as not synchronised.
pub const MAX_AGE: Duration = Duration::from_secs(1024);

/// Seconds from 1900-01-01 (the epoch of NTP) to 1970-01-01 (the epoch of IEC 61850 timestamps).
const NTP_UNIX_OFFSET: u32 = 2_208_988_800;

/// The NTP mode of a client request (RFC 5905 §7.3).
const MODE_CLIENT: u8 = 3;

/// The NTP mode of a server reply (RFC 5905 §7.3).
const MODE_SERVER: u8 = 4;

/// The version of the requests: the current version of RFC 5905.
const VERSION_REQUEST: u8 = 4;

/// The leap indicator of an unsynchronised server (RFC 5905 §7.3).
const LEAP_UNSYNCHRONISED: u8 = 3;

/// The highest stratum of a synchronised server; stratum 0 is a kiss-o'-death and 16 is unsynchronised (RFC 5905 §7.3).
const MAX_STRATUM: u8 = 15;

/// The fraction of an NTP timestamp, in units of 2^-32 s, has 32 bits.
const FRACTION_BITS: u32 = 32;

/// The units of the fraction of an IEC 61850 timestamp, in bits: 2^-24 s (7-2 TimeStamp).
const IEC_FRACTION_SHIFT: u32 = 8;

/// The time quality of a clock that the client cannot qualify.
const NOT_SYNCHRONISED: TimeQuality = TimeQuality {
    leap_second_known: false,
    clock_failure: false,
    clock_not_synchronized: true,
    accuracy: TimeAccuracy::Unspecified,
};

/// An NTP timestamp: seconds since 1900-01-01 and a fraction in units of 2^-32 s (RFC 5905 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct NtpTime {
    /// Seconds since 1900-01-01 00:00:00 UTC.
    pub seconds: u32,
    /// Fraction of the second, in units of 2^-32 s.
    pub fraction: u32,
}

impl NtpTime {
    /// The zero timestamp: the transmit timestamp that RFC 5905 forbids in a reply.
    pub const ZERO: Self = Self {
        seconds: 0,
        fraction: 0,
    };

    /// A timestamp from its seconds and its fraction.
    pub const fn new(seconds: u32, fraction: u32) -> Self {
        Self { seconds, fraction }
    }

    /// The timestamp as one integer of 2^-32 s since 1900.
    pub fn units(self) -> u64 {
        (u64::from(self.seconds) << FRACTION_BITS) | u64::from(self.fraction)
    }

    /// The timestamp of an integer of 2^-32 s since 1900. A value beyond the 32 bits of the seconds is saturated.
    pub fn from_units(units: u64) -> Self {
        let seconds = u32::try_from(units >> FRACTION_BITS).unwrap_or(u32::MAX);
        let fraction = (units & u64::from(u32::MAX)) as u32;
        Self::new(seconds, fraction)
    }

    /// The timestamp of a Unix time (seconds since 1970 and nanoseconds). `None` when it is out of the range of the
    /// NTP seconds, or when the nanoseconds are not a second.
    pub fn from_unix(seconds: u64, nanos: u32) -> Option<Self> {
        let seconds = u32::try_from(seconds.checked_add(u64::from(NTP_UNIX_OFFSET))?).ok()?;
        let fraction = (u64::from(nanos) << FRACTION_BITS) / 1_000_000_000;
        Some(Self::new(seconds, u32::try_from(fraction).ok()?))
    }
}

/// A reply of a server, as the fields of RFC 5905 §7.3 that the client uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    /// The leap indicator (two bits).
    pub leap: u8,
    /// The version (three bits).
    pub version: u8,
    /// The mode (three bits).
    pub mode: u8,
    /// The stratum: 0 for a kiss-o'-death, 1 to 15 for a synchronised server, 16 for an unsynchronised one.
    pub stratum: u8,
    /// The root delay, in the NTP short format (units of 2^-16 s).
    pub root_delay: u32,
    /// The root dispersion, in the NTP short format (units of 2^-16 s).
    pub root_dispersion: u32,
    /// The reference identifier: the kiss code of a kiss-o'-death.
    pub reference_id: u32,
    /// The origin timestamp: the transmit timestamp of the request that the reply answers.
    pub origin: NtpTime,
    /// The receive timestamp: when the server received the request.
    pub receive: NtpTime,
    /// The transmit timestamp: when the server sent the reply.
    pub transmit: NtpTime,
}

/// Why a datagram is not a reply that the client uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rejection {
    /// The datagram is shorter than an SNTP packet.
    TooShort {
        /// The length of the datagram.
        length: usize,
    },
    /// The mode is not that of a server (4).
    NotServer {
        /// The mode of the datagram.
        mode: u8,
    },
    /// The version is not one that SNTP clients accept (3 or 4).
    UnsupportedVersion {
        /// The version of the datagram.
        version: u8,
    },
    /// The origin timestamp is not the transmit timestamp of the pending request: an old or foreign reply.
    OriginMismatch,
    /// The transmit timestamp of the reply is zero.
    ZeroTransmit,
    /// The server reports that it is not synchronised (leap indicator 3).
    Unsynchronised,
    /// A kiss-o'-death (stratum 0): the server asks the client to stop; its kiss code is kept.
    KissOfDeath {
        /// The kiss code, the reference identifier of the packet.
        code: u32,
    },
    /// The stratum is 16 or more.
    InvalidStratum {
        /// The stratum of the reply.
        stratum: u8,
    },
    /// No request is pending: the reply is unexpected, or it is a second reply to the same request.
    NoRequest,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { length } => write!(
                f,
                "the SNTP packet has {length} octets, at least 48 are needed"
            ),
            Self::NotServer { mode } => write!(
                f,
                "the SNTP reply has mode {mode}, a server replies with mode 4"
            ),
            Self::UnsupportedVersion { version } => {
                write!(f, "the SNTP version {version} is not accepted")
            }
            Self::OriginMismatch => {
                write!(f, "the origin timestamp is not that of the pending request")
            }
            Self::ZeroTransmit => write!(f, "the transmit timestamp of the SNTP reply is zero"),
            Self::Unsynchronised => {
                write!(f, "the SNTP server is not synchronised (leap indicator 3)")
            }
            Self::KissOfDeath { code } => {
                write!(f, "the SNTP server sent a kiss-o'-death, code {code:#010x}")
            }
            Self::InvalidStratum { stratum } => {
                write!(f, "the SNTP stratum {stratum} is not synchronised")
            }
            Self::NoRequest => write!(f, "no SNTP request is pending"),
        }
    }
}

impl std::error::Error for Rejection {}

/// The request of a client: mode 3, version 4, and the transmit timestamp (the origin of the reply, RFC 5905 §8).
pub fn request(transmit: NtpTime) -> [u8; PACKET_LEN] {
    let first = (VERSION_REQUEST << 3) | MODE_CLIENT;
    let [s0, s1, s2, s3] = transmit.seconds.to_be_bytes();
    let [f0, f1, f2, f3] = transmit.fraction.to_be_bytes();
    [
        first, 0, 0, 0, 0, 0, 0, 0, // octets 0 to 7: leap, version, mode, then zeros
        0, 0, 0, 0, 0, 0, 0,
        0, // octets 8 to 15: root delay and dispersion, zero in a request
        0, 0, 0, 0, 0, 0, 0, 0, // octets 16 to 23: reference and originate, zero
        0, 0, 0, 0, 0, 0, 0, 0, // octets 24 to 31: originate and receive, zero
        0, 0, 0, 0, 0, 0, 0, 0, // octets 32 to 39: receive and transmit, zero
        s0, s1, s2, s3, f0, f1, f2, f3, // octets 40 to 47: the transmit timestamp
    ]
}

/// Reads a big-endian word of the datagram at `at`.
fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let slice = bytes.get(at..end)?;
    let array: [u8; 4] = slice.try_into().ok()?;
    Some(u32::from_be_bytes(array))
}

/// Reads the timestamp whose seconds are at `at` and whose fraction follows them.
fn timestamp(bytes: &[u8], at: usize) -> Option<NtpTime> {
    let seconds = word(bytes, at)?;
    let fraction = word(bytes, at.checked_add(4)?)?;
    Some(NtpTime::new(seconds, fraction))
}

/// Parses an SNTP datagram (RFC 5905 §7.3). The extension fields and the MAC, if any, are not read.
pub fn parse(bytes: &[u8]) -> Result<Packet, Rejection> {
    let too_short = Rejection::TooShort {
        length: bytes.len(),
    };
    if bytes.len() < PACKET_LEN {
        return Err(too_short);
    }
    let first = *bytes.first().ok_or(too_short)?;
    let stratum = *bytes.get(1).ok_or(too_short)?;
    let missing = || too_short;
    Ok(Packet {
        leap: first >> 6,
        version: (first >> 3) & 0x07,
        mode: first & 0x07,
        stratum,
        root_delay: word(bytes, 4).ok_or_else(missing)?,
        root_dispersion: word(bytes, 8).ok_or_else(missing)?,
        reference_id: word(bytes, 12).ok_or_else(missing)?,
        origin: timestamp(bytes, 24).ok_or_else(missing)?,
        receive: timestamp(bytes, 32).ok_or_else(missing)?,
        transmit: timestamp(bytes, 40).ok_or_else(missing)?,
    })
}

/// Checks a parsed reply against the request it answers: the origin is the transmit timestamp of the request, the
/// transmit timestamp is not zero, the server is synchronised, and it is a server reply with a stratum of 1 to 15.
/// A kiss-o'-death is reported as such (its stratum is 0, and its leap indicator is 3, so it is checked first).
pub fn check_reply(sent: NtpTime, packet: &Packet) -> Result<(), Rejection> {
    if packet.mode != MODE_SERVER {
        return Err(Rejection::NotServer { mode: packet.mode });
    }
    if packet.version != 3 && packet.version != VERSION_REQUEST {
        return Err(Rejection::UnsupportedVersion {
            version: packet.version,
        });
    }
    if packet.origin != sent {
        return Err(Rejection::OriginMismatch);
    }
    if packet.transmit == NtpTime::ZERO {
        return Err(Rejection::ZeroTransmit);
    }
    if packet.stratum == 0 {
        return Err(Rejection::KissOfDeath {
            code: packet.reference_id,
        });
    }
    if packet.leap == LEAP_UNSYNCHRONISED {
        return Err(Rejection::Unsynchronised);
    }
    if packet.stratum > MAX_STRATUM {
        return Err(Rejection::InvalidStratum {
            stratum: packet.stratum,
        });
    }
    Ok(())
}

/// The offset and the delay of one exchange, in units of 2^-32 s (RFC 5905 §8: θ and δ). The offset is the amount to add
/// to the local clock to read the server's clock; the delay is the round trip without the server's processing time,
/// clamped to zero when the clocks make it negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exchange {
    /// The offset θ = ((T2 − T1) + (T3 − T4)) / 2.
    pub offset: i128,
    /// The delay δ = (T4 − T1) − (T3 − T2), not negative.
    pub delay: i128,
}

/// The offset and the delay of an exchange from its four timestamps: T1 the request's transmit (local), T2 the server's
/// receive, T3 the server's transmit, and T4 the local arrival of the reply.
pub fn exchange(t1: NtpTime, t2: NtpTime, t3: NtpTime, t4: NtpTime) -> Exchange {
    let t1 = i128::from(t1.units());
    let t2 = i128::from(t2.units());
    let t3 = i128::from(t3.units());
    let t4 = i128::from(t4.units());
    let offset = t2.saturating_sub(t1).saturating_add(t3.saturating_sub(t4)) / 2;
    let delay = t4.saturating_sub(t1).saturating_sub(t3.saturating_sub(t2));
    Exchange {
        offset,
        delay: delay.max(0),
    }
}

/// The root distance of a reply (RFC 5905 §8, LAMBDA = EPSILON + DELTA/2): the root dispersion plus half the root delay, both
/// as the reply carries them, in units of 2^-16 s. Half the delay is rounded up, so the bound is not reduced.
pub fn root_distance(packet: &Packet) -> u64 {
    let half_delay = u64::from(packet.root_delay).saturating_add(1) / 2;
    u64::from(packet.root_dispersion).saturating_add(half_delay)
}

/// The time quality that a root distance gives (TIME-003; 8-1 §6.4.2.2): the accuracy is the number of bits of the
/// 2^-24 s fraction that the distance covers, rounded up, so that the error stays within it. A distance above 1 s does not
/// fit in the fraction, and the clock is reported as not synchronised.
pub fn accuracy_of(distance: u64) -> TimeQuality {
    // The distance is in 2^-16 s, and the fraction of an IEC 61850 timestamp in 2^-24 s: 256 units of the fraction per unit.
    let units = distance.saturating_mul(1 << IEC_FRACTION_SHIFT);
    let bits = if units <= 1 {
        0
    } else {
        u64::BITS.saturating_sub(units.saturating_sub(1).leading_zeros())
    };
    match u8::try_from(bits) {
        Ok(bits) if bits <= 24 => TimeQuality {
            leap_second_known: true,
            clock_failure: false,
            clock_not_synchronized: false,
            accuracy: TimeAccuracy::Bits(bits),
        },
        _ => NOT_SYNCHRONISED,
    }
}

/// The estimate of the last reply that passed the checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Estimate {
    /// The offset to add to the local clock (units of 2^-32 s).
    offset: i128,
    /// The root distance of the reply (units of 2^-16 s).
    distance: u64,
    /// The monotonic instant of the reply.
    taken_at: Duration,
}

/// The SNTP client: one request at a time to its server, and the estimate of the last accepted reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Client {
    /// The earliest instant of the next request.
    next_poll: Duration,
    /// The origin of the pending request: its transmit timestamp, in the local clock.
    pending: Option<NtpTime>,
    /// The estimate of the last accepted reply.
    estimate: Option<Estimate>,
}

impl Client {
    /// A client that has no reply yet; its first request is due at once.
    pub fn new() -> Self {
        Self::default()
    }

    /// The request to send at the monotonic instant `now`, with the local UTC time `wall` as its transmit timestamp.
    /// `None` until the poll interval has elapsed since the previous request.
    pub fn poll(&mut self, now: Duration, wall: NtpTime) -> Option<[u8; PACKET_LEN]> {
        if now < self.next_poll {
            return None;
        }
        self.next_poll = now.saturating_add(POLL_INTERVAL);
        self.pending = Some(wall);
        Some(request(wall))
    }

    /// Handles a datagram that arrived at the monotonic instant `now`, when the local UTC time was `wall`. A reply that
    /// the checks refuse leaves the estimate as it was. An accepted reply replaces it and answers the pending request.
    pub fn receive(&mut self, now: Duration, wall: NtpTime, bytes: &[u8]) -> Result<(), Rejection> {
        let sent = self.pending.ok_or(Rejection::NoRequest)?;
        let packet = parse(bytes)?;
        check_reply(sent, &packet)?;
        let exchange = exchange(sent, packet.receive, packet.transmit, wall);
        self.pending = None;
        self.estimate = Some(Estimate {
            offset: exchange.offset,
            distance: root_distance(&packet),
            taken_at: now,
        });
        Ok(())
    }

    /// Whether the clock is synchronised at the monotonic instant `now`: a reply was accepted, and it is not older than
    /// [`MAX_AGE`].
    pub fn is_synchronised(&self, now: Duration) -> bool {
        self.estimate
            .is_some_and(|estimate| now.saturating_sub(estimate.taken_at) <= MAX_AGE)
    }

    /// The time stamped at the monotonic instant `now`, from the local UTC time `wall`, with its quality. When the clock is
    /// synchronised, the time is corrected by the offset of the last reply and its quality is that reply's root distance.
    /// Otherwise the local time is stamped as not synchronised (TIME-001). A time that the timestamp cannot carry (before
    /// 1970, or after 2106) is stamped at the epoch, not synchronised.
    pub fn stamp(&self, now: Duration, wall: NtpTime) -> UtcTimestamp {
        let local = i128::from(wall.units());
        let stamp = match self.estimate {
            Some(estimate) if self.is_synchronised(now) => utc_at(
                local.saturating_add(estimate.offset),
                accuracy_of(estimate.distance),
            ),
            _ => utc_at(local, NOT_SYNCHRONISED),
        };
        stamp.unwrap_or(UtcTimestamp {
            seconds: 0,
            fraction: 0,
            quality: NOT_SYNCHRONISED,
        })
    }
}

/// The IEC 61850 timestamp of an NTP time of `units` (2^-32 s since 1900) with the quality given. `None` when the time is
/// before 1970 or after the range of the seconds.
fn utc_at(units: i128, quality: TimeQuality) -> Option<UtcTimestamp> {
    let units = u64::try_from(units).ok()?;
    let ntp_seconds = u32::try_from(units >> FRACTION_BITS).ok()?;
    let seconds = ntp_seconds.checked_sub(NTP_UNIX_OFFSET)?;
    let fraction = ((units & u64::from(u32::MAX)) as u32) >> IEC_FRACTION_SHIFT;
    Some(UtcTimestamp {
        seconds,
        fraction,
        quality,
    })
}
