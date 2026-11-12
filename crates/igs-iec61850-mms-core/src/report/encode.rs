// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The layout of a report in MMS (IEC 61850-8-1 §17.2.1, table 74 and clauses 8.1.3.10 and 8.1.3.13; task D4, RPT-002).
//!
//! A report is one `InformationReport` whose variable list is the VMD-specific `RPT`, followed by the results of this module in
//! the order of table 74: `RptID`, the reported options, the sequence number, the time of entry, the data set name, the
//! configuration revision, the inclusion bit string (one bit per variable of the data set), the data references, the values,
//! and the reasons for inclusion. This module builds those results; the PDU and its delivery are the crate that serves them.
//!
//! The unbuffered block reports neither the buffer overflow nor the entry identifier, and no segmentation: those option bits
//! are always zero in the reported options (8-1 table 73 and 17.2.5.2).

use std::time::Duration;

use igs_iso9506_mms_codec::generated::types::{Data, TimeOfDay};
use igs_iso9506_mms_codec::schema::BitString;

use super::urcb::{OptionalFields, Reason, Report, TriggerOptions};

/// The variable name of the report in the MMS PDU: the VMD-specific list `RPT` (8-1 17.2.1).
pub const REPORT_LIST: &str = "RPT";

/// Milliseconds in a day.
const DAY_MS: u64 = 86_400_000;

/// One variable of the data set, in the order of the list: the member it belongs to, its object reference, and its value.
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    /// The index of the member of the data set (the FCDA) that selected the variable.
    pub member: usize,
    /// The object reference of the variable (see [`object_reference`]).
    pub reference: String,
    /// The value of the variable, at the time of the report.
    pub value: Data,
}

/// What a report is built from: the block's identity and options, and the variables of its data set.
#[derive(Debug, Clone, PartialEq)]
pub struct Content<'a> {
    /// The report identifier (`RptID`). When empty, the reference of the block is reported instead (8-1 17.2.1).
    pub rpt_id: &'a str,
    /// The object reference of the block, used when the identifier is empty.
    pub block_reference: &'a str,
    /// The name of the data set.
    pub data_set: &'a str,
    /// The configuration revision.
    pub conf_rev: u32,
    /// The optional fields of the block.
    pub opt_flds: OptionalFields,
    /// The time of the report, as the time since 1 January 1984 (the binary time of MMS counts from there).
    pub time_of_entry: Duration,
    /// The sequence number of the report (`SqNum`).
    pub sequence: u8,
    /// The variables of the data set, in the order of the list.
    pub variables: &'a [Variable],
}

/// The results of an `InformationReport` for a report of the block, in the order of table 74.
///
/// A variable is included when its member is an entry of the report; an integrity or general-interrogation report includes
/// every member. The reason for inclusion of each included value is the reasons of its member.
pub fn information_report(content: &Content<'_>, report: &Report) -> Vec<Data> {
    let included: Vec<bool> = content
        .variables
        .iter()
        .map(|variable| {
            report
                .entries
                .iter()
                .any(|entry| entry.member == variable.member)
        })
        .collect();
    let mut results = Vec::new();
    results.push(Data::VisibleString(if content.rpt_id.is_empty() {
        content.block_reference.to_string()
    } else {
        content.rpt_id.to_string()
    }));
    results.push(Data::BitString(optional_fields_bits(content.opt_flds)));
    if content.opt_flds.seq_num {
        results.push(Data::Unsigned(u128::from(report.sequence)));
    }
    if content.opt_flds.time_stamp {
        results.push(Data::BinaryTime(binary_time(content.time_of_entry)));
    }
    if content.opt_flds.data_set_name {
        results.push(Data::VisibleString(content.data_set.to_string()));
    }
    if content.opt_flds.conf_rev {
        results.push(Data::Unsigned(u128::from(content.conf_rev)));
    }
    results.push(Data::BitString(bits(
        included.len(),
        included
            .iter()
            .enumerate()
            .filter(|(_, &set)| set)
            .map(|(index, _)| index),
    )));
    let selected: Vec<&Variable> = content
        .variables
        .iter()
        .zip(&included)
        .filter(|(_, &set)| set)
        .map(|(variable, _)| variable)
        .collect();
    if content.opt_flds.data_ref {
        for variable in &selected {
            results.push(Data::VisibleString(variable.reference.clone()));
        }
    }
    for variable in &selected {
        results.push(variable.value.clone());
    }
    if content.opt_flds.reason_code {
        for variable in &selected {
            let reasons = report
                .entries
                .iter()
                .find(|entry| entry.member == variable.member)
                .map(|entry| entry.reasons.iter().copied().collect::<Vec<Reason>>())
                .unwrap_or_default();
            results.push(Data::BitString(reason_bits(&reasons)));
        }
    }
    results
}

/// The object reference of a variable of a data set: `LD/LN.DO.DA` (the functional constraint is not part of it). The
/// variable name is the served one, `LN$FC$DO$DA…`.
pub fn object_reference(device: &str, variable: &str) -> String {
    let mut parts = variable.split('$');
    let node = parts.next().unwrap_or_default();
    let _constraint = parts.next();
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        format!("{device}/{node}")
    } else {
        format!("{device}/{node}.{}", rest.join("."))
    }
}

/// The binary time of MMS for a time since 1 January 1984: four octets of the time of day in milliseconds, then two octets
/// of days, both big-endian (ISO 9506-2 binary time, the six-octet form).
pub fn binary_time(since_1984: Duration) -> TimeOfDay {
    let millis = u64::try_from(since_1984.as_millis()).unwrap_or(u64::MAX);
    let time_of_day = u32::try_from(millis % DAY_MS).unwrap_or(0);
    let days = u16::try_from(millis / DAY_MS).unwrap_or(u16::MAX);
    let mut octets = time_of_day.to_be_bytes().to_vec();
    octets.extend_from_slice(&days.to_be_bytes());
    TimeOfDay(octets)
}

/// The reported options as a bit string: the bits of table 41, with buffer-overflow, entry identifier and segmentation
/// always zero, and the minimum of ten bits.
pub fn optional_fields_bits(fields: OptionalFields) -> BitString {
    let mut set = Vec::new();
    if fields.seq_num {
        set.push(1);
    }
    if fields.time_stamp {
        set.push(2);
    }
    if fields.reason_code {
        set.push(3);
    }
    if fields.data_set_name {
        set.push(4);
    }
    if fields.data_ref {
        set.push(5);
    }
    if fields.conf_rev {
        set.push(8);
    }
    bits(10, set.into_iter())
}

/// The reported options of a bit string (table 41): the inverse of [`optional_fields_bits`].
pub fn optional_fields_from_bits(bits: &BitString) -> OptionalFields {
    OptionalFields {
        seq_num: bits.bit(1),
        time_stamp: bits.bit(2),
        reason_code: bits.bit(3),
        data_set_name: bits.bit(4),
        data_ref: bits.bit(5),
        conf_rev: bits.bit(8),
    }
}

/// The trigger options as a bit string (table 37: data-change 1, quality-change 2, data-update 3, integrity 4, general-interrogation 5).
pub fn triggers_to_bits(options: TriggerOptions) -> BitString {
    let mut set = Vec::new();
    if options.data_change {
        set.push(1);
    }
    if options.quality_change {
        set.push(2);
    }
    if options.data_update {
        set.push(3);
    }
    if options.integrity {
        set.push(4);
    }
    if options.general_interrogation {
        set.push(5);
    }
    bits(6, set.into_iter())
}

/// The trigger options of a bit string (table 37): the inverse of [`triggers_to_bits`].
pub fn triggers_from_bits(bits: &BitString) -> TriggerOptions {
    TriggerOptions {
        data_change: bits.bit(1),
        quality_change: bits.bit(2),
        data_update: bits.bit(3),
        integrity: bits.bit(4),
        general_interrogation: bits.bit(5),
    }
}

/// The reasons for inclusion of a bit string (table 38), in the order of the table.
pub fn reasons_from_bits(bits: &BitString) -> Vec<Reason> {
    let mut reasons = Vec::new();
    if bits.bit(1) {
        reasons.push(Reason::DataChange);
    }
    if bits.bit(2) {
        reasons.push(Reason::QualityChange);
    }
    if bits.bit(3) {
        reasons.push(Reason::DataUpdate);
    }
    if bits.bit(4) {
        reasons.push(Reason::Integrity);
    }
    if bits.bit(5) {
        reasons.push(Reason::GeneralInterrogation);
    }
    reasons
}

/// The number of bits of a bit string (its octets, less the unused bits of the last one).
pub fn bit_count(bits: &BitString) -> usize {
    bits.bytes
        .len()
        .saturating_mul(8)
        .saturating_sub(usize::from(bits.unused))
}

/// The reasons for inclusion as a bit string: the bits of table 38 (data-change 1 … general-interrogation 5), at least six.
fn reason_bits(reasons: &[Reason]) -> BitString {
    let positions = reasons.iter().map(|reason| match reason {
        Reason::DataChange => 1,
        Reason::QualityChange => 2,
        Reason::DataUpdate => 3,
        Reason::Integrity => 4,
        Reason::GeneralInterrogation => 5,
    });
    bits(6, positions)
}

/// A bit string of `length` bits, with the bits at `set` positions set, as MMS encodes it (bit 0 is the leading bit).
fn bits(length: usize, set: impl Iterator<Item = usize>) -> BitString {
    let octets = length.div_ceil(8);
    let mut bytes = vec![0u8; octets];
    for position in set {
        if position < length {
            if let Some(byte) = bytes.get_mut(position / 8) {
                *byte |= 0x80 >> (position % 8);
            }
        }
    }
    let unused = u8::try_from(octets.saturating_mul(8).saturating_sub(length)).unwrap_or(0);
    BitString { unused, bytes }
}
