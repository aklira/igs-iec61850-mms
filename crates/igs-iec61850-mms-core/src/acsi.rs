// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! ACSI basic and common types and their MMS mapping (IEC 61850-8-1 §8.1, Table 15, §8.1.2, §8.1.3; task A2).
//!
//! Each type maps to an MMS value ([`Data`]) and to an MMS type description ([`TypeDescription`]). The
//! functions read a value and refuse what the type does not allow, with a typed error; they never silently
//! change a value. Two rules of the standard shape the readings:
//!
//! - an enumeration gives a standardised value (zero or more, from the catalogue), an extension (not
//!   standardised, but in the range of the integer) or a private value (negative); an extension is not an
//!   error (§8.1.2.2);
//! - a coded enumeration or a bit string is read with the bits the peer sent: the extra bits past the size of the
//!   type are ignored, and the missing bits take the value zero (§8.1.2.1, first point). The NOTE of the same
//!   clause calls a longer bit string an `object-value-invalid` when written; this module writes the exact size
//!   and never a longer one, and reads as the normative point says (PROVENANCE D-004).
//!
//! The time quality of a timestamp is the one of N3 (`UtcTimestamp`, `TimeQuality`), and the quality
//! of a value follows the bit positions of 8-1 Table 44.

use std::fmt;

use igs_iso9506_mms_codec::mms::{
    Data, DataMMSString, FloatingPoint, Integer32, TimeOfDay, TypeDescription,
    TypeDescriptionFloatingPoint, Unsigned8, UtcTime, UtcTimestamp,
};
use igs_iso9506_mms_codec::schema::BitString;

use crate::catalogue::{Enumeration, Literal};

/// Why an MMS value is refused as an ACSI value, or an ACSI value cannot be mapped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AcsiError {
    /// The MMS value is not of the MMS type of the ACSI type.
    WrongType {
        /// The ACSI type.
        ty: &'static str,
        /// The MMS alternative that was expected.
        expected: &'static str,
    },
    /// An integer outside the range of the type.
    OutOfRange {
        /// The ACSI type.
        ty: &'static str,
        /// The smallest value allowed.
        min: i128,
        /// The largest value allowed.
        max: i128,
        /// The value given.
        value: i128,
    },
    /// A string or an octet string longer than the type allows (characters for strings, octets for octet strings).
    TooLong {
        /// The ACSI type.
        ty: &'static str,
        /// The limit.
        max: usize,
        /// The length given.
        length: usize,
    },
    /// A character the type does not allow.
    InvalidCharacter {
        /// The ACSI type.
        ty: &'static str,
        /// The character.
        character: char,
    },
    /// A bit string of the wrong length or with a bit the type does not allow.
    InvalidBits {
        /// The ACSI type.
        ty: &'static str,
        /// What is wrong.
        why: &'static str,
    },
    /// A value that the standard does not allow.
    InvalidValue {
        /// The ACSI type.
        ty: &'static str,
        /// What is wrong.
        why: &'static str,
    },
}

impl fmt::Display for AcsiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongType { ty, expected } => write!(f, "{ty} is read from {expected}"),
            Self::OutOfRange {
                ty,
                min,
                max,
                value,
            } => write!(f, "{ty} {value} is outside {min}..={max}"),
            Self::TooLong { ty, max, length } => {
                write!(f, "{ty} of {length}, at most {max} allowed")
            }
            Self::InvalidCharacter { ty, character } => {
                write!(f, "{ty} does not allow {character:?}")
            }
            Self::InvalidBits { ty, why } => write!(f, "{ty}: {why}"),
            Self::InvalidValue { ty, why } => write!(f, "{ty}: {why}"),
        }
    }
}

impl std::error::Error for AcsiError {}

// ---------------------------------------------------------------------------------------------------------------
// Basic types (8-1 Table 15)
// ---------------------------------------------------------------------------------------------------------------

/// `BOOLEAN` is an MMS Boolean.
pub fn boolean_to_data(value: bool) -> Data {
    Data::Boolean(value)
}

/// Reads a `BOOLEAN`. Any Boolean is accepted: the standard maps `false` to zero and `true` to any non-zero
/// octet, and the codec has already read the octet.
pub fn boolean_from_data(data: &Data) -> Result<bool, AcsiError> {
    match data {
        Data::Boolean(value) => Ok(*value),
        _ => Err(AcsiError::WrongType {
            ty: "BOOLEAN",
            expected: "boolean",
        }),
    }
}

/// A signed integer type of the ACSI model (`INT8` to `INT64`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedInteger {
    /// `INT8`.
    I8,
    /// `INT16`.
    I16,
    /// `INT32`.
    I32,
    /// `INT64`.
    I64,
}

impl SignedInteger {
    /// The name of the ACSI type.
    pub fn name(self) -> &'static str {
        match self {
            Self::I8 => "INT8",
            Self::I16 => "INT16",
            Self::I32 => "INT32",
            Self::I64 => "INT64",
        }
    }

    /// The size in bits (the MMS integer of the type, Table 15).
    pub fn bits(self) -> u8 {
        match self {
            Self::I8 => 8,
            Self::I16 => 16,
            Self::I32 => 32,
            Self::I64 => 64,
        }
    }

    /// The smallest and the largest value of the type.
    pub fn range(self) -> (i128, i128) {
        match self {
            Self::I8 => (i128::from(i8::MIN), i128::from(i8::MAX)),
            Self::I16 => (i128::from(i16::MIN), i128::from(i16::MAX)),
            Self::I32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
            Self::I64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
        }
    }
}

/// An unsigned integer type of the ACSI model (`INT8U`, `INT16U`, `INT32U`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsignedInteger {
    /// `INT8U`.
    U8,
    /// `INT16U`.
    U16,
    /// `INT32U`.
    U32,
}

impl UnsignedInteger {
    /// The name of the ACSI type.
    pub fn name(self) -> &'static str {
        match self {
            Self::U8 => "INT8U",
            Self::U16 => "INT16U",
            Self::U32 => "INT32U",
        }
    }

    /// The size in bits.
    pub fn bits(self) -> u8 {
        match self {
            Self::U8 => 8,
            Self::U16 => 16,
            Self::U32 => 32,
        }
    }

    /// The largest value of the type.
    pub fn max(self) -> u128 {
        match self {
            Self::U8 => u128::from(u8::MAX),
            Self::U16 => u128::from(u16::MAX),
            Self::U32 => u128::from(u32::MAX),
        }
    }
}

/// A signed integer as an MMS Integer, refused outside the range of its type.
pub fn signed_to_data(kind: SignedInteger, value: i64) -> Result<Data, AcsiError> {
    let (min, max) = kind.range();
    let value = i128::from(value);
    if value < min || value > max {
        return Err(AcsiError::OutOfRange {
            ty: kind.name(),
            min,
            max,
            value,
        });
    }
    Ok(Data::Integer(value))
}

/// Reads a signed integer, refused outside the range of its type.
pub fn signed_from_data(kind: SignedInteger, data: &Data) -> Result<i64, AcsiError> {
    let Data::Integer(value) = data else {
        return Err(AcsiError::WrongType {
            ty: kind.name(),
            expected: "integer",
        });
    };
    let (min, max) = kind.range();
    if *value < min || *value > max {
        return Err(AcsiError::OutOfRange {
            ty: kind.name(),
            min,
            max,
            value: *value,
        });
    }
    // Within the range of an i64, which holds every signed kind of the model.
    i64::try_from(*value).map_err(|_| AcsiError::OutOfRange {
        ty: kind.name(),
        min,
        max,
        value: *value,
    })
}

/// An unsigned integer as an MMS Unsigned, refused above the range of its type.
pub fn unsigned_to_data(kind: UnsignedInteger, value: u64) -> Result<Data, AcsiError> {
    let value = u128::from(value);
    if value > kind.max() {
        return Err(AcsiError::OutOfRange {
            ty: kind.name(),
            min: 0,
            max: i128::try_from(kind.max()).unwrap_or(i128::MAX),
            value: i128::try_from(value).unwrap_or(i128::MAX),
        });
    }
    Ok(Data::Unsigned(value))
}

/// Reads an unsigned integer, refused above the range of its type. An MMS Integer that is negative is not an
/// unsigned value and is refused as out of range.
pub fn unsigned_from_data(kind: UnsignedInteger, data: &Data) -> Result<u64, AcsiError> {
    let value = match data {
        Data::Unsigned(value) => i128::try_from(*value).unwrap_or(i128::MAX),
        Data::Integer(value) => *value,
        _ => {
            return Err(AcsiError::WrongType {
                ty: kind.name(),
                expected: "unsigned",
            })
        }
    };
    let max = i128::try_from(kind.max()).unwrap_or(i128::MAX);
    if value < 0 || value > max {
        return Err(AcsiError::OutOfRange {
            ty: kind.name(),
            min: 0,
            max,
            value,
        });
    }
    u64::try_from(value).map_err(|_| AcsiError::OutOfRange {
        ty: kind.name(),
        min: 0,
        max,
        value,
    })
}

/// `FLOAT32` as an MMS floating-point value of the IEEE 754 single precision form (exponent width 8, four
/// octets after the width octet).
pub fn float32_to_data(value: f32) -> Data {
    Data::FloatingPoint(FloatingPoint::from_f32(value))
}

/// Reads a `FLOAT32`. Only the single precision form is accepted: other widths are a different type.
pub fn float32_from_data(data: &Data) -> Result<f32, AcsiError> {
    let Data::FloatingPoint(value) = data else {
        return Err(AcsiError::WrongType {
            ty: "FLOAT32",
            expected: "floating-point",
        });
    };
    // The width octet, then the sign, the eight exponent bits and the 23 fraction bits: four octets.
    if value.exponent_width().ok() != Some(8) || value.0.len() != 5 {
        return Err(AcsiError::InvalidValue {
            ty: "FLOAT32",
            why: "not the single precision form (exponent width 8, format width 32)",
        });
    }
    value.to_f32().map_err(|_| AcsiError::InvalidValue {
        ty: "FLOAT32",
        why: "the octets are not a floating-point value",
    })
}

/// Checks the characters of an ISO 646 string (printable ASCII and space, 8-1 §8.1.2.5).
fn check_iso646(ty: &'static str, text: &str) -> Result<(), AcsiError> {
    for character in text.chars() {
        if !(' '..='~').contains(&character) {
            return Err(AcsiError::InvalidCharacter { ty, character });
        }
    }
    Ok(())
}

fn check_chars(ty: &'static str, text: &str, max: usize) -> Result<(), AcsiError> {
    let length = text.chars().count();
    if length > max {
        return Err(AcsiError::TooLong { ty, max, length });
    }
    Ok(())
}

/// `VISIBLE-STRING`: an MMS VisibleString of at most `max` characters, in the ISO 646 set (8-1 §8.1.2.5).
pub fn visible_to_data(text: &str, max: usize) -> Result<Data, AcsiError> {
    check_iso646("Visible string", text)?;
    check_chars("Visible string", text, max)?;
    Ok(Data::VisibleString(text.to_string()))
}

/// Reads a visible string of at most `max` characters.
pub fn visible_from_data(data: &Data, max: usize) -> Result<String, AcsiError> {
    let Data::VisibleString(text) = data else {
        return Err(AcsiError::WrongType {
            ty: "Visible string",
            expected: "visible-string",
        });
    };
    check_iso646("Visible string", text)?;
    check_chars("Visible string", text, max)?;
    Ok(text.clone())
}

/// `OCTET STRING`: at most `max` octets (8-1 §8.1.2.4).
pub fn octets_to_data(bytes: &[u8], max: usize) -> Result<Data, AcsiError> {
    if bytes.len() > max {
        return Err(AcsiError::TooLong {
            ty: "Octet string",
            max,
            length: bytes.len(),
        });
    }
    Ok(Data::OctetString(bytes.to_vec()))
}

/// Reads an octet string of at most `max` octets.
pub fn octets_from_data(data: &Data, max: usize) -> Result<Vec<u8>, AcsiError> {
    let Data::OctetString(bytes) = data else {
        return Err(AcsiError::WrongType {
            ty: "Octet string",
            expected: "octet-string",
        });
    };
    if bytes.len() > max {
        return Err(AcsiError::TooLong {
            ty: "Octet string",
            max,
            length: bytes.len(),
        });
    }
    Ok(bytes.clone())
}

/// `Unicode string`: an MMS string in UTF-8 of at most `max` characters (8-1 §8.1.2.6, Annex F.3.3).
pub fn unicode_to_data(text: &str, max: usize) -> Result<Data, AcsiError> {
    check_chars("Unicode string", text, max)?;
    Ok(Data::MMSString(DataMMSString(text.to_string())))
}

/// Reads a Unicode string of at most `max` characters.
pub fn unicode_from_data(data: &Data, max: usize) -> Result<String, AcsiError> {
    let Data::MMSString(DataMMSString(text)) = data else {
        return Err(AcsiError::WrongType {
            ty: "Unicode string",
            expected: "mMSString",
        });
    };
    check_chars("Unicode string", text, max)?;
    Ok(text.clone())
}

// ---------------------------------------------------------------------------------------------------------------
// Enumerations and coded enumerations (8-1 §8.1.2.2, §8.1.2.3)
// ---------------------------------------------------------------------------------------------------------------

/// The reading of an enumeration value against its standardised literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enumerated {
    /// A standardised value, with the name of its literal.
    Standard(&'static str),
    /// A value that is not standardised, in the range of the integer: an extension, not an error (§8.1.2.2).
    Extension(i64),
    /// A negative value: private to the implementation (§8.1.2.2).
    Private(i64),
}

/// An enumeration as an MMS Integer.
pub fn enumeration_to_data(value: i64) -> Data {
    Data::Integer(i128::from(value))
}

/// Reads the integer of an enumeration.
pub fn enumeration_from_data(data: &Data) -> Result<i64, AcsiError> {
    match data {
        Data::Integer(value) => i64::try_from(*value).map_err(|_| AcsiError::OutOfRange {
            ty: "enumeration",
            min: i128::from(i64::MIN),
            max: i128::from(i64::MAX),
            value: *value,
        }),
        _ => Err(AcsiError::WrongType {
            ty: "enumeration",
            expected: "integer",
        }),
    }
}

/// Classifies an enumeration value against the literals of its enumeration in the catalogue.
pub fn classify(value: i64, enumeration: &'static Enumeration) -> Enumerated {
    let literal: Option<&Literal> = enumeration
        .literals
        .iter()
        .find(|literal| i64::from(literal.value) == value);
    match literal {
        Some(literal) => Enumerated::Standard(literal.name),
        None if value < 0 => Enumerated::Private(value),
        None => Enumerated::Extension(value),
    }
}

/// The size in bits of a coded enumeration whose largest standardised value is `max`: the number of bits that
/// represent `max`, and at least one (§8.1.2.3).
pub fn coded_enum_size(max: u32) -> u8 {
    let bits = u32::BITS.saturating_sub(max.leading_zeros());
    u8::try_from(bits.max(1)).unwrap_or(u8::MAX)
}

/// A coded enumeration of `size` bits as an MMS bit string: the value is written most significant bit first
/// (§8.1.2.3). A size of zero is refused (§8.1.2.1), and a value that does not fit in the size is refused.
pub fn coded_enum_to_data(value: u32, size: u8) -> Result<Data, AcsiError> {
    if size == 0 {
        return Err(AcsiError::InvalidBits {
            ty: "coded enumeration",
            why: "zero length",
        });
    }
    if let Some(limit) = 1u32.checked_shl(u32::from(size)) {
        if value >= limit {
            return Err(AcsiError::OutOfRange {
                ty: "coded enumeration",
                min: 0,
                max: i128::from(limit.saturating_sub(1)),
                value: i128::from(value),
            });
        }
    }
    let flags: Vec<bool> = (0..size)
        .rev()
        .map(|shift| (value >> shift) & 1 == 1)
        .collect();
    Ok(flags_to_bits(&flags))
}

/// Reads a coded enumeration of `size` bits from the bit string the peer sent: the bits past `size` are ignored,
/// the missing ones are zero (§8.1.2.1). A bit string of no bit is refused.
pub fn coded_enum_from_data(data: &Data, size: u8) -> Result<u32, AcsiError> {
    let Data::BitString(bits) = data else {
        return Err(AcsiError::WrongType {
            ty: "coded enumeration",
            expected: "bit-string",
        });
    };
    let length = bits
        .bytes
        .len()
        .saturating_mul(8)
        .saturating_sub(usize::from(bits.unused));
    if length == 0 {
        return Err(AcsiError::InvalidBits {
            ty: "coded enumeration",
            why: "zero length",
        });
    }
    Ok(read_bits(bits, length, usize::from(size)))
}

/// The unsigned value of the first `size` bits of a bit string of `length` bits, the missing bits being zero.
fn read_bits(bits: &BitString, length: usize, size: usize) -> u32 {
    let mut value: u32 = 0;
    for index in 0..size {
        let set = index < length && bit(bits, index);
        value = value.checked_shl(1).unwrap_or(0) | u32::from(set);
    }
    value
}

fn bit(bits: &BitString, index: usize) -> bool {
    bits.bytes
        .get(index >> 3)
        .is_some_and(|byte| byte & (0x80u8 >> (index & 7)) != 0)
}

/// Builds the bit string of the flags, in order (bit zero first), with the padding bits of the last octet zero
/// (§8.1.2.1). The flags are not empty: the callers check that.
fn flags_to_bits(flags: &[bool]) -> Data {
    let bytes_len = flags.len().div_ceil(8);
    let mut bytes = vec![0u8; bytes_len];
    for (index, flag) in flags.iter().enumerate() {
        if *flag {
            if let Some(byte) = bytes.get_mut(index / 8) {
                *byte |= 0x80u8 >> (index % 8);
            }
        }
    }
    let unused = bytes_len.saturating_mul(8).saturating_sub(flags.len());
    Data::BitString(BitString {
        unused: u8::try_from(unused).unwrap_or(0),
        bytes,
    })
}

/// Reads the first `size` flags of a bit string; a missing bit is false, an extra one is ignored (§8.1.2.1).
fn bits_to_flags(data: &Data, ty: &'static str, size: usize) -> Result<Vec<bool>, AcsiError> {
    let Data::BitString(bits) = data else {
        return Err(AcsiError::WrongType {
            ty,
            expected: "bit-string",
        });
    };
    let length = bits
        .bytes
        .len()
        .saturating_mul(8)
        .saturating_sub(usize::from(bits.unused));
    Ok((0..size)
        .map(|index| index < length && bit(bits, index))
        .collect())
}

// ---------------------------------------------------------------------------------------------------------------
// Timestamp (8-1 §8.1.3.7, Annex F.3.1)
// ---------------------------------------------------------------------------------------------------------------

/// The ACSI `Timestamp` as an MMS UtcTime of eight octets.
pub fn timestamp_to_data(stamp: UtcTimestamp) -> Data {
    Data::UtcTime(UtcTime::from_timestamp(stamp))
}

/// Reads a `Timestamp`: the eight octets, and a valid time quality (N3).
pub fn timestamp_from_data(data: &Data) -> Result<UtcTimestamp, AcsiError> {
    let Data::UtcTime(time) = data else {
        return Err(AcsiError::WrongType {
            ty: "Timestamp",
            expected: "utc-time",
        });
    };
    time.timestamp().map_err(|_| AcsiError::InvalidValue {
        ty: "Timestamp",
        why: "not eight octets with a valid time quality",
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Common types (8-1 Table 43 and Table 44, §8.1.3.17)
// ---------------------------------------------------------------------------------------------------------------

/// The validity of a quality (IEC 61850-7-2, Table 44 of 8-1: bits 0 and 1, coded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validity {
    /// `good` (`00`).
    Good,
    /// `invalid` (`01`).
    Invalid,
    /// `reserved` (`10`).
    Reserved,
    /// `questionable` (`11`).
    Questionable,
}

/// The source of a quality (bit 10): `process` (0) or `substituted` (1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `process`.
    Process,
    /// `substituted`.
    Substituted,
}

/// The detailed quality flags (bits 2 to 9 of Table 44).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DetailQuality {
    /// `overflow` (bit 2).
    pub overflow: bool,
    /// `outOfRange` (bit 3).
    pub out_of_range: bool,
    /// `badReference` (bit 4).
    pub bad_reference: bool,
    /// `oscillatory` (bit 5).
    pub oscillatory: bool,
    /// `failure` (bit 6).
    pub failure: bool,
    /// `oldData` (bit 7).
    pub old_data: bool,
    /// `inconsistent` (bit 8).
    pub inconsistent: bool,
    /// `inaccurate` (bit 9).
    pub inaccurate: bool,
}

/// The quality of a value (IEC 61850-7-2), as the 13-bit string of 8-1 Table 44.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quality {
    /// The validity (bits 0 and 1).
    pub validity: Validity,
    /// The detailed flags (bits 2 to 9).
    pub detail: DetailQuality,
    /// The source (bit 10).
    pub source: Source,
    /// The value comes from a test (bit 11).
    pub test: bool,
    /// The operator blocked the value (bit 12).
    pub operator_blocked: bool,
}

/// The number of bits of a quality (Table 44 of 8-1).
const QUALITY_BITS: usize = 13;

impl Quality {
    /// A good value from the process, not under test and not blocked.
    pub fn good() -> Self {
        Self {
            validity: Validity::Good,
            detail: DetailQuality::default(),
            source: Source::Process,
            test: false,
            operator_blocked: false,
        }
    }

    /// The MMS bit string of the quality: 13 bits, in the order of Table 44.
    pub fn to_data(self) -> Data {
        let (high, low) = match self.validity {
            Validity::Good => (false, false),
            Validity::Invalid => (false, true),
            Validity::Reserved => (true, false),
            Validity::Questionable => (true, true),
        };
        let flags = [
            high,
            low,
            self.detail.overflow,
            self.detail.out_of_range,
            self.detail.bad_reference,
            self.detail.oscillatory,
            self.detail.failure,
            self.detail.old_data,
            self.detail.inconsistent,
            self.detail.inaccurate,
            self.source == Source::Substituted,
            self.test,
            self.operator_blocked,
        ];
        flags_to_bits(&flags)
    }

    /// Reads a quality from its bit string. Bits past the 13th are ignored, missing ones are zero (§8.1.2.1).
    pub fn from_data(data: &Data) -> Result<Self, AcsiError> {
        let flags = bits_to_flags(data, "Quality", QUALITY_BITS)?;
        let at = |index: usize| flags.get(index).copied().unwrap_or(false);
        let validity = match (at(0), at(1)) {
            (false, false) => Validity::Good,
            (false, true) => Validity::Invalid,
            (true, false) => Validity::Reserved,
            (true, true) => Validity::Questionable,
        };
        Ok(Self {
            validity,
            detail: DetailQuality {
                overflow: at(2),
                out_of_range: at(3),
                bad_reference: at(4),
                oscillatory: at(5),
                failure: at(6),
                old_data: at(7),
                inconsistent: at(8),
                inaccurate: at(9),
            },
            source: if at(10) {
                Source::Substituted
            } else {
                Source::Process
            },
            test: at(11),
            operator_blocked: at(12),
        })
    }
}

/// The check conditions of a control (CheckConditions, Table 43 of 8-1): `synchrocheck` (bit 0) and
/// `interlockCheck` (bit 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CheckConditions {
    /// `synchrocheck`.
    pub synchrocheck: bool,
    /// `interlockCheck`.
    pub interlock_check: bool,
}

impl CheckConditions {
    /// The bit string of the two conditions.
    pub fn to_data(self) -> Data {
        flags_to_bits(&[self.synchrocheck, self.interlock_check])
    }

    /// Reads the conditions; missing bits are false.
    pub fn from_data(data: &Data) -> Result<Self, AcsiError> {
        let flags = bits_to_flags(data, "CheckConditions", 2)?;
        Ok(Self {
            synchrocheck: flags.first().copied().unwrap_or(false),
            interlock_check: flags.get(1).copied().unwrap_or(false),
        })
    }
}

/// The originator of a control (`Originator`, IEC 61850-7-2): its category and its identity, as a structure of
/// `orCat` (the enumeration `OriginatorCategoryKind`) and `orIdent` (an octet string of 64).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Originator {
    /// The category, as the integer of the enumeration (IEC 61850-7-2 Originator, the order of its components follows the catalogue).
    pub category: i64,
    /// The identity: at most 64 octets.
    pub identity: Vec<u8>,
}

/// The longest `orIdent`, in octets (`Octet64`).
pub const ORIGINATOR_IDENTITY_MAX: usize = 64;

impl Originator {
    /// The MMS structure of the originator.
    pub fn to_data(&self) -> Result<Data, AcsiError> {
        Ok(Data::Structure(vec![
            enumeration_to_data(self.category),
            octets_to_data(&self.identity, ORIGINATOR_IDENTITY_MAX)?,
        ]))
    }

    /// Reads an originator: a structure of two components, in this order.
    pub fn from_data(data: &Data) -> Result<Self, AcsiError> {
        let Data::Structure(components) = data else {
            return Err(AcsiError::WrongType {
                ty: "Originator",
                expected: "structure",
            });
        };
        let (Some(category), Some(identity)) = (components.first(), components.get(1)) else {
            return Err(AcsiError::InvalidValue {
                ty: "Originator",
                why: "the structure has no orCat and orIdent",
            });
        };
        Ok(Self {
            category: enumeration_from_data(category)?,
            identity: octets_from_data(identity, ORIGINATOR_IDENTITY_MAX)?,
        })
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Type descriptions (8-1 §8.1.1, §8.1.2)
// ---------------------------------------------------------------------------------------------------------------

/// The type description of `BOOLEAN`.
pub fn describe_boolean() -> TypeDescription {
    TypeDescription::Boolean
}

/// The type description of a signed integer: an integer of its size in bits.
pub fn describe_signed(kind: SignedInteger) -> TypeDescription {
    TypeDescription::Integer(Unsigned8(i64::from(kind.bits())))
}

/// The type description of an unsigned integer.
pub fn describe_unsigned(kind: UnsignedInteger) -> TypeDescription {
    TypeDescription::Unsigned(Unsigned8(i64::from(kind.bits())))
}

/// The type description of `FLOAT32`: the single precision form.
pub fn describe_float32() -> TypeDescription {
    TypeDescription::FloatingPoint(TypeDescriptionFloatingPoint {
        format_width: Unsigned8(32),
        exponent_width: Unsigned8(8),
    })
}

/// The type description of an enumeration whose largest standardised value is `max`: the minimum integer that
/// holds it (§8.1.2.2), as a signed integer of `bits(max) + 1` bits.
pub fn describe_enumeration(max: i64) -> TypeDescription {
    let bits = u64::BITS.saturating_sub(u64::try_from(max.max(0)).unwrap_or(0).leading_zeros());
    TypeDescription::Integer(Unsigned8(i64::from(bits.saturating_add(1))))
}

/// The type description of a coded enumeration of `size` bits (§8.1.2.3).
pub fn describe_coded_enum(size: u8) -> TypeDescription {
    TypeDescription::BitString(Integer32(i64::from(size)))
}

/// The size in bits of a `Dbpos`: the largest standardised value of DpStatusKind is 3 (§8.1.2.3).
pub const DOUBLE_POSITION_SIZE: u8 = 2;

/// The double-point status of IEC 61850-7-2 Table 24 (DpStatusKind, the form of a `Dbpos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoublePosition {
    /// Value 0: the equipment is in an intermediate state.
    IntermediateState,
    /// Value 1: the equipment is open.
    Off,
    /// Value 2: the equipment is closed.
    On,
    /// Value 3: the server cannot tell whether the equipment is open, closed or in an intermediate state.
    BadState,
}

impl DoublePosition {
    /// The value of the literal in the namespace of IEC 61850-7-2.
    pub fn value(self) -> u32 {
        match self {
            Self::IntermediateState => 0,
            Self::Off => 1,
            Self::On => 2,
            Self::BadState => 3,
        }
    }

    /// The name of the literal in the namespace of IEC 61850-7-2.
    pub fn name(self) -> &'static str {
        match self {
            Self::IntermediateState => "intermediate-state",
            Self::Off => "off",
            Self::On => "on",
            Self::BadState => "bad-state",
        }
    }

    fn from_value(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::IntermediateState),
            1 => Some(Self::Off),
            2 => Some(Self::On),
            3 => Some(Self::BadState),
            _ => None,
        }
    }
}

/// A double-point status as an MMS bit string of two bits (`Dbpos`, 8-1 §8.1.2.3).
pub fn double_position_to_data(position: DoublePosition) -> Result<Data, AcsiError> {
    coded_enum_to_data(position.value(), DOUBLE_POSITION_SIZE)
}

/// Reads a double-point status from the bit string the peer sent. The bits past the two of the type are ignored
/// (§8.1.2.1); a value outside the four literals cannot occur and is refused.
pub fn double_position_from_data(data: &Data) -> Result<DoublePosition, AcsiError> {
    let value = coded_enum_from_data(data, DOUBLE_POSITION_SIZE)?;
    DoublePosition::from_value(value).ok_or(AcsiError::OutOfRange {
        ty: "double position",
        min: 0,
        max: 3,
        value: i128::from(value),
    })
}

/// The size in bits of a `Tcmd`: the largest standardised value of StepControlKind is 3 (§8.1.2.3).
pub const STEP_COMMAND_SIZE: u8 = 2;

/// The step command of IEC 61850-7-2 Table 23 (StepControlKind, the form of a `Tcmd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepCommand {
    /// Value 0: deactivate the persistent output.
    Stop,
    /// Value 1: decrease the step by one, or until the local timeout or `stop`.
    Lower,
    /// Value 2: increase the step by one, or until the local timeout or `stop`.
    Higher,
    /// Value 3: reserved by the standard.
    Reserved,
}

impl StepCommand {
    /// The value of the literal in the namespace of IEC 61850-7-2.
    pub fn value(self) -> u32 {
        match self {
            Self::Stop => 0,
            Self::Lower => 1,
            Self::Higher => 2,
            Self::Reserved => 3,
        }
    }

    /// The name of the literal in the namespace of IEC 61850-7-2.
    pub fn name(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Lower => "lower",
            Self::Higher => "higher",
            Self::Reserved => "reserved",
        }
    }

    fn from_value(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Stop),
            1 => Some(Self::Lower),
            2 => Some(Self::Higher),
            3 => Some(Self::Reserved),
            _ => None,
        }
    }
}

/// A step command as an MMS bit string of two bits (`Tcmd`, 8-1 §8.1.2.3).
pub fn step_command_to_data(command: StepCommand) -> Result<Data, AcsiError> {
    coded_enum_to_data(command.value(), STEP_COMMAND_SIZE)
}

/// Reads a step command from the bit string the peer sent; the bits past the two of the type are ignored (§8.1.2.1).
pub fn step_command_from_data(data: &Data) -> Result<StepCommand, AcsiError> {
    let value = coded_enum_from_data(data, STEP_COMMAND_SIZE)?;
    StepCommand::from_value(value).ok_or(AcsiError::OutOfRange {
        ty: "step command",
        min: 0,
        max: 3,
        value: i128::from(value),
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Entries, trigger options and option fields (8-1 §8.1.3.5, §8.1.3.8, §8.1.3.9, §8.1.3.13, §8.1.3.15)
// ---------------------------------------------------------------------------------------------------------------

/// The size in octets of an `EntryID` (§8.1.3.5): a fixed octet string.
pub const ENTRY_ID_OCTETS: usize = 8;

/// An `EntryID`: the handle of an entry of a log or a buffered report. The reserved value is eight zero octets.
pub type EntryId = [u8; ENTRY_ID_OCTETS];

/// An `EntryID` as an MMS octet string of eight octets.
pub fn entry_id_to_data(id: &EntryId) -> Data {
    Data::OctetString(id.to_vec())
}

/// Reads an `EntryID`: exactly eight octets. Any other length is refused.
pub fn entry_id_from_data(data: &Data) -> Result<EntryId, AcsiError> {
    let Data::OctetString(bytes) = data else {
        return Err(AcsiError::WrongType {
            ty: "EntryID",
            expected: "octet-string",
        });
    };
    <EntryId>::try_from(bytes.as_slice()).map_err(|_| AcsiError::InvalidValue {
        ty: "EntryID",
        why: "eight octets are expected",
    })
}

/// An `EntryTime` (§8.1.3.8): the milliseconds since midnight and the day since 1 January 1984, in the six-octet
/// TimeOfDay. The NULL entry time has both fields at zero (7-2 §6.2.3.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryTime {
    /// Milliseconds since midnight.
    pub milliseconds: u32,
    /// Days since 1 January 1984 (day 0).
    pub day: u16,
}

/// An `EntryTime` as an MMS TimeOfDay of six octets.
pub fn entry_time_to_data(time: EntryTime) -> Result<Data, AcsiError> {
    TimeOfDay::from_milliseconds_and_day(time.milliseconds, time.day)
        .map(Data::BinaryTime)
        .map_err(|_| AcsiError::InvalidValue {
            ty: "EntryTime",
            why: "the milliseconds are out of the range of a time of day",
        })
}

/// Reads an `EntryTime`. The four-octet form of TimeOfDay has no date and is refused: an entry time has a day.
pub fn entry_time_from_data(data: &Data) -> Result<EntryTime, AcsiError> {
    let Data::BinaryTime(time) = data else {
        return Err(AcsiError::WrongType {
            ty: "EntryTime",
            expected: "binary-time",
        });
    };
    let invalid = |why| AcsiError::InvalidValue {
        ty: "EntryTime",
        why,
    };
    let milliseconds = time
        .milliseconds()
        .map_err(|_| invalid("the milliseconds are not a time of day"))?;
    let day = time
        .day()
        .map_err(|_| invalid("the day is not a time of day"))?
        .ok_or_else(|| invalid("the six-octet form, with the day, is expected"))?;
    Ok(EntryTime { milliseconds, day })
}

/// The trigger conditions of a block (8-1 §8.1.3.9, Table 37). Bit 0 is reserved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TriggerOptions {
    /// Bit 1: data-change.
    pub data_change: bool,
    /// Bit 2: quality-change.
    pub quality_change: bool,
    /// Bit 3: data-update.
    pub data_update: bool,
    /// Bit 4: integrity.
    pub integrity: bool,
    /// Bit 5: general-interrogation.
    pub general_interrogation: bool,
}

/// The trigger conditions as an MMS bit string of six bits (the minimum size of the standard).
pub fn trigger_options_to_data(options: TriggerOptions) -> Data {
    flags_to_bits(&[
        false,
        options.data_change,
        options.quality_change,
        options.data_update,
        options.integrity,
        options.general_interrogation,
    ])
}

/// Reads the trigger conditions. The bits past the six of the minimum are ignored, the missing ones are zero.
pub fn trigger_options_from_data(data: &Data) -> Result<TriggerOptions, AcsiError> {
    let flags = bits_to_flags(data, "TriggerConditions", TRIGGER_OPTIONS_BITS)?;
    Ok(TriggerOptions {
        data_change: flag(&flags, 1),
        quality_change: flag(&flags, 2),
        data_update: flag(&flags, 3),
        integrity: flag(&flags, 4),
        general_interrogation: flag(&flags, 5),
    })
}

/// The minimum size of the trigger conditions in bits (§8.1.3.9).
const TRIGGER_OPTIONS_BITS: usize = 6;

/// The optional fields of a report (8-1 §8.1.3.13, Table 41, the type `OptFlds`). Bit 0 is reserved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReportOptions {
    /// Bit 1: sequence-number.
    pub sequence_number: bool,
    /// Bit 2: report-time-stamp.
    pub report_time_stamp: bool,
    /// Bit 3: reason-for-inclusion.
    pub reason_for_inclusion: bool,
    /// Bit 4: data-set-name.
    pub data_set_name: bool,
    /// Bit 5: data-reference.
    pub data_reference: bool,
    /// Bit 6: buffer-overflow.
    pub buffer_overflow: bool,
    /// Bit 7: entryID.
    pub entry_id: bool,
    /// Bit 8: conf-revision.
    pub conf_revision: bool,
    /// Bit 9: segmentation.
    pub segmentation: bool,
}

/// The minimum size of the report options in bits (§8.1.3.13).
const REPORT_OPTIONS_BITS: usize = 10;

/// The report options as an MMS bit string of ten bits (the minimum size of the standard).
pub fn report_options_to_data(options: ReportOptions) -> Data {
    flags_to_bits(&[
        false,
        options.sequence_number,
        options.report_time_stamp,
        options.reason_for_inclusion,
        options.data_set_name,
        options.data_reference,
        options.buffer_overflow,
        options.entry_id,
        options.conf_revision,
        options.segmentation,
    ])
}

/// Reads the report options. The bits past the ten of the minimum are ignored, the missing ones are zero.
pub fn report_options_from_data(data: &Data) -> Result<ReportOptions, AcsiError> {
    let flags = bits_to_flags(data, "RCBReportOptions", REPORT_OPTIONS_BITS)?;
    Ok(ReportOptions {
        sequence_number: flag(&flags, 1),
        report_time_stamp: flag(&flags, 2),
        reason_for_inclusion: flag(&flags, 3),
        data_set_name: flag(&flags, 4),
        data_reference: flag(&flags, 5),
        buffer_overflow: flag(&flags, 6),
        entry_id: flag(&flags, 7),
        conf_revision: flag(&flags, 8),
        segmentation: flag(&flags, 9),
    })
}

/// The optional fields of a sampled value message (8-1 §8.1.3.15, Table 42, the type `SvOptFlds`). Bit 1 is
/// reserved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SvOptions {
    /// Bit 0: refresh-time.
    pub refresh_time: bool,
    /// Bit 2: sample-rate.
    pub sample_rate: bool,
    /// Bit 3: data-set-name.
    pub data_set_name: bool,
    /// Bit 4: security.
    pub security: bool,
    /// Bit 5: sample-mode.
    pub sample_mode: bool,
    /// Bit 6: synch-source-identity.
    pub synch_source_identity: bool,
}

/// The minimum size of the sampled value options in bits (§8.1.3.15).
const SV_OPTIONS_BITS: usize = 7;

/// The sampled value options as an MMS bit string of seven bits (the minimum size of the standard).
pub fn sv_options_to_data(options: SvOptions) -> Data {
    flags_to_bits(&[
        options.refresh_time,
        false,
        options.sample_rate,
        options.data_set_name,
        options.security,
        options.sample_mode,
        options.synch_source_identity,
    ])
}

/// Reads the sampled value options. The bits past the seven of the minimum are ignored, the missing ones are zero.
pub fn sv_options_from_data(data: &Data) -> Result<SvOptions, AcsiError> {
    let flags = bits_to_flags(data, "SVMessageOptions", SV_OPTIONS_BITS)?;
    Ok(SvOptions {
        refresh_time: flag(&flags, 0),
        sample_rate: flag(&flags, 2),
        data_set_name: flag(&flags, 3),
        security: flag(&flags, 4),
        sample_mode: flag(&flags, 5),
        synch_source_identity: flag(&flags, 6),
    })
}

/// The flag at an index of a list of flags; a missing flag is false.
fn flag(flags: &[bool], index: usize) -> bool {
    flags.get(index).copied().unwrap_or(false)
}

/// The type description of a bit string of `size` bits (§8.1.2.1).
pub fn describe_bit_string(size: u8) -> TypeDescription {
    TypeDescription::BitString(Integer32(i64::from(size)))
}

/// The type description of `TriggerConditions`: a bit string of six bits, the minimum size (§8.1.3.9).
pub fn describe_trigger_options() -> TypeDescription {
    describe_bit_string(6)
}

/// The type description of `RCBReportOptions` (`OptFlds`): a bit string of ten bits, the minimum size (§8.1.3.13).
pub fn describe_report_options() -> TypeDescription {
    describe_bit_string(10)
}

/// The type description of `SVMessageOptions` (`SvOptFlds`): a bit string of seven bits, the minimum size (§8.1.3.15).
pub fn describe_sv_options() -> TypeDescription {
    describe_bit_string(7)
}

/// The type description of an `EntryID`: an octet string of eight octets (§8.1.3.5).
pub fn describe_entry_id() -> TypeDescription {
    TypeDescription::OctetString(Integer32(8))
}

/// The type description of an `EntryTime`: the six-octet TimeOfDay (`true`, §8.1.3.8).
pub fn describe_entry_time() -> TypeDescription {
    TypeDescription::BinaryTime(true)
}

/// The type description of an octet string of at most `max` octets.
pub fn describe_octets(max: i64) -> TypeDescription {
    TypeDescription::OctetString(Integer32(max))
}

/// The type description of a visible string of at most `max` characters: a variable length with the maximum, which MMS
/// writes as a negative size (ISO 9506-2 §14.2.2.4.2; the reference peer describes its strings this way too).
pub fn describe_visible(max: i64) -> TypeDescription {
    TypeDescription::VisibleString(Integer32(max.saturating_neg()))
}

/// The type description of a Unicode string of at most `max` characters: a variable length with the maximum (a negative size).
pub fn describe_unicode(max: i64) -> TypeDescription {
    TypeDescription::MMSString(Integer32(max.saturating_neg()))
}

/// The type description of a `Timestamp`: the UtcTime of Annex F.3.1.
pub fn describe_timestamp() -> TypeDescription {
    TypeDescription::UtcTime
}

/// The type description of a `Quality`: 13 bits.
pub fn describe_quality() -> TypeDescription {
    TypeDescription::BitString(Integer32(13))
}

/// The type description of `CheckConditions`: 2 bits.
pub fn describe_check_conditions() -> TypeDescription {
    TypeDescription::BitString(Integer32(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_integer_has_the_range_of_its_size() {
        assert_eq!(SignedInteger::I8.range(), (-128, 127));
        assert_eq!(SignedInteger::I16.range(), (-32_768, 32_767));
        assert_eq!(SignedInteger::I32.range(), (-2_147_483_648, 2_147_483_647));
        assert_eq!(
            SignedInteger::I64.range(),
            (i128::from(i64::MIN), i128::from(i64::MAX))
        );
        assert_eq!(UnsignedInteger::U8.max(), 255);
        assert_eq!(UnsignedInteger::U32.max(), 4_294_967_295);
    }

    #[test]
    fn a_boolean_is_an_mms_boolean() {
        assert_eq!(boolean_to_data(true), Data::Boolean(true));
        assert_eq!(boolean_from_data(&Data::Boolean(false)), Ok(false));
        assert_eq!(
            boolean_from_data(&Data::Integer(1)),
            Err(AcsiError::WrongType {
                ty: "BOOLEAN",
                expected: "boolean"
            })
        );
    }

    #[test]
    fn signed_integers_keep_their_range() {
        for (kind, value) in [
            (SignedInteger::I8, -128),
            (SignedInteger::I8, 127),
            (SignedInteger::I16, -32_768),
            (SignedInteger::I32, 2_147_483_647),
            (SignedInteger::I64, i64::MIN),
            (SignedInteger::I64, i64::MAX),
        ] {
            let data = signed_to_data(kind, value).unwrap();
            assert_eq!(signed_from_data(kind, &data), Ok(value));
        }
        assert!(matches!(
            signed_to_data(SignedInteger::I8, 128),
            Err(AcsiError::OutOfRange {
                ty: "INT8",
                min: -128,
                max: 127,
                value: 128
            })
        ));
        assert!(matches!(
            signed_from_data(SignedInteger::I16, &Data::Integer(40_000)),
            Err(AcsiError::OutOfRange { ty: "INT16", .. })
        ));
        assert!(matches!(
            signed_from_data(SignedInteger::I8, &Data::Unsigned(1)),
            Err(AcsiError::WrongType { ty: "INT8", .. })
        ));
    }

    #[test]
    fn unsigned_integers_keep_their_range() {
        assert_eq!(
            unsigned_to_data(UnsignedInteger::U8, 255),
            Ok(Data::Unsigned(255))
        );
        assert_eq!(
            unsigned_to_data(UnsignedInteger::U8, 256),
            Err(AcsiError::OutOfRange {
                ty: "INT8U",
                min: 0,
                max: 255,
                value: 256
            })
        );
        assert_eq!(
            unsigned_from_data(UnsignedInteger::U32, &Data::Unsigned(4_294_967_295)),
            Ok(4_294_967_295)
        );
        assert!(matches!(
            unsigned_from_data(UnsignedInteger::U16, &Data::Integer(-1)),
            Err(AcsiError::OutOfRange { ty: "INT16U", .. })
        ));
        assert!(matches!(
            unsigned_from_data(UnsignedInteger::U16, &Data::Unsigned(70_000)),
            Err(AcsiError::OutOfRange { ty: "INT16U", .. })
        ));
    }

    #[test]
    fn a_float32_is_the_single_precision_form() {
        // 1.5 is 0x3FC00000: the width octet 8, then the four octets of the value.
        assert_eq!(
            float32_to_data(1.5),
            Data::FloatingPoint(FloatingPoint(vec![8, 0x3F, 0xC0, 0, 0]))
        );
        assert_eq!(float32_from_data(&float32_to_data(1.5)), Ok(1.5));
        assert!(float32_from_data(&float32_to_data(f32::NAN))
            .unwrap()
            .is_nan());
        // A double precision form (width octet 11) is another type.
        let double = Data::FloatingPoint(FloatingPoint(vec![11, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(matches!(
            float32_from_data(&double),
            Err(AcsiError::InvalidValue { ty: "FLOAT32", .. })
        ));
        assert!(matches!(
            float32_from_data(&Data::FloatingPoint(FloatingPoint(vec![8, 0x3F, 0xC0]))),
            Err(AcsiError::InvalidValue { ty: "FLOAT32", .. })
        ));
    }

    #[test]
    fn visible_strings_are_iso646_and_bounded() {
        assert_eq!(
            visible_to_data("LLN0$ST", 64),
            Ok(Data::VisibleString("LLN0$ST".into()))
        );
        assert!(matches!(
            visible_to_data("é", 64),
            Err(AcsiError::InvalidCharacter {
                ty: "Visible string",
                character: 'é'
            })
        ));
        assert!(matches!(
            visible_to_data("a\u{7}", 64),
            Err(AcsiError::InvalidCharacter { .. })
        ));
        assert!(visible_to_data(&"a".repeat(64), 64).is_ok());
        assert_eq!(
            visible_to_data(&"a".repeat(65), 64),
            Err(AcsiError::TooLong {
                ty: "Visible string",
                max: 64,
                length: 65
            })
        );
        assert!(matches!(
            visible_from_data(&Data::Integer(1), 64),
            Err(AcsiError::WrongType { .. })
        ));
    }

    #[test]
    fn octet_strings_are_bounded_in_octets() {
        assert_eq!(
            octets_to_data(&[1, 2], 2),
            Ok(Data::OctetString(vec![1, 2]))
        );
        assert_eq!(
            octets_to_data(&[1, 2, 3], 2),
            Err(AcsiError::TooLong {
                ty: "Octet string",
                max: 2,
                length: 3
            })
        );
        assert_eq!(
            octets_from_data(&Data::OctetString(vec![9]), 1),
            Ok(vec![9])
        );
    }

    #[test]
    fn unicode_strings_are_utf8_bounded_in_characters() {
        assert_eq!(
            unicode_to_data("é€", 2),
            Ok(Data::MMSString(DataMMSString("é€".into())))
        );
        // Characters, not octets: 255 characters of four octets are allowed, 256 are not.
        let four = "\u{1D11E}";
        assert!(unicode_to_data(&four.repeat(255), 255).is_ok());
        assert_eq!(
            unicode_to_data(&four.repeat(256), 255),
            Err(AcsiError::TooLong {
                ty: "Unicode string",
                max: 255,
                length: 256
            })
        );
        assert!(matches!(
            unicode_from_data(&Data::VisibleString("a".into()), 8),
            Err(AcsiError::WrongType { .. })
        ));
    }

    fn orcat() -> &'static Enumeration {
        crate::catalogue::enumeration("OriginatorCategoryKind").unwrap()
    }

    #[test]
    fn an_enumeration_value_is_standard_an_extension_or_private() {
        assert_eq!(classify(1, orcat()), Enumerated::Standard("bay-control"));
        assert_eq!(classify(0, orcat()), Enumerated::Standard("not-supported"));
        // Not standardised, and non-negative: an extension, not an error (§8.1.2.2).
        assert_eq!(classify(99, orcat()), Enumerated::Extension(99));
        // Negative: private to the implementation.
        assert_eq!(classify(-3, orcat()), Enumerated::Private(-3));
        assert_eq!(enumeration_from_data(&enumeration_to_data(-3)), Ok(-3));
    }

    #[test]
    fn a_double_position_is_two_bits_with_the_values_of_table_24() {
        // The size is the number of bits of the largest standardised value, 3.
        assert_eq!(coded_enum_size(3), DOUBLE_POSITION_SIZE);
        // The four literals of DpStatusKind, most significant bit first, in the first two bits of the octet.
        let vectors = [
            (DoublePosition::IntermediateState, 0x00),
            (DoublePosition::Off, 0x40),
            (DoublePosition::On, 0x80),
            (DoublePosition::BadState, 0xC0),
        ];
        for (position, octet) in vectors {
            assert_eq!(
                double_position_to_data(position),
                Ok(Data::BitString(BitString {
                    unused: 6,
                    bytes: vec![octet]
                }))
            );
            assert_eq!(
                double_position_from_data(&double_position_to_data(position).unwrap()),
                Ok(position)
            );
        }
        assert_eq!(DoublePosition::On.name(), "on");
        assert_eq!(DoublePosition::BadState.value(), 3);
    }

    #[test]
    fn a_double_position_read_ignores_the_bits_past_its_size() {
        // The first two bits are 10 (on); the six bits after them are ignored (§8.1.2.1).
        let data = Data::BitString(BitString {
            unused: 0,
            bytes: vec![0b1011_1111],
        });
        assert_eq!(double_position_from_data(&data), Ok(DoublePosition::On));
        // A value that is not a bit string is refused with its type.
        assert!(matches!(
            double_position_from_data(&Data::Integer(2)),
            Err(AcsiError::WrongType { .. })
        ));
    }

    #[test]
    fn a_step_command_is_two_bits_with_the_values_of_table_23() {
        assert_eq!(coded_enum_size(3), STEP_COMMAND_SIZE);
        let vectors = [
            (StepCommand::Stop, 0x00),
            (StepCommand::Lower, 0x40),
            (StepCommand::Higher, 0x80),
            (StepCommand::Reserved, 0xC0),
        ];
        for (command, octet) in vectors {
            assert_eq!(
                step_command_to_data(command),
                Ok(Data::BitString(BitString {
                    unused: 6,
                    bytes: vec![octet]
                }))
            );
            assert_eq!(
                step_command_from_data(&step_command_to_data(command).unwrap()),
                Ok(command)
            );
        }
        assert_eq!(StepCommand::Lower.name(), "lower");
        assert_eq!(StepCommand::Higher.value(), 2);
        // The bits past the two of the type are ignored: 01 followed by 1111 is lower.
        let data = Data::BitString(BitString {
            unused: 0,
            bytes: vec![0b0111_1111],
        });
        assert_eq!(step_command_from_data(&data), Ok(StepCommand::Lower));
    }

    #[test]
    fn trigger_options_are_six_bits_with_the_positions_of_table_37() {
        let none = TriggerOptions::default();
        assert_eq!(
            trigger_options_to_data(none),
            Data::BitString(BitString {
                unused: 2,
                bytes: vec![0x00]
            })
        );
        let general = TriggerOptions {
            general_interrogation: true,
            ..TriggerOptions::default()
        };
        assert_eq!(
            trigger_options_to_data(general),
            Data::BitString(BitString {
                unused: 2,
                bytes: vec![0x04]
            })
        );
        let change = TriggerOptions {
            data_change: true,
            ..TriggerOptions::default()
        };
        assert_eq!(
            trigger_options_to_data(change),
            Data::BitString(BitString {
                unused: 2,
                bytes: vec![0x40]
            })
        );
        assert_eq!(
            trigger_options_from_data(&trigger_options_to_data(general)),
            Ok(general)
        );
        // Bits past the six are ignored; bit 5 is general-interrogation.
        let long = Data::BitString(BitString {
            unused: 0,
            bytes: vec![0b0000_0111],
        });
        assert_eq!(trigger_options_from_data(&long), Ok(general));
        // A shorter string takes zero for the missing bits.
        let short = Data::BitString(BitString {
            unused: 4,
            bytes: vec![0b0100_0000],
        });
        assert_eq!(trigger_options_from_data(&short), Ok(change));
        assert_eq!(describe_trigger_options(), describe_bit_string(6));
    }

    #[test]
    fn report_options_are_ten_bits_and_sampled_value_options_seven() {
        // The segmentation is bit 9: the second octet, second bit.
        let segmentation = ReportOptions {
            segmentation: true,
            ..ReportOptions::default()
        };
        assert_eq!(
            report_options_to_data(segmentation),
            Data::BitString(BitString {
                unused: 6,
                bytes: vec![0x00, 0x40]
            })
        );
        // The entryID is bit 7: the first octet, last bit.
        let entry = ReportOptions {
            entry_id: true,
            ..ReportOptions::default()
        };
        assert_eq!(
            report_options_to_data(entry),
            Data::BitString(BitString {
                unused: 6,
                bytes: vec![0x01, 0x00]
            })
        );
        assert_eq!(
            report_options_from_data(&report_options_to_data(entry)),
            Ok(entry)
        );
        // The refresh-time of the sampled values is bit 0, the synch-source-identity bit 6.
        let refresh = SvOptions {
            refresh_time: true,
            ..SvOptions::default()
        };
        assert_eq!(
            sv_options_to_data(refresh),
            Data::BitString(BitString {
                unused: 1,
                bytes: vec![0x80]
            })
        );
        let synch = SvOptions {
            synch_source_identity: true,
            ..SvOptions::default()
        };
        assert_eq!(sv_options_from_data(&sv_options_to_data(synch)), Ok(synch));
        assert_eq!(describe_report_options(), describe_bit_string(10));
        assert_eq!(describe_sv_options(), describe_bit_string(7));
    }

    #[test]
    fn an_entry_id_is_eight_octets_and_any_other_length_is_refused() {
        let id: EntryId = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            entry_id_to_data(&id),
            Data::OctetString(vec![1, 2, 3, 4, 5, 6, 7, 8])
        );
        assert_eq!(entry_id_from_data(&entry_id_to_data(&id)), Ok(id));
        assert!(matches!(
            entry_id_from_data(&Data::OctetString(vec![1; 7])),
            Err(AcsiError::InvalidValue { .. })
        ));
        assert!(matches!(
            entry_id_from_data(&Data::OctetString(vec![1; 9])),
            Err(AcsiError::InvalidValue { .. })
        ));
        assert!(matches!(
            entry_id_from_data(&Data::Integer(1)),
            Err(AcsiError::WrongType { .. })
        ));
        assert_eq!(
            describe_entry_id(),
            TypeDescription::OctetString(Integer32(ENTRY_ID_OCTETS as i64))
        );
    }

    #[test]
    fn an_entry_time_is_the_six_octet_time_of_day_with_its_day() {
        let time = EntryTime {
            milliseconds: 3_600_000,
            day: 5,
        };
        let data = entry_time_to_data(time).unwrap();
        assert_eq!(entry_time_from_data(&data), Ok(time));
        // The NULL entry time is zero in every field.
        let null = EntryTime {
            milliseconds: 0,
            day: 0,
        };
        assert_eq!(
            entry_time_from_data(&entry_time_to_data(null).unwrap()),
            Ok(null)
        );
        // The four-octet form has no day and is refused for an entry time.
        let four = Data::BinaryTime(TimeOfDay::from_milliseconds(1000).unwrap());
        assert!(matches!(
            entry_time_from_data(&four),
            Err(AcsiError::InvalidValue { .. })
        ));
        assert_eq!(describe_entry_time(), TypeDescription::BinaryTime(true));
    }

    #[test]
    fn a_coded_enumeration_is_a_bit_string_most_significant_bit_first() {
        // Two bits, value 3: 11, unused 6 bits of the octet.
        assert_eq!(
            coded_enum_to_data(3, 2),
            Ok(Data::BitString(BitString {
                unused: 6,
                bytes: vec![0xC0]
            }))
        );
        // Value 2 is 10.
        assert_eq!(
            coded_enum_to_data(2, 2),
            Ok(Data::BitString(BitString {
                unused: 6,
                bytes: vec![0x80]
            }))
        );
        assert_eq!(
            coded_enum_from_data(&coded_enum_to_data(3, 2).unwrap(), 2),
            Ok(3)
        );
        // Thirteen bits span two octets.
        let thirteen = coded_enum_to_data(0x1000, 13).unwrap();
        assert_eq!(coded_enum_from_data(&thirteen, 13), Ok(0x1000));
    }

    #[test]
    fn a_coded_enumeration_size_is_the_bits_of_its_maximum() {
        assert_eq!(coded_enum_size(0), 1);
        assert_eq!(coded_enum_size(1), 1);
        assert_eq!(coded_enum_size(2), 2);
        assert_eq!(coded_enum_size(3), 2);
        assert_eq!(coded_enum_size(4), 3);
        assert_eq!(coded_enum_size(8191), 13);
        assert_eq!(coded_enum_size(u32::MAX), 32);
    }

    #[test]
    fn a_coded_enumeration_refuses_a_zero_size_and_a_value_out_of_size() {
        assert!(matches!(
            coded_enum_to_data(0, 0),
            Err(AcsiError::InvalidBits { .. })
        ));
        assert!(matches!(
            coded_enum_to_data(4, 2),
            Err(AcsiError::OutOfRange {
                ty: "coded enumeration",
                ..
            })
        ));
        let empty = Data::BitString(BitString {
            unused: 0,
            bytes: vec![],
        });
        assert!(matches!(
            coded_enum_from_data(&empty, 2),
            Err(AcsiError::InvalidBits { .. })
        ));
    }

    #[test]
    fn a_coded_enumeration_ignores_extra_bits_and_defaults_missing_ones() {
        // A peer sends eight bits 11000000 for a size of two: the first two bits are read.
        let eight = Data::BitString(BitString {
            unused: 0,
            bytes: vec![0xC0],
        });
        assert_eq!(coded_enum_from_data(&eight, 2), Ok(3));
        // A peer sends one bit 1 for a size of two: the second bit defaults to zero, the value is 2.
        let one = Data::BitString(BitString {
            unused: 7,
            bytes: vec![0x80],
        });
        assert_eq!(coded_enum_from_data(&one, 2), Ok(2));
    }

    #[test]
    fn a_timestamp_is_an_utc_time_of_eight_octets() {
        let epoch = UtcTimestamp::from_octets([0; 8]).unwrap();
        assert_eq!(timestamp_to_data(epoch), Data::UtcTime(UtcTime(vec![0; 8])));
        assert_eq!(timestamp_from_data(&timestamp_to_data(epoch)), Ok(epoch));
        assert!(matches!(
            timestamp_from_data(&Data::UtcTime(UtcTime(vec![0; 7]))),
            Err(AcsiError::InvalidValue {
                ty: "Timestamp",
                ..
            })
        ));
        // A time quality of an invalid accuracy (N3, Table 36): refused as the timestamp.
        assert!(matches!(
            timestamp_from_data(&Data::UtcTime(UtcTime(vec![0, 0, 0, 0, 0, 0, 0, 0x19]))),
            Err(AcsiError::InvalidValue {
                ty: "Timestamp",
                ..
            })
        ));
    }

    #[test]
    fn a_good_quality_is_thirteen_zero_bits() {
        let data = Quality::good().to_data();
        assert_eq!(
            data,
            Data::BitString(BitString {
                unused: 3,
                bytes: vec![0, 0]
            })
        );
        assert_eq!(Quality::from_data(&data), Ok(Quality::good()));
    }

    #[test]
    fn each_quality_flag_is_at_its_bit_of_table_44() {
        // Invalid (01) at bits 0 and 1: the first octet 0x40.
        let invalid = Quality {
            validity: Validity::Invalid,
            ..Quality::good()
        };
        assert_eq!(
            invalid.to_data(),
            Data::BitString(BitString {
                unused: 3,
                bytes: vec![0x40, 0]
            })
        );
        // Test at bit 11: the second octet 0x10; operatorBlocked at bit 12: 0x08; substituted at bit 10: 0x20.
        let flags = Quality {
            test: true,
            operator_blocked: true,
            source: Source::Substituted,
            ..Quality::good()
        };
        assert_eq!(
            flags.to_data(),
            Data::BitString(BitString {
                unused: 3,
                bytes: vec![0, 0x38]
            })
        );
        // oldData is bit 7: 0x01 of the first octet.
        let old = Quality {
            detail: DetailQuality {
                old_data: true,
                ..DetailQuality::default()
            },
            ..Quality::good()
        };
        assert_eq!(
            old.to_data(),
            Data::BitString(BitString {
                unused: 3,
                bytes: vec![0x01, 0]
            })
        );
    }

    #[test]
    fn a_quality_round_trips_and_defaults_missing_bits() {
        let quality = Quality {
            validity: Validity::Questionable,
            detail: DetailQuality {
                overflow: true,
                inaccurate: true,
                ..DetailQuality::default()
            },
            source: Source::Substituted,
            test: true,
            operator_blocked: false,
        };
        assert_eq!(Quality::from_data(&quality.to_data()), Ok(quality));
        // A shorter string: the missing bits are good, not set.
        let short = Data::BitString(BitString {
            unused: 6,
            bytes: vec![0x40],
        });
        assert_eq!(
            Quality::from_data(&short).unwrap().validity,
            Validity::Invalid
        );
        assert!(matches!(
            Quality::from_data(&Data::Integer(0)),
            Err(AcsiError::WrongType { ty: "Quality", .. })
        ));
    }

    #[test]
    fn check_conditions_are_two_bits() {
        let both = CheckConditions {
            synchrocheck: true,
            interlock_check: true,
        };
        assert_eq!(
            both.to_data(),
            Data::BitString(BitString {
                unused: 6,
                bytes: vec![0xC0]
            })
        );
        assert_eq!(CheckConditions::from_data(&both.to_data()), Ok(both));
        let interlock = CheckConditions {
            interlock_check: true,
            ..CheckConditions::default()
        };
        assert_eq!(
            interlock.to_data(),
            Data::BitString(BitString {
                unused: 6,
                bytes: vec![0x40]
            })
        );
    }

    #[test]
    fn an_originator_is_a_structure_of_category_and_identity() {
        let origin = Originator {
            category: 1,
            identity: b"bay1".to_vec(),
        };
        let data = origin.to_data().unwrap();
        assert_eq!(
            data,
            Data::Structure(vec![Data::Integer(1), Data::OctetString(b"bay1".to_vec())])
        );
        assert_eq!(Originator::from_data(&data), Ok(origin));
        assert!(matches!(
            Originator {
                category: 1,
                identity: vec![0; 65]
            }
            .to_data(),
            Err(AcsiError::TooLong {
                ty: "Octet string",
                max: 64,
                length: 65
            })
        ));
        assert!(matches!(
            Originator::from_data(&Data::Structure(vec![Data::Integer(1)])),
            Err(AcsiError::InvalidValue {
                ty: "Originator",
                ..
            })
        ));
    }

    #[test]
    fn each_row_of_the_type_table_has_its_type_description() {
        assert_eq!(describe_boolean(), TypeDescription::Boolean);
        assert_eq!(
            describe_signed(SignedInteger::I8),
            TypeDescription::Integer(Unsigned8(8))
        );
        assert_eq!(
            describe_signed(SignedInteger::I64),
            TypeDescription::Integer(Unsigned8(64))
        );
        assert_eq!(
            describe_unsigned(UnsignedInteger::U16),
            TypeDescription::Unsigned(Unsigned8(16))
        );
        assert_eq!(
            describe_float32(),
            TypeDescription::FloatingPoint(TypeDescriptionFloatingPoint {
                format_width: Unsigned8(32),
                exponent_width: Unsigned8(8),
            })
        );
        // Standardised values up to 2 need two bits as a signed integer (§8.1.2.2): 0, 1, 2 fit in 2 bits.
        assert_eq!(
            describe_enumeration(2),
            TypeDescription::Integer(Unsigned8(3))
        );
        assert_eq!(
            describe_enumeration(0),
            TypeDescription::Integer(Unsigned8(1))
        );
        assert_eq!(
            describe_coded_enum(13),
            TypeDescription::BitString(Integer32(13))
        );
        assert_eq!(
            describe_enumeration(i64::from(u32::MAX)),
            TypeDescription::Integer(Unsigned8(33))
        );
        assert_eq!(
            describe_octets(64),
            TypeDescription::OctetString(Integer32(64))
        );
        // A string of at most a given length is a variable length with the maximum: a negative size (A2, D-031).
        assert_eq!(
            describe_visible(64),
            TypeDescription::VisibleString(Integer32(-64))
        );
        assert_eq!(
            describe_unicode(255),
            TypeDescription::MMSString(Integer32(-255))
        );
        assert_eq!(describe_timestamp(), TypeDescription::UtcTime);
        assert_eq!(
            describe_quality(),
            TypeDescription::BitString(Integer32(13))
        );
        assert_eq!(
            describe_check_conditions(),
            TypeDescription::BitString(Integer32(2))
        );
    }
}
