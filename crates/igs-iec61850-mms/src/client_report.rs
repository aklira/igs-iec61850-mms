// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The client side of the unbuffered reports (task D5; RPT-001, RPT-002): the typed configuration and reading of a block, and the
//! decoding of its reports, in the layout of IEC 61850-8-1 §17.2.1 (table 74).
//!
//! A block is configured through its attributes, which are MMS named variables `node$RP$block$attribute` (8-1 table 73): a client
//! reserves it, sets its data set, its options and its triggers while it is disabled, and enables it. The server sends the reports
//! of an enabled block to the client that holds it, and nothing to another client (RPT-003).

use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, Identifier, ObjectName, UnconfirmedService, VariableAccessSpecification,
};

use igs_iec61850_mms_core::report::encode::{
    bit_count, optional_fields_bits, optional_fields_from_bits, reasons_from_bits,
    triggers_from_bits, triggers_to_bits,
};
use igs_iec61850_mms_core::report::urcb::{OptionalFields, Reason, TriggerOptions};

use crate::client::{ClientError, IecClient, ItemError};
use crate::client_control::component_item;

/// The attributes of a block that a client reads (table 73, without the owner and the general interrogation).
const READ_ATTRIBUTES: [&str; 10] = [
    "RptID", "RptEna", "Resv", "DatSet", "ConfRev", "OptFlds", "BufTm", "SqNum", "TrgOps", "IntgPd",
];

/// The values of an unbuffered block, as its attributes give them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrcbValues {
    /// The report identifier.
    pub rpt_id: String,
    /// Whether the block is enabled.
    pub enabled: bool,
    /// Whether the block is reserved (by any client).
    pub reserved: bool,
    /// The data set (`None` for the NULL value).
    pub data_set: Option<String>,
    /// The configuration revision.
    pub conf_rev: u32,
    /// The reported options.
    pub options: OptionalFields,
    /// The buffer time, in milliseconds.
    pub buf_tm_ms: u32,
    /// The sequence number of the next report.
    pub sequence: u8,
    /// The trigger options.
    pub triggers: TriggerOptions,
    /// The integrity period, in milliseconds (zero: none).
    pub intg_pd_ms: u32,
}

/// A write of a client to a block (the attributes it may write, and the general interrogation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrcbWrite {
    /// Reserve (`true`) or release (`false`) the block.
    Reserve(bool),
    /// Enable (`true`) or disable (`false`) the block.
    Enable(bool),
    /// The report identifier.
    RptId(String),
    /// The data set (`None` for the NULL value).
    DataSet(Option<String>),
    /// The reported options.
    OptFields(OptionalFields),
    /// The buffer time, in milliseconds.
    BufTm(u32),
    /// The trigger options.
    TriggerOptions(TriggerOptions),
    /// The integrity period, in milliseconds (zero: none).
    IntgPd(u32),
    /// A general interrogation.
    GeneralInterrogation,
}

/// An entry of a decoded report: the variable of the data set (its position in the list), its object reference when the report
/// carries it, its value, and the reasons for its inclusion when the report carries them.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedEntry {
    /// The position of the variable in the data set.
    pub index: usize,
    /// The object reference of the variable, when the report has the data references.
    pub reference: Option<String>,
    /// The value of the variable.
    pub value: Data,
    /// The reasons for the inclusion, when the report has them.
    pub reasons: Vec<Reason>,
}

/// A decoded report of an unbuffered block.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedReport {
    /// The report identifier.
    pub rpt_id: String,
    /// The sequence number, when the report carries it.
    pub sequence: Option<u8>,
    /// The data set, when the report carries its name.
    pub data_set: Option<String>,
    /// The configuration revision, when the report carries it.
    pub conf_rev: Option<u32>,
    /// The time of entry (six octets of the binary time), when the report carries it.
    pub time_of_entry: Option<Vec<u8>>,
    /// The entries, in the order of the variables of the data set.
    pub entries: Vec<DecodedEntry>,
}

/// The report of an `InformationReport` on the list `RPT`, decoded in the order of table 74. `None` when the message is not a report.
pub fn decode_report(report: &UnconfirmedService) -> Option<DecodedReport> {
    let UnconfirmedService::InformationReport(info) = report else {
        return None;
    };
    match &info.variable_access_specification {
        VariableAccessSpecification::VariableListName(ObjectName::VmdSpecific(Identifier(
            name,
        ))) if name == "RPT" => {}
        _ => return None,
    }
    let mut values = Vec::with_capacity(info.list_of_access_result.len());
    for result in &info.list_of_access_result {
        match result {
            AccessResult::Success(data) => values.push(data.clone()),
            AccessResult::Failure(_) => return None,
        }
    }
    let mut values = values.into_iter();
    let Some(Data::VisibleString(rpt_id)) = values.next() else {
        return None;
    };
    let Some(Data::BitString(option_bits)) = values.next() else {
        return None;
    };
    let options = optional_fields_from_bits(&option_bits);
    let sequence = if options.seq_num {
        match values.next()? {
            Data::Unsigned(number) => Some(u8::try_from(number).ok()?),
            _ => return None,
        }
    } else {
        None
    };
    let time_of_entry = if options.time_stamp {
        match values.next()? {
            Data::BinaryTime(time) => Some(time.0),
            _ => return None,
        }
    } else {
        None
    };
    let data_set = if options.data_set_name {
        match values.next()? {
            Data::VisibleString(name) => Some(name),
            _ => return None,
        }
    } else {
        None
    };
    let conf_rev = if options.conf_rev {
        match values.next()? {
            Data::Unsigned(number) => Some(u32::try_from(number).ok()?),
            _ => return None,
        }
    } else {
        None
    };
    let Some(Data::BitString(inclusion)) = values.next() else {
        return None;
    };
    let included: Vec<usize> = (0..bit_count(&inclusion))
        .filter(|&position| inclusion.bit(position))
        .collect();
    let references = if options.data_ref {
        let mut references = Vec::with_capacity(included.len());
        for _ in &included {
            match values.next()? {
                Data::VisibleString(reference) => references.push(Some(reference)),
                _ => return None,
            }
        }
        references
    } else {
        vec![None; included.len()]
    };
    let mut entries: Vec<DecodedEntry> = Vec::with_capacity(included.len());
    for (&index, reference) in included.iter().zip(references) {
        entries.push(DecodedEntry {
            index,
            reference,
            value: values.next()?,
            reasons: Vec::new(),
        });
    }
    if options.reason_code {
        for entry in &mut entries {
            match values.next()? {
                Data::BitString(reasons) => entry.reasons = reasons_from_bits(&reasons),
                _ => return None,
            }
        }
    }
    Some(DecodedReport {
        rpt_id,
        sequence,
        data_set,
        conf_rev,
        time_of_entry,
        entries,
    })
}

/// The name of a data set from the value of `DatSet`, which the server gives as an object reference (`device/LN$name`): the name only.
fn plain_name(reference: &str) -> String {
    reference
        .split_once('/')
        .map_or(reference, |(_, name)| name)
        .to_string()
}

/// The value of `DatSet` that a write gives: the object reference of the data set, `device/LN$name` (7-2 17.2.2.6). The server takes a
/// name too, but the peer needs the reference.
fn data_set_reference(device: &str, name: Option<&str>) -> String {
    name.map_or_else(String::new, |name| format!("{device}/{name}"))
}

impl IecClient {
    /// The attributes of a block, read in one request.
    pub async fn urcb_values(
        &self,
        device: &str,
        node: &str,
        block: &str,
    ) -> Result<UrcbValues, ClientError> {
        let mut items = Vec::with_capacity(READ_ATTRIBUTES.len());
        for attribute in READ_ATTRIBUTES {
            items.push(component_item(
                device,
                &format!("{node}$RP${block}"),
                attribute,
            )?);
        }
        let results = self.read(&items).await?;
        let mut values = Vec::with_capacity(results.len());
        for result in results {
            values.push(
                result.map_err(|_| {
                    ClientError::Response("an attribute of the block cannot be read")
                })?,
            );
        }
        let mut values = values.into_iter();
        let mut next = || {
            values.next().ok_or(ClientError::Response(
                "the block answers too few attributes",
            ))
        };
        let rpt_id = match next()? {
            Data::VisibleString(id) => id,
            _ => return Err(ClientError::Response("RptID is not a visible string")),
        };
        let enabled = matches!(next()?, Data::Boolean(true));
        let reserved = matches!(next()?, Data::Boolean(true));
        let data_set = match next()? {
            Data::VisibleString(name) if !name.is_empty() => Some(plain_name(&name)),
            _ => None,
        };
        let conf_rev = match next()? {
            Data::Unsigned(number) => u32::try_from(number).unwrap_or(u32::MAX),
            _ => return Err(ClientError::Response("ConfRev is not an integer")),
        };
        let options = match next()? {
            Data::BitString(bits) => optional_fields_from_bits(&bits),
            _ => return Err(ClientError::Response("OptFlds is not a bit string")),
        };
        let buf_tm_ms = match next()? {
            Data::Unsigned(number) => u32::try_from(number).unwrap_or(u32::MAX),
            _ => return Err(ClientError::Response("BufTm is not an integer")),
        };
        let sequence = match next()? {
            Data::Unsigned(number) => u8::try_from(number).unwrap_or(0),
            _ => return Err(ClientError::Response("SqNum is not an integer")),
        };
        let triggers = match next()? {
            Data::BitString(bits) => triggers_from_bits(&bits),
            _ => return Err(ClientError::Response("TrgOps is not a bit string")),
        };
        let intg_pd_ms = match next()? {
            Data::Unsigned(number) => u32::try_from(number).unwrap_or(u32::MAX),
            _ => return Err(ClientError::Response("IntgPd is not an integer")),
        };
        Ok(UrcbValues {
            rpt_id,
            enabled,
            reserved,
            data_set,
            conf_rev,
            options,
            buf_tm_ms,
            sequence,
            triggers,
            intg_pd_ms,
        })
    }

    /// One write of a block: its result, as the server answers it.
    pub async fn urcb_write(
        &self,
        device: &str,
        node: &str,
        block: &str,
        write: UrcbWrite,
    ) -> Result<Result<(), ItemError>, ClientError> {
        let (attribute, data) = match write {
            UrcbWrite::Reserve(on) => ("Resv", Data::Boolean(on)),
            UrcbWrite::Enable(on) => ("RptEna", Data::Boolean(on)),
            UrcbWrite::RptId(id) => ("RptID", Data::VisibleString(id)),
            UrcbWrite::DataSet(name) => (
                "DatSet",
                Data::VisibleString(data_set_reference(device, name.as_deref())),
            ),
            UrcbWrite::OptFields(fields) => {
                ("OptFlds", Data::BitString(optional_fields_bits(fields)))
            }
            UrcbWrite::BufTm(ms) => ("BufTm", Data::Unsigned(u128::from(ms))),
            UrcbWrite::TriggerOptions(options) => {
                ("TrgOps", Data::BitString(triggers_to_bits(options)))
            }
            UrcbWrite::IntgPd(ms) => ("IntgPd", Data::Unsigned(u128::from(ms))),
            UrcbWrite::GeneralInterrogation => ("GI", Data::Boolean(true)),
        };
        let item = component_item(device, &format!("{node}$RP${block}"), attribute)?;
        let mut results = self.write(&[(item, data)]).await?;
        match results.pop() {
            Some(result) => Ok(result),
            None => Err(ClientError::Response("the write answers no item")),
        }
    }
}
