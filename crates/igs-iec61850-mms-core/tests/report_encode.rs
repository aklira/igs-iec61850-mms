// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The layout of a report in MMS: the vectors of table 74, the bit positions of tables 38 and 41, the binary time and the object
//! references (task D4; RPT-002).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::BTreeSet;
use std::time::Duration;

use igs_iec61850_mms_core::report::encode::{
    binary_time, information_report, object_reference, Content, Variable,
};
use igs_iec61850_mms_core::report::urcb::{Entry, OptionalFields, Reason, Report, ReportKind};
use igs_iso9506_mms_codec::generated::types::{Data, TimeOfDay};
use igs_iso9506_mms_codec::schema::BitString;

fn variable(member: usize, reference: &str, value: Data) -> Variable {
    Variable {
        member,
        reference: reference.to_string(),
        value,
    }
}

fn report(kind: ReportKind, members: &[(usize, &[Reason])]) -> Report {
    Report {
        at: Duration::ZERO,
        sequence: 7,
        kind,
        entries: members
            .iter()
            .map(|(member, reasons)| Entry {
                member: *member,
                reasons: reasons.iter().copied().collect::<BTreeSet<_>>(),
            })
            .collect(),
    }
}

fn content<'a>(variables: &'a [Variable], opt_flds: OptionalFields) -> Content<'a> {
    Content {
        rpt_id: "RPT1",
        block_reference: "LD1/LLN0.RP.RPT1",
        data_set: "DS1",
        conf_rev: 3,
        opt_flds,
        time_of_entry: Duration::from_millis(1_500),
        sequence: 0,
        variables,
    }
}

fn bitstring(data: &Data) -> &BitString {
    match data {
        Data::BitString(bits) => bits,
        other => panic!("expected a bit string, got {other:?}"),
    }
}

#[test]
fn the_reported_options_have_their_bits_of_table_41() {
    let variables = [variable(0, "LD1/GGIO1.Ind1.stVal", Data::Boolean(true))];
    let flags = OptionalFields {
        seq_num: true,
        time_stamp: true,
        reason_code: true,
        data_set_name: true,
        data_ref: true,
        conf_rev: true,
    };
    let results = information_report(
        &content(&variables, flags),
        &report(ReportKind::Notification, &[(0, &[Reason::DataChange])]),
    );
    // Bits 1, 2, 3, 4, 5 and 8 of a ten-bit string: 0111 1100, then 1000 0000 with six unused bits.
    let options = bitstring(&results[1]);
    assert_eq!(options.bytes, [0x7C, 0x80]);
    assert_eq!(options.unused, 6);
}

#[test]
fn the_results_are_in_the_order_of_table_74() {
    let variables = [
        variable(0, "LD1/GGIO1.Ind1.stVal", Data::Boolean(true)),
        variable(1, "LD1/GGIO1.Ind1.q", Data::Boolean(false)),
        variable(2, "LD1/GGIO1.Ind2.stVal", Data::Boolean(false)),
    ];
    let flags = OptionalFields {
        seq_num: true,
        time_stamp: true,
        reason_code: true,
        data_set_name: true,
        data_ref: true,
        conf_rev: true,
    };
    let results = information_report(
        &content(&variables, flags),
        &report(
            ReportKind::Notification,
            &[(0, &[Reason::DataChange]), (2, &[Reason::QualityChange])],
        ),
    );
    // RptID, options, SeqNum, TimeOfEntry, DatSet, ConfRev, inclusion, two references, two values, two reasons.
    assert_eq!(results.len(), 1 + 1 + 1 + 1 + 1 + 1 + 1 + 2 + 2 + 2);
    assert!(matches!(&results[0], Data::VisibleString(id) if id == "RPT1"));
    assert!(
        matches!(results[2], Data::Unsigned(7)),
        "the sequence number of the report"
    );
    assert!(matches!(&results[3], Data::BinaryTime(TimeOfDay(octets)) if octets.len() == 6));
    assert!(matches!(&results[4], Data::VisibleString(name) if name == "DS1"));
    assert!(
        matches!(results[5], Data::Unsigned(3)),
        "the configuration revision"
    );
    assert!(matches!(&results[7], Data::VisibleString(r) if r == "LD1/GGIO1.Ind1.stVal"));
    assert!(matches!(&results[8], Data::VisibleString(r) if r == "LD1/GGIO1.Ind2.stVal"));
    assert!(matches!(results[9], Data::Boolean(true)));
    assert!(matches!(results[10], Data::Boolean(false)));
    assert!(matches!(results[11], Data::BitString(_)));
    assert!(matches!(results[12], Data::BitString(_)));
}

#[test]
fn the_inclusion_bit_string_has_one_bit_per_variable_of_the_data_set() {
    let variables = [
        variable(0, "LD1/GGIO1.Ind1.stVal", Data::Boolean(true)),
        variable(1, "LD1/GGIO1.Ind1.q", Data::Boolean(false)),
        variable(2, "LD1/GGIO1.Ind2.stVal", Data::Boolean(false)),
    ];
    let results = information_report(
        &content(&variables, OptionalFields::default()),
        &report(
            ReportKind::Notification,
            &[(0, &[Reason::DataChange]), (2, &[Reason::DataChange])],
        ),
    );
    // Variables 0 and 2 are included: 1 0 1, five unused bits.
    let inclusion = bitstring(&results[2]);
    assert_eq!(inclusion.bytes, [0xA0]);
    assert_eq!(inclusion.unused, 5);
    assert_eq!(
        results.len(),
        2 + 1 + 2,
        "the options, the inclusion, two values"
    );
}

#[test]
fn an_integrity_report_includes_every_variable_with_its_reason() {
    let variables = [
        variable(0, "LD1/GGIO1.Ind1.stVal", Data::Boolean(true)),
        variable(1, "LD1/GGIO1.Ind2.stVal", Data::Boolean(false)),
    ];
    let flags = OptionalFields {
        reason_code: true,
        ..Default::default()
    };
    let results = information_report(
        &content(&variables, flags),
        &report(
            ReportKind::Integrity,
            &[(0, &[Reason::Integrity]), (1, &[Reason::Integrity])],
        ),
    );
    assert_eq!(bitstring(&results[2]).bytes, [0xC0]);
    // The reason of each value: bit 4 (integrity) of a six-bit string is 0000 1000 with two unused bits.
    let reason = bitstring(&results[results.len() - 1]);
    assert_eq!(reason.bytes, [0x08]);
    assert_eq!(reason.unused, 2);
}

#[test]
fn the_reasons_have_their_bits_of_table_38() {
    let variables = [variable(0, "LD1/GGIO1.Ind1.stVal", Data::Boolean(true))];
    let flags = OptionalFields {
        reason_code: true,
        ..Default::default()
    };
    let results = information_report(
        &content(&variables, flags),
        &report(
            ReportKind::GeneralInterrogation,
            &[(0, &[Reason::DataChange, Reason::GeneralInterrogation])],
        ),
    );
    // Bit 1 (data-change) and bit 5 (general-interrogation): 0100 0100, two unused bits.
    let reason = bitstring(&results[results.len() - 1]);
    assert_eq!(reason.bytes, [0x44]);
    assert_eq!(reason.unused, 2);
}

#[test]
fn the_binary_time_is_four_octets_of_time_of_day_then_two_of_days() {
    assert_eq!(binary_time(Duration::ZERO).0, [0, 0, 0, 0, 0, 0]);
    // One day and one and a half seconds after 1984-01-01: 1500 ms into the day, one day.
    let time = binary_time(Duration::from_millis(86_400_000 + 1_500));
    assert_eq!(time.0, [0x00, 0x00, 0x05, 0xDC, 0x00, 0x01]);
}

#[test]
fn an_object_reference_drops_the_constraint_and_joins_the_parts_with_dots() {
    assert_eq!(
        object_reference("LD1", "GGIO1$ST$Ind1$stVal"),
        "LD1/GGIO1.Ind1.stVal"
    );
    assert_eq!(
        object_reference("LD1", "MMXU1$MX$TotW$mag$f"),
        "LD1/MMXU1.TotW.mag.f"
    );
    assert_eq!(object_reference("LD1", "GGIO1"), "LD1/GGIO1");
}

#[test]
fn an_empty_identifier_is_replaced_by_the_block_reference() {
    let variables: [Variable; 0] = [];
    let mut block = content(&variables, OptionalFields::default());
    block.rpt_id = "";
    let results = information_report(&block, &report(ReportKind::Notification, &[]));
    assert!(matches!(&results[0], Data::VisibleString(r) if r == "LD1/LLN0.RP.RPT1"));
}

#[test]
fn the_options_triggers_and_reasons_decode_what_they_encode() {
    use igs_iec61850_mms_core::report::encode::{
        optional_fields_bits, optional_fields_from_bits, reasons_from_bits, triggers_from_bits,
        triggers_to_bits,
    };
    use igs_iec61850_mms_core::report::urcb::TriggerOptions;
    let fields = OptionalFields {
        seq_num: true,
        time_stamp: false,
        reason_code: true,
        data_set_name: true,
        data_ref: false,
        conf_rev: true,
    };
    assert_eq!(
        optional_fields_from_bits(&optional_fields_bits(fields)),
        fields
    );
    let triggers = TriggerOptions {
        data_change: true,
        quality_change: false,
        data_update: true,
        integrity: true,
        general_interrogation: false,
    };
    assert_eq!(triggers_from_bits(&triggers_to_bits(triggers)), triggers);
    assert_eq!(
        reasons_from_bits(&triggers_to_bits(triggers)),
        [Reason::DataChange, Reason::DataUpdate, Reason::Integrity]
    );
}
