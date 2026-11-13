// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Type resolution: the type names of the catalogue, as the attributes of the namespaces give them, to MMS
//! type descriptions (IEC 61850-8-1 §8.1.1, §8.1.2 and §8.1.3; task A3).
//!
//! Only the types with a fixed MMS form are resolved here. A name that is not in the table is refused by the
//! caller with a typed error, never mapped to a guess: the coded enumerations of the reporting and control
//! blocks, the physical address and the entry time wait for their own sources (`docs/capabilities.md`).

use igs_iso9506_mms_codec::mms::TypeDescription;

use crate::acsi::{
    describe_boolean, describe_check_conditions, describe_coded_enum, describe_entry_id,
    describe_entry_time, describe_enumeration, describe_float32, describe_octets, describe_quality,
    describe_report_options, describe_signed, describe_sv_options, describe_timestamp,
    describe_trigger_options, describe_unicode, describe_unsigned, describe_visible, SignedInteger,
    UnsignedInteger, DOUBLE_POSITION_SIZE, STEP_COMMAND_SIZE,
};
use crate::catalogue;

/// The MMS type of a basic type of 7-2 with a fixed form. `None` for a name that is not resolved here.
pub fn basic(name: &str) -> Option<TypeDescription> {
    Some(match name {
        "BOOLEAN" => describe_boolean(),
        "INT8" => describe_signed(SignedInteger::I8),
        "INT16" => describe_signed(SignedInteger::I16),
        "INT32" => describe_signed(SignedInteger::I32),
        "INT64" => describe_signed(SignedInteger::I64),
        "INT8U" => describe_unsigned(UnsignedInteger::U8),
        "INT16U" => describe_unsigned(UnsignedInteger::U16),
        "INT32U" => describe_unsigned(UnsignedInteger::U32),
        "FLOAT32" => describe_float32(),
        "Octet64" => describe_octets(64),
        "VisString64" => describe_visible(64),
        "VisString129" => describe_visible(129),
        "VisString255" => describe_visible(255),
        // The object reference is a visible string of 129 characters (8-1 §8.1.3.2, A1).
        "ObjRef" => describe_visible(129),
        // The currency code has three characters (8-1 §8.1.3.18).
        "Currency" => describe_visible(3),
        "Unicode255" => describe_unicode(255),
        "Timestamp" => describe_timestamp(),
        "Quality" => describe_quality(),
        "Check" => describe_check_conditions(),
        // The double-point status: a coded enumeration of two bits (7-2 Table 24, 8-1 §8.1.2.3).
        "Dbpos" => describe_coded_enum(DOUBLE_POSITION_SIZE),
        // The step command of a tap changer: a coded enumeration of two bits (7-2 Table 23).
        "Tcmd" => describe_coded_enum(STEP_COMMAND_SIZE),
        // The trigger conditions, the report and the sampled value options: bit strings of their minimum size
        // (8-1 §8.1.3.9, §8.1.3.13, §8.1.3.15).
        "TrgOps" => describe_trigger_options(),
        "OptFlds" => describe_report_options(),
        "SvOptFlds" => describe_sv_options(),
        // The entry identifier and the entry time (8-1 §8.1.3.5, §8.1.3.8).
        "EntryID" => describe_entry_id(),
        "EntryTime" => describe_entry_time(),
        // PhyComAddr stays unmapped: its form depends on the protocol of the multicast traffic (Layer 2 or UDP/IP,
        // 8-1 Tables 16 and 40), and that traffic is not in the scope of the library.
        _ => return None,
    })
}

/// The MMS type of an enumeration of the catalogue: an integer that holds its largest standardised value
/// (8-1 §8.1.2.2). `None` for a name that the catalogue does not define as an enumeration.
pub fn enumeration(name: &str) -> Option<TypeDescription> {
    let definition = catalogue::enumeration(name)?;
    let largest = definition
        .literals
        .iter()
        .map(|literal| i64::from(literal.value))
        .max()
        .unwrap_or(0);
    Some(describe_enumeration(largest))
}

/// The values that an enumeration of the catalogue defines, in the namespace order. `None` for a name that the
/// catalogue does not define as an enumeration.
pub fn enumeration_values(name: &str) -> Option<Vec<i64>> {
    let definition = catalogue::enumeration(name)?;
    Some(
        definition
            .literals
            .iter()
            .map(|literal| i64::from(literal.value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use igs_iso9506_mms_codec::mms::TypeDescription;

    use super::basic;
    use crate::acsi::{describe_coded_enum, DOUBLE_POSITION_SIZE, STEP_COMMAND_SIZE};

    #[test]
    fn the_double_point_and_the_step_command_are_two_bit_coded_enumerations() {
        assert_eq!(
            basic("Dbpos"),
            Some(describe_coded_enum(DOUBLE_POSITION_SIZE))
        );
        assert_eq!(basic("Tcmd"), Some(describe_coded_enum(STEP_COMMAND_SIZE)));
        assert_eq!(DOUBLE_POSITION_SIZE, 2);
        assert_eq!(STEP_COMMAND_SIZE, 2);
        // The types still unmapped are refused, never guessed.
        assert_eq!(basic("PhyComAddr"), None);
        assert!(matches!(
            basic("Dbpos"),
            Some(TypeDescription::BitString(_))
        ));
    }
}
