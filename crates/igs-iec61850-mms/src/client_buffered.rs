// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The client side of the buffered reports (task B2; BRCB-002, BRCB-003): the typed reading and writing of a buffered block, and the
//! decoding of its reports in the layout of IEC 61850-7-2 table 73, with the entry identifier to resynchronise from.
//!
//! A buffered block is configured through its attributes, which are the MMS named variables `node$BR$block$attribute`. A client does
//! not reserve it: it sets its options while it is disabled, and enables it. The server sends the reports of an enabled block to the
//! association that enabled it. To resume after a lost association, the client keeps the `EntryID` of the last report it received,
//! sets it (`EntryId`), and enables the block again: the reports start after that entry, with no loss flag.

use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, Identifier, ObjectName, UnconfirmedService, VariableAccessSpecification,
};

use igs_iec61850_mms_core::report::brcb::{BufferedFields, EntryId};
use igs_iec61850_mms_core::report::encode::{
    bit_count, buffered_fields_from_bits, buffered_optional_fields_bits, reasons_from_bits,
    triggers_from_bits, triggers_to_bits,
};
use igs_iec61850_mms_core::report::urcb::{Reason, TriggerOptions};

use crate::client::{ClientError, IecClient, ItemError};
use crate::client_control::component_item;
use crate::client_report::DecodedEntry;

/// The attributes of a buffered block that a client reads (table 70, without the owner and the attributes that are not served).
const READ_ATTRIBUTES: [&str; 10] = [
    "RptID", "RptEna", "DatSet", "ConfRev", "OptFlds", "BufTm", "SqNum", "TrgOps", "IntgPd",
    "EntryID",
];

/// The values of a buffered block, as its attributes give them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferedValues {
    /// The report identifier.
    pub rpt_id: String,
    /// Whether the block is enabled.
    pub enabled: bool,
    /// The data set, when one is set.
    pub data_set: Option<String>,
    /// The configuration revision.
    pub conf_rev: u32,
    /// The options of the reports.
    pub options: BufferedFields,
    /// The buffer time, in milliseconds.
    pub buf_tm_ms: u32,
    /// The sequence number of the next report.
    pub sequence: u16,
    /// The trigger options.
    pub triggers: TriggerOptions,
    /// The integrity period, in milliseconds (zero: none).
    pub intg_pd_ms: u32,
    /// The last entry: the newest made while disabled, or the last one sent while enabled (the NULL identifier when there is none).
    pub entry_id: EntryId,
}

/// A write of a client to a buffered block: one attribute, with its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BufferedWrite {
    /// Enable (`true`) or disable (`false`) the block.
    Enable(bool),
    /// The report identifier.
    RptId(String),
    /// The data set (`None` for the NULL value).
    DataSet(Option<String>),
    /// The options of the reports.
    OptFields(BufferedFields),
    /// The buffer time, in milliseconds.
    BufTm(u32),
    /// The trigger options.
    TriggerOptions(TriggerOptions),
    /// The integrity period, in milliseconds (zero: none).
    IntgPd(u32),
    /// Discard the buffered entries.
    PurgeBuf,
    /// The entry to resynchronise to (the NULL identifier for the oldest one).
    EntryId(EntryId),
}

/// A decoded buffered report: the fields that its options select, and its entries.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedBuffered {
    /// The report identifier.
    pub rpt_id: String,
    /// The sequence number, when the report has it.
    pub sequence: Option<u16>,
    /// The subsequence number of this segment, when the report is segmented.
    pub sub_sequence: Option<u16>,
    /// Whether more segments of the same report follow, when the report is segmented.
    pub more_segments: Option<bool>,
    /// The data set, when the report has it.
    pub data_set: Option<String>,
    /// Whether entries may have been lost before this report, when the report has it.
    pub buf_ovfl: Option<bool>,
    /// The configuration revision, when the report has it.
    pub conf_rev: Option<u32>,
    /// The time at which the entry was made (binary time), when the report has it.
    pub time_of_entry: Option<Vec<u8>>,
    /// The identifier of the entry, when the report has it: the value to resynchronise to after a loss.
    pub entry_id: Option<EntryId>,
    /// The variables of the entry, in the order of the report.
    pub entries: Vec<DecodedEntry>,
}

fn unsigned(data: Data) -> Option<u128> {
    match data {
        Data::Unsigned(number) => Some(number),
        _ => None,
    }
}

fn boolean(data: Data) -> Option<bool> {
    match data {
        Data::Boolean(on) => Some(on),
        _ => None,
    }
}

/// Decodes a buffered report (a segment of it), in the wire order of IEC 61850-8-1 table 74. `None` when the PDU is not a report of a
/// buffered block, or when its layout does not follow its options and its inclusion bit string.
pub fn decode_buffered_report(report: &UnconfirmedService) -> Option<DecodedBuffered> {
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
    let Some(Data::BitString(bits)) = values.next() else {
        return None;
    };
    let options = buffered_fields_from_bits(&bits);
    let sequence = if options.seq_num {
        Some(u16::try_from(unsigned(values.next()?)?).ok()?)
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
    let buf_ovfl = if options.buf_ovfl {
        Some(boolean(values.next()?)?)
    } else {
        None
    };
    let entry_id = if options.entry_id {
        match values.next()? {
            Data::OctetString(octets) => Some(EntryId(u64::from_be_bytes(
                <[u8; 8]>::try_from(octets.as_slice()).ok()?,
            ))),
            _ => return None,
        }
    } else {
        None
    };
    let conf_rev = if options.conf_rev {
        Some(u32::try_from(unsigned(values.next()?)?).ok()?)
    } else {
        None
    };
    let (sub_sequence, more_segments) = if options.segmentation {
        (
            Some(u16::try_from(unsigned(values.next()?)?).ok()?),
            Some(boolean(values.next()?)?),
        )
    } else {
        (None, None)
    };
    // The inclusion bit string names the variables of the report, by their positions in the data set; the references, the values and
    // the reasons follow, one of each per included variable.
    let inclusion = match values.next()? {
        Data::BitString(inclusion) => inclusion,
        _ => return None,
    };
    let positions: Vec<usize> = (0..bit_count(&inclusion))
        .filter(|position| inclusion.bit(*position))
        .collect();
    let count = positions.len();
    let rest: Vec<Data> = values.collect();
    let references_len = if options.data_ref { count } else { 0 };
    let reasons_len = if options.reason_code { count } else { 0 };
    if rest.len()
        != references_len
            .saturating_add(count)
            .saturating_add(reasons_len)
    {
        return None;
    }
    let references = rest.get(..references_len)?;
    let values_part = rest.get(references_len..references_len.saturating_add(count))?;
    let reasons_part = rest.get(references_len.saturating_add(count)..)?;
    let mut entries = Vec::with_capacity(count);
    for (slot, position) in positions.into_iter().enumerate() {
        let reference = match references.get(slot) {
            Some(Data::VisibleString(text)) => Some(text.clone()),
            Some(_) => return None,
            None => None,
        };
        let value = values_part.get(slot)?.clone();
        let reasons: Vec<Reason> = if options.reason_code {
            match reasons_part.get(slot) {
                Some(Data::BitString(bits)) => reasons_from_bits(bits),
                _ => return None,
            }
        } else {
            Vec::new()
        };
        entries.push(DecodedEntry {
            index: position,
            reference,
            value,
            reasons,
        });
    }
    Some(DecodedBuffered {
        rpt_id,
        sequence,
        sub_sequence,
        more_segments,
        data_set,
        buf_ovfl,
        conf_rev,
        time_of_entry,
        entry_id,
        entries,
    })
}

impl IecClient {
    /// The attributes of a buffered block, read in one request.
    pub async fn buffered_values(
        &self,
        device: &str,
        node: &str,
        block: &str,
    ) -> Result<BufferedValues, ClientError> {
        let mut items = Vec::with_capacity(READ_ATTRIBUTES.len());
        for attribute in READ_ATTRIBUTES {
            items.push(component_item(
                device,
                &format!("{node}$BR${block}"),
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
        let data_set = match next()? {
            Data::VisibleString(name) if !name.is_empty() => Some(name),
            _ => None,
        };
        let conf_rev = match next()? {
            Data::Unsigned(number) => u32::try_from(number).unwrap_or(u32::MAX),
            _ => return Err(ClientError::Response("ConfRev is not an integer")),
        };
        let options = match next()? {
            Data::BitString(bits) => buffered_fields_from_bits(&bits),
            _ => return Err(ClientError::Response("OptFlds is not a bit string")),
        };
        let buf_tm_ms = match next()? {
            Data::Unsigned(number) => u32::try_from(number).unwrap_or(u32::MAX),
            _ => return Err(ClientError::Response("BufTm is not an integer")),
        };
        let sequence = match next()? {
            Data::Unsigned(number) => u16::try_from(number).unwrap_or(0),
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
        let entry_id = match next()? {
            Data::OctetString(octets) => EntryId(u64::from_be_bytes(
                <[u8; 8]>::try_from(octets.as_slice())
                    .map_err(|_| ClientError::Response("EntryID is not eight octets"))?,
            )),
            _ => return Err(ClientError::Response("EntryID is not an octet string")),
        };
        Ok(BufferedValues {
            rpt_id,
            enabled,
            data_set,
            conf_rev,
            options,
            buf_tm_ms,
            sequence,
            triggers,
            intg_pd_ms,
            entry_id,
        })
    }

    /// One write of a buffered block: its result, as the server answers it.
    pub async fn buffered_write(
        &self,
        device: &str,
        node: &str,
        block: &str,
        write: BufferedWrite,
    ) -> Result<Result<(), ItemError>, ClientError> {
        let (attribute, data) = match write {
            BufferedWrite::Enable(on) => ("RptEna", Data::Boolean(on)),
            BufferedWrite::RptId(id) => ("RptID", Data::VisibleString(id)),
            BufferedWrite::DataSet(name) => {
                ("DatSet", Data::VisibleString(name.unwrap_or_default()))
            }
            BufferedWrite::OptFields(fields) => (
                "OptFlds",
                Data::BitString(buffered_optional_fields_bits(fields)),
            ),
            BufferedWrite::BufTm(ms) => ("BufTm", Data::Unsigned(u128::from(ms))),
            BufferedWrite::TriggerOptions(options) => {
                ("TrgOps", Data::BitString(triggers_to_bits(options)))
            }
            BufferedWrite::IntgPd(ms) => ("IntgPd", Data::Unsigned(u128::from(ms))),
            BufferedWrite::PurgeBuf => ("PurgeBuf", Data::Boolean(true)),
            BufferedWrite::EntryId(id) => {
                ("EntryID", Data::OctetString(id.0.to_be_bytes().to_vec()))
            }
        };
        let item = component_item(device, &format!("{node}$BR${block}"), attribute)?;
        let mut results = self.write(&[(item, data)]).await?;
        match results.pop() {
            Some(result) => Ok(result),
            None => Err(ClientError::Response("the write answers no item")),
        }
    }
}
