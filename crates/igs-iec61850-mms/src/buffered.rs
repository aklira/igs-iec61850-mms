// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The buffered reports of a served model (task B2; BRCB-002, BRCB-003).
//!
//! A served model holds one buffered block (the state machine of task B1) per buffered report control block of its configuration. The
//! block keeps its history while no association is attached: a change made then is an entry, sent when the owning association
//! enables the block again and resynchronises to the last entry it received (`EntryID`, 8-1 §8.1.3.5).
//!
//! The values of an entry are read when the entry is made, and kept with it (the [`Action::Entered`] of the state machine): a
//! report of an old entry carries the values of its moment, not the current ones (7-2 §17.2.3.2.2.9). A report that is too long for
//! the segment budget is sent as several segments with the same sequence number and increasing subsequence numbers, the last one
//! with `MoreSegmentsFollow` false (7-2 §17.2.3.2.2.5 and §17.2.3.2.2.6); the budget is an estimate of the octets, not the negotiated
//! PDU size, which this release does not read.
//!
//! The owner of a block is the association that enabled it. Its reports go to that association only; the others do not receive them
//! (RPT-003). An association that is lost leaves the block disabled, and the entries stay in its history.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, SystemTime};

use igs_iec61850_mms_core::control::ClientId;
use igs_iec61850_mms_core::report::brcb::{
    Action, Brcb, BufferedReport, Config, EntryId, Event, Refusal, Write,
};
use igs_iec61850_mms_core::report::encode::{
    binary_time, buffered_fields_from_bits, buffered_information_report,
    buffered_optional_fields_bits, triggers_from_bits, triggers_to_bits, BufferedContent,
    BufferedVariable, REPORT_LIST,
};
use igs_iec61850_mms_core::report::urcb::Timer;
use igs_iso9506_mms::server::{Outbound, PeerId};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, DataAccessError, Identifier, InformationReport, ObjectName,
    UnconfirmedService, VariableAccessSpecification, WriteResponseItemItem,
};

use crate::reports::{due, since_1984, trigger_of, Wire};

/// The entries that a block keeps. The standard leaves the number to the server (7-2 §17.2.3.2.3.6, NOTE 2).
pub(crate) const HISTORY_CAPACITY: usize = 256;

/// The budget of one segment of a report, in octets as [`estimate`] counts them. Half of the default transport size, which leaves room
/// for the envelope of the PDU.
const SEGMENT_OCTETS: usize = 4096;

/// The values of an entry, read when the entry was made: the time of entry since 1 January 1984, and the value of each variable of
/// the members of the entry (by the index of the variable).
struct Snapshot {
    time_of_entry: Duration,
    values: Vec<(usize, Data)>,
}

/// One buffered report control block of a served model: its state machine, its variables, its owner, and the values of its entries.
#[cfg_attr(not(feature = "scl"), allow(dead_code))]
pub(crate) struct BufferedBlock {
    name: String,
    device: String,
    node: String,
    reference: String,
    brcb: Brcb,
    wires: Vec<Wire>,
    by_variable: BTreeMap<String, Vec<usize>>,
    timers: Vec<(Timer, Duration)>,
    owner: Option<PeerId>,
    snapshots: BTreeMap<EntryId, Snapshot>,
    /// The value that each variable had at its last notification, by the index of the variable.
    latest: BTreeMap<usize, Data>,
}

#[cfg_attr(not(feature = "scl"), allow(dead_code))]
impl BufferedBlock {
    /// A block named `name` of the logical node `node` of the device `device`, with this configuration and these variables.
    pub(crate) fn new(
        name: &str,
        device: &str,
        node: &str,
        config: Config,
        wires: Vec<Wire>,
    ) -> Self {
        let mut by_variable: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, wire) in wires.iter().enumerate() {
            by_variable
                .entry(format!("{}/{}", wire.device(), wire.name()))
                .or_default()
                .push(index);
        }
        Self {
            name: name.to_string(),
            device: device.to_string(),
            node: node.to_string(),
            reference: format!("{device}/{node}.{name}"),
            brcb: Brcb::new(config, HISTORY_CAPACITY),
            wires,
            by_variable,
            timers: Vec::new(),
            owner: None,
            snapshots: BTreeMap::new(),
            latest: BTreeMap::new(),
        }
    }

    /// The logical device of the block.
    pub(crate) fn device(&self) -> &str {
        &self.device
    }

    /// The component of the block in the model: `node$BR$name` (8-1 §17.1.3).
    pub(crate) fn component(&self) -> String {
        format!("{}$BR${}", self.node, self.name)
    }

    /// Starts the block at `now`: its integrity period, when one is configured.
    pub(crate) fn start(&mut self, now: Duration, outbox: &mut VecDeque<Outbound>) {
        let actions = self.brcb.start(now);
        self.apply(actions, outbox);
    }

    /// A write of the association `peer`: the block answers it, and its reports go to the association that enables it.
    pub(crate) fn request(
        &mut self,
        now: Duration,
        peer: PeerId,
        write: Write,
        outbox: &mut VecDeque<Outbound>,
    ) -> Option<Result<(), Refusal>> {
        if matches!(write, Write::RptEna(true)) {
            // The owner is set before the reports of the enabling are formatted; a refused enabling resets it below.
            self.owner = Some(peer);
        }
        let actions = self.brcb.handle(
            now,
            Event::Request {
                client: ClientId(peer.0),
                write,
            },
        );
        self.apply(actions, outbox)
    }

    /// An event of the block (a notification, a timer, or the loss of an association).
    pub(crate) fn event(&mut self, now: Duration, event: Event, outbox: &mut VecDeque<Outbound>) {
        let actions = self.brcb.handle(now, event);
        self.apply(actions, outbox);
    }

    /// The notification of a change of a variable, with the value it was written with: one notification per member whose trigger the
    /// change raises.
    pub(crate) fn notify(
        &mut self,
        now: Duration,
        name: &str,
        changed: bool,
        value: &Data,
        outbox: &mut VecDeque<Outbound>,
    ) {
        let Some(indexes) = self.by_variable.get(name).cloned() else {
            return;
        };
        for index in &indexes {
            self.latest.insert(*index, value.clone());
        }
        for index in indexes {
            let Some(wire) = self.wires.get(index) else {
                continue;
            };
            let Some(trigger) = trigger_of(wire.triggers(), changed) else {
                continue;
            };
            let member = wire.member();
            self.event(now, Event::Notification { member, trigger }, outbox);
        }
    }

    /// The association was lost: the block is disabled, and it keeps its history.
    pub(crate) fn released(
        &mut self,
        now: Duration,
        peer: PeerId,
        outbox: &mut VecDeque<Outbound>,
    ) {
        if self.owner == Some(peer) {
            self.event(now, Event::AssociationLost(ClientId(peer.0)), outbox);
        }
    }

    /// The timers that have expired at `now`, each handled once.
    pub(crate) fn expire(&mut self, now: Duration, outbox: &mut VecDeque<Outbound>) {
        while let Some(index) = due(&self.timers, now) {
            let (timer, _) = self.timers.remove(index);
            self.event(now, Event::Timer(timer), outbox);
        }
    }

    /// Applies the actions of the block: its entries are kept with their values, its reports go to its owner, and its timers follow.
    /// Returns the answer to the request, if the actions have one.
    fn apply(
        &mut self,
        actions: Vec<Action>,
        outbox: &mut VecDeque<Outbound>,
    ) -> Option<Result<(), Refusal>> {
        let mut answer = None;
        for action in actions {
            match action {
                Action::Respond(outcome) => answer = Some(outcome),
                Action::Entered { id, members } => self.snapshot(id, &members),
                Action::Report(report) => {
                    if let Some(owner) = self.owner {
                        for segment in self.segments(&report) {
                            outbox.push_back(Outbound {
                                peer: Some(owner),
                                report: segment,
                            });
                        }
                    }
                }
                Action::Arm { timer, at } => {
                    self.timers.retain(|(armed, _)| *armed != timer);
                    self.timers.push((timer, at));
                }
                Action::Disarm(timer) => self.timers.retain(|(armed, _)| *armed != timer),
            }
        }
        if !self.brcb.is_enabled() {
            self.owner = None;
        }
        let history: Vec<EntryId> = self.brcb.history().map(|entry| entry.id).collect();
        self.snapshots.retain(|id, _| history.contains(id));
        answer
    }

    /// Reads the values of the variables of the members of an entry, now, and keeps them with the entry.
    fn snapshot(&mut self, id: EntryId, members: &[usize]) {
        let values = self
            .wires
            .iter()
            .enumerate()
            .filter(|(_, wire)| members.contains(&wire.member()))
            .map(|(index, wire)| {
                let value = self
                    .latest
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| wire.value());
                (index, value)
            })
            .collect();
        self.snapshots.insert(
            id,
            Snapshot {
                time_of_entry: since_1984(SystemTime::now()),
                values,
            },
        );
    }

    /// The segments of a report: one `InformationReport` each, on the list `RPT`. Without the segmentation option the report is one
    /// segment, whatever its size.
    fn segments(&self, report: &BufferedReport) -> Vec<UnconfirmedService> {
        let config = self.brcb.config();
        let snapshot = self.snapshots.get(&report.entry_id);
        let mut variables = Vec::new();
        for entry in &report.entries {
            for (index, wire) in self.wires.iter().enumerate() {
                if wire.member() != entry.member {
                    continue;
                }
                let value = snapshot
                    .and_then(|kept| kept.values.iter().find(|(kept, _)| *kept == index))
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| wire.value());
                variables.push(BufferedVariable {
                    position: index,
                    reference: wire.reference().to_string(),
                    value,
                    reasons: entry.reasons.clone(),
                });
            }
        }
        let time_of_entry = snapshot.map_or(Duration::ZERO, |kept| kept.time_of_entry);
        let groups = if config.opt_flds.segmentation {
            split(&variables)
        } else {
            vec![variables.as_slice()]
        };
        let count = groups.len();
        groups
            .into_iter()
            .enumerate()
            .map(|(index, group)| {
                let content = BufferedContent {
                    rpt_id: &config.rpt_id,
                    block_reference: &self.reference,
                    data_set: config.data_set.as_deref().unwrap_or_default(),
                    conf_rev: config.conf_rev,
                    opt_flds: config.opt_flds,
                    sequence: report.sequence,
                    sub_sequence: u16::try_from(index).unwrap_or(u16::MAX),
                    more_segments: index.saturating_add(1) < count,
                    buf_ovfl: report.buf_ovfl,
                    time_of_entry,
                    entry_id: report.entry_id,
                    variable_count: self.wires.len(),
                    variables: group,
                };
                UnconfirmedService::InformationReport(InformationReport {
                    variable_access_specification: VariableAccessSpecification::VariableListName(
                        ObjectName::VmdSpecific(Identifier(REPORT_LIST.to_string())),
                    ),
                    list_of_access_result: buffered_information_report(&content)
                        .into_iter()
                        .map(AccessResult::Success)
                        .collect(),
                })
            })
            .collect()
    }

    /// The value of an attribute of the block (7-2 table 70, GetBRCBValues). `ResvTms`, `Owner` and the other attributes that this
    /// release does not serve are not in the model.
    pub(crate) fn read_attribute(&self, attribute: &str) -> AccessResult {
        let config = self.brcb.config();
        let last = self.brcb.last_entry_id();
        let value = match attribute {
            "RptID" => Data::VisibleString(config.rpt_id.clone()),
            "RptEna" => Data::Boolean(self.brcb.is_enabled()),
            "DatSet" => Data::VisibleString(config.data_set.clone().unwrap_or_default()),
            "ConfRev" => Data::Unsigned(u128::from(config.conf_rev)),
            "OptFlds" => Data::BitString(buffered_optional_fields_bits(config.opt_flds)),
            "BufTm" => Data::Unsigned(config.buf_tm.as_millis()),
            "IntgPd" => Data::Unsigned(config.intg_pd.as_millis()),
            "TrgOps" => Data::BitString(triggers_to_bits(config.trg_ops)),
            "SqNum" => Data::Unsigned(u128::from(self.brcb.sequence())),
            "EntryID" => Data::OctetString(last.0.to_be_bytes().to_vec()),
            "TimeOfEntry" => Data::BinaryTime(binary_time(
                self.snapshots
                    .get(&last)
                    .map_or(Duration::ZERO, |kept| kept.time_of_entry),
            )),
            "PurgeBuf" | "GI" => Data::Boolean(false),
            "Owner" => Data::OctetString(Vec::new()),
            _ => return AccessResult::Failure(DataAccessError::OBJECT_NON_EXISTENT),
        };
        AccessResult::Success(value)
    }

    /// A write of an attribute of the block (SetBRCBValues): the write of the state machine, with its refusal as the access result.
    /// The attributes that a client does not write are access denied; `PurgeBuf` FALSE is a positive answer that changes nothing.
    pub(crate) fn write_attribute(
        &mut self,
        now: Duration,
        peer: PeerId,
        attribute: &str,
        data: &Data,
        outbox: &mut VecDeque<Outbound>,
    ) -> WriteResponseItemItem {
        let write = match (attribute, data) {
            ("RptID", Data::VisibleString(id)) => Write::RptId(id.clone()),
            ("RptEna", Data::Boolean(on)) => Write::RptEna(*on),
            ("DatSet", Data::VisibleString(set)) => {
                Write::DataSet((!set.is_empty()).then(|| set.clone()))
            }
            ("OptFlds", Data::BitString(bits)) => Write::OptFields(buffered_fields_from_bits(bits)),
            ("BufTm", Data::Unsigned(millis)) => Write::BufTm(Duration::from_millis(
                u64::try_from(*millis).unwrap_or(u64::MAX),
            )),
            ("TrgOps", Data::BitString(bits)) => Write::TrgOps(triggers_from_bits(bits)),
            ("IntgPd", Data::Unsigned(millis)) => Write::IntgPd(Duration::from_millis(
                u64::try_from(*millis).unwrap_or(u64::MAX),
            )),
            ("PurgeBuf", Data::Boolean(true)) => Write::PurgeBuf,
            ("PurgeBuf", Data::Boolean(false)) => return WriteResponseItemItem::Success,
            ("EntryID", Data::OctetString(octets)) => {
                match <[u8; 8]>::try_from(octets.as_slice()) {
                    Ok(bytes) => Write::EntryId(EntryId(u64::from_be_bytes(bytes))),
                    Err(_) => {
                        return WriteResponseItemItem::Failure(DataAccessError::TYPE_INCONSISTENT)
                    }
                }
            }
            ("GI" | "SqNum" | "TimeOfEntry" | "Owner" | "ConfRev", _) => {
                return WriteResponseItemItem::Failure(DataAccessError::OBJECT_ACCESS_DENIED)
            }
            (
                "RptID" | "RptEna" | "DatSet" | "OptFlds" | "BufTm" | "TrgOps" | "IntgPd"
                | "PurgeBuf" | "EntryID",
                _,
            ) => return WriteResponseItemItem::Failure(DataAccessError::TYPE_INCONSISTENT),
            _ => return WriteResponseItemItem::Failure(DataAccessError::OBJECT_NON_EXISTENT),
        };
        match self.request(now, peer, write, outbox) {
            Some(Err(refusal)) => WriteResponseItemItem::Failure(refusal_error(refusal)),
            _ => WriteResponseItemItem::Success,
        }
    }
}

/// The access result of a refusal of a write of a buffered block: a block enabled, or a data set that is missing, is refused; an entry
/// that is not in the history is an invalid value.
fn refusal_error(refusal: Refusal) -> DataAccessError {
    match refusal {
        Refusal::DataSetNull | Refusal::UnknownDataSet | Refusal::EntryNotInQueue { .. } => {
            DataAccessError::OBJECT_VALUE_INVALID
        }
        #[allow(unreachable_patterns)]
        _ => DataAccessError::OBJECT_ACCESS_DENIED,
    }
}

/// The segments of the variables of a report: consecutive groups whose estimated size stays within the budget. A single variable
/// larger than the budget is a segment of its own.
fn split(variables: &[BufferedVariable]) -> Vec<&[BufferedVariable]> {
    let mut groups = Vec::new();
    let mut start = 0usize;
    let mut size = 0usize;
    for (index, variable) in variables.iter().enumerate() {
        let cost =
            estimate(&variable.value).saturating_add(variable.reference.len().saturating_add(4));
        if size.saturating_add(cost) > SEGMENT_OCTETS && index > start {
            groups.push(variables.get(start..index).unwrap_or_default());
            start = index;
            size = 0;
        }
        size = size.saturating_add(cost);
    }
    groups.push(variables.get(start..).unwrap_or_default());
    groups
}

/// An estimate of the octets of a value in the report: a conservative count of its encoding, which the segment budget is made of.
fn estimate(value: &Data) -> usize {
    match value {
        Data::Boolean(_) => 3,
        Data::Integer(_) | Data::Unsigned(_) | Data::Bcd(_) => 18,
        Data::BinaryTime(_) => 8,
        Data::OctetString(octets) => octets.len().saturating_add(4),
        Data::VisibleString(text) | Data::GeneralizedTime(text) => text.len().saturating_add(4),
        Data::BitString(bits) | Data::BooleanArray(bits) => bits.bytes.len().saturating_add(4),
        Data::Structure(items) | Data::Array(items) => items
            .iter()
            .map(estimate)
            .fold(4usize, |sum, size| sum.saturating_add(size)),
        _ => 32,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn variable(octets: usize) -> BufferedVariable {
        BufferedVariable {
            position: 0,
            reference: "LD1/GGIO1.Ind1.stVal".to_string(),
            value: Data::OctetString(vec![0; octets]),
            reasons: BTreeSet::new(),
        }
    }

    #[test]
    fn a_report_within_the_budget_is_one_segment() {
        let variables: Vec<BufferedVariable> = (0..10).map(|_| variable(16)).collect();
        let groups = split(&variables);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 10);
    }

    #[test]
    fn a_report_over_the_budget_is_split_into_consecutive_segments_within_it() {
        // Each variable is about 3 KiB: one segment holds one of them.
        let variables: Vec<BufferedVariable> = (0..3).map(|_| variable(3000)).collect();
        let groups = split(&variables);
        assert_eq!(groups.len(), 3);
        assert!(groups.iter().all(|group| group.len() == 1));
        let total: usize = groups.iter().map(|group| group.len()).sum();
        assert_eq!(total, 3, "no variable is lost or repeated");
    }

    #[test]
    fn an_empty_report_is_one_empty_segment() {
        let groups = split(&[]);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].is_empty());
    }

    #[test]
    fn a_variable_larger_than_the_budget_is_a_segment_of_its_own() {
        let variables = vec![variable(10), variable(20_000), variable(10)];
        let groups = split(&variables);
        assert_eq!(
            groups.iter().map(|group| group.len()).collect::<Vec<_>>(),
            [1, 1, 1]
        );
    }
}
