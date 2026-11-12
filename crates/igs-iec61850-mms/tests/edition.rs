// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The editions of IEC 61850-7-2 that a client meets (task E2): the additional causes 26 and 27 read in the first and in the second edition.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use igs_iec61850_mms::client_control::{cause_name_in, termination_in, Edition, Termination};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, Identifier, InformationReport, ObjectName, UnconfirmedService,
    VariableAccessSpecification,
};

/// A `LastApplError` with the additional cause `cause` (table 109 of 8-1).
fn last_error(cause: i128) -> UnconfirmedService {
    UnconfirmedService::InformationReport(InformationReport {
        variable_access_specification: VariableAccessSpecification::VariableListName(
            ObjectName::VmdSpecific(Identifier("LastApplError".to_string())),
        ),
        list_of_access_result: vec![AccessResult::Success(Data::Structure(vec![
            Data::VisibleString("LD1/LLN0$CO$Mod".to_string()),
            Data::Integer(0),
            Data::Structure(vec![Data::Integer(1), Data::OctetString(vec![1])]),
            Data::Unsigned(4),
            Data::Integer(cause),
        ]))],
    })
}

#[test]
fn the_causes_26_and_27_are_read_in_the_first_edition_as_its_tissues_give_them() {
    assert_eq!(
        cause_name_in(Edition::One, 26),
        Some("Locked-by-other-client")
    );
    assert_eq!(
        cause_name_in(Edition::One, 27),
        Some("Inconsistent-parameters")
    );
    assert_eq!(
        cause_name_in(Edition::Two, 26),
        Some("Inconsistent-parameters")
    );
    assert_eq!(
        cause_name_in(Edition::Two, 27),
        Some("Locked-by-other-client")
    );
}

#[test]
fn a_termination_of_the_first_edition_names_its_cause_with_its_own_table() {
    let report = last_error(26);
    match termination_in(Edition::One, &report).expect("a negative termination") {
        Termination::Negative {
            cause, cause_name, ..
        } => {
            assert_eq!(cause, 26);
            assert_eq!(cause_name, Some("Locked-by-other-client"));
        }
        other => panic!("a negative termination is expected: {other:?}"),
    }
    match termination_in(Edition::Two, &report).expect("a negative termination") {
        Termination::Negative { cause_name, .. } => {
            assert_eq!(cause_name, Some("Inconsistent-parameters"))
        }
        other => panic!("a negative termination is expected: {other:?}"),
    }
}

#[test]
fn the_other_causes_are_the_same_in_both_editions() {
    assert_eq!(
        cause_name_in(Edition::One, 0),
        cause_name_in(Edition::Two, 0)
    );
    assert_eq!(
        cause_name_in(Edition::One, 1),
        cause_name_in(Edition::Two, 1)
    );
    assert_eq!(Edition::default(), Edition::Two);
}
