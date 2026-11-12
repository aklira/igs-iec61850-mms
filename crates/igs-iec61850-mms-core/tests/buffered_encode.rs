// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The layout of a buffered report (IEC 61850-8-1 table 74; task B2, BRCB-002): the wire order of the results, the inclusion bit
//! string, the entry identifier, the optional fields that the segmentation and the overflow select, and the round trip of the options.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::BTreeSet;
use std::time::Duration;

use igs_iec61850_mms_core::report::brcb::{BufferedFields, EntryId};
use igs_iec61850_mms_core::report::encode::{
    bit_count, buffered_fields_from_bits, buffered_information_report,
    buffered_optional_fields_bits, BufferedContent, BufferedVariable,
};
use igs_iec61850_mms_core::report::urcb::Reason;
use igs_iso9506_mms_codec::generated::types::Data;

fn all_fields() -> BufferedFields {
    BufferedFields {
        seq_num: true,
        time_stamp: true,
        reason_code: true,
        data_set_name: true,
        data_ref: true,
        buf_ovfl: true,
        entry_id: true,
        conf_rev: true,
        segmentation: true,
    }
}

/// Two variables of a data set of three: the first and the third are in the report.
fn variables() -> Vec<BufferedVariable> {
    vec![
        BufferedVariable {
            position: 0,
            reference: "LD1/GGIO1.Ind1.stVal".to_string(),
            value: Data::Boolean(true),
            reasons: BTreeSet::from([Reason::DataChange]),
        },
        BufferedVariable {
            position: 2,
            reference: "LD1/GGIO1.Ind2.stVal".to_string(),
            value: Data::Boolean(false),
            reasons: BTreeSet::from([Reason::QualityChange, Reason::Integrity]),
        },
    ]
}

fn content<'a>(fields: BufferedFields, variables: &'a [BufferedVariable]) -> BufferedContent<'a> {
    BufferedContent {
        rpt_id: "RPT1",
        block_reference: "LD1/GGIO1.RP1",
        data_set: "DS1",
        conf_rev: 7,
        opt_flds: fields,
        sequence: 300,
        sub_sequence: 2,
        more_segments: true,
        buf_ovfl: true,
        time_of_entry: Duration::from_millis(1_000),
        entry_id: EntryId(42),
        variable_count: 3,
        variables,
    }
}

fn kinds(results: &[Data]) -> Vec<&'static str> {
    results
        .iter()
        .map(|result| match result {
            Data::VisibleString(_) => "string",
            Data::BitString(_) => "bits",
            Data::Unsigned(_) => "unsigned",
            Data::Boolean(_) => "boolean",
            Data::BinaryTime(_) => "time",
            Data::OctetString(_) => "octets",
            _ => "other",
        })
        .collect()
}

/// The kinds of the results, in the wire order of table 74 of 8-1, with every option set.
#[test]
fn the_results_are_in_the_wire_order_of_table_74() {
    let values = variables();
    let results = buffered_information_report(&content(all_fields(), &values));
    assert_eq!(
        kinds(&results),
        [
            // RptID, OptFlds, SqNum, TimeOfEntry, DatSet, BufOvfl, EntryID, ConfRev, SubSeqNum, MoreSegmentsFollow,
            "string", "bits", "unsigned", "time", "string", "boolean", "octets", "unsigned",
            "unsigned", "boolean",
            // the inclusion bit string, DataRef x2, Value x2, ReasonCode x2
            "bits", "string", "string", "boolean", "boolean", "bits", "bits",
        ]
    );
}

/// The inclusion bit string has one bit per variable of the data set, set for the variables of the report.
#[test]
fn the_inclusion_bit_string_marks_the_variables_of_the_report() {
    let values = variables();
    let results = buffered_information_report(&content(all_fields(), &values));
    let Some(Data::BitString(inclusion)) = results.get(10) else {
        panic!("the inclusion bit string is at position 10");
    };
    assert_eq!(bit_count(inclusion), 3);
    assert!(inclusion.bit(0) && !inclusion.bit(1) && inclusion.bit(2));
}

/// The entry identifier is the eight octets of its value, big-endian; the NULL identifier is eight zero octets.
#[test]
fn the_entry_identifier_is_eight_octets_and_the_null_one_is_zero() {
    let values = variables();
    let results = buffered_information_report(&content(all_fields(), &values));
    assert!(results.contains(&Data::OctetString(vec![0, 0, 0, 0, 0, 0, 0, 42])));

    let mut null = content(all_fields(), &values);
    null.entry_id = EntryId::NULL;
    let results = buffered_information_report(&null);
    assert!(results.contains(&Data::OctetString(vec![0; 8])));
}

/// The fields that the options do not select are not in the report: no time, no overflow, no identifier, no segmentation.
#[test]
fn an_unselected_field_is_omitted() {
    let values = variables();
    let fields = BufferedFields {
        seq_num: true,
        ..Default::default()
    };
    let results = buffered_information_report(&content(fields, &values));
    // RptID, OptFlds, SqNum, the inclusion bit string, then the two values.
    assert_eq!(
        kinds(&results),
        ["string", "bits", "unsigned", "bits", "boolean", "boolean"]
    );
    assert!(matches!(results.get(4), Some(Data::Boolean(true))));
}

/// The options round-trip: every combination of the nine options is the same after the bit string and back.
#[test]
fn the_options_round_trip_through_the_bit_string() {
    for combination in 0u16..512 {
        let bit = |position: u16| combination & (1 << position) != 0;
        let fields = BufferedFields {
            seq_num: bit(0),
            time_stamp: bit(1),
            reason_code: bit(2),
            data_set_name: bit(3),
            data_ref: bit(4),
            buf_ovfl: bit(5),
            entry_id: bit(6),
            conf_rev: bit(7),
            segmentation: bit(8),
        };
        let bits = buffered_optional_fields_bits(fields);
        assert_eq!(
            buffered_fields_from_bits(&bits),
            fields,
            "combination {combination}"
        );
        assert_eq!(bits.unused, 6, "ten bits, the last octet has six unused");
    }
}
