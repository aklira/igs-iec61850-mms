// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The unbuffered reports of a served model (task D4; RPT-002, RPT-003).
//!
//! A served model holds one unbuffered block (the state machine of task D3) per report control block of its configuration. The
//! application writes to a block through a [`Reports`] handle, as an association would: the block answers the write, and it
//! produces its reports. The changes that the application applies through the updater are notifications of the blocks: a change
//! of a data attribute of a member raises its triggers (dchg and qchg on a change, dupd on any update). The notifications, the
//! buffer and integrity timers, and the reports are handled when the server polls the model.
//!
//! A report is sent to the association that holds the block: the owner, which reserved it and enabled it. Any other association
//! does not receive it (RPT-003). A report is encoded as an `InformationReport` on the list `RPT` (8-1 §17.2.1; the layout is in
//! `igs_iec61850_mms_core::report::encode`) and queued as an outbound PDU for the owner: its delivery, and the diagnostic of a
//! saturated queue, are the outcomes of the observable delivery (task N5).
//!
//! The clock of the blocks is the time since the state was created. A request of the application takes that clock too, so the
//! timers of a block and its requests are on one clock.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use igs_iec61850_mms_core::control::ClientId;
use igs_iec61850_mms_core::model::layout::value_at;
use igs_iec61850_mms_core::report::encode::{
    information_report, object_reference, optional_fields_bits, optional_fields_from_bits,
    triggers_from_bits, triggers_to_bits, Content, Variable, REPORT_LIST,
};
use igs_iec61850_mms_core::report::urcb::{
    Action, Config, Event, Refusal, Report, Timer, Trigger, TriggerOptions, Urcb, Write,
};
use igs_iso9506_mms::server::{Outbound, PeerId};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, DataAccessError, Identifier, InformationReport, ObjectName,
    UnconfirmedService, VariableAccessSpecification, WriteResponseItemItem,
};

use crate::model::NodeValue;
use crate::updates::{Notice, Updater};

/// The seconds from 1 January 1970 to 1 January 1984: the epoch of the binary time of MMS.
const SECONDS_1970_TO_1984: u64 = 441_763_200;

/// Why a request to a block is refused, or why it cannot be made.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportError {
    /// The served model has no unbuffered block of this name.
    UnknownBlock(String),
    /// The block refuses the write (reservation, configuration while enabled, data set, general interrogation).
    Refused(Refusal),
    /// The state of the reports is unavailable: a task that held its lock failed.
    Poisoned,
}

impl std::fmt::Display for ReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownBlock(name) => write!(f, "there is no unbuffered block {name}"),
            Self::Refused(refusal) => write!(f, "the block refuses the write: {refusal:?}"),
            Self::Poisoned => write!(f, "the state of the reports is unavailable"),
        }
    }
}

impl std::error::Error for ReportError {}

/// A variable of the data set of a block: where its value is, the member that selected it, and its triggers. Blocks are built
/// from a configuration only (the `scl` feature): without it, no block exists and these items are not built.
#[cfg_attr(not(feature = "scl"), allow(dead_code))]
pub(crate) struct Wire {
    device: String,
    name: String,
    member: usize,
    triggers: TriggerOptions,
    reference: String,
    root: Arc<Mutex<NodeValue>>,
    path: Vec<usize>,
}

#[cfg_attr(not(feature = "scl"), allow(dead_code))]
impl Wire {
    /// The variable of a data set, from the served model (its value, its path, and its object reference).
    pub(crate) fn new(
        updater: &Updater,
        device: &str,
        name: &str,
        member: usize,
        triggers: TriggerOptions,
    ) -> Option<Self> {
        let (root, path) = updater.point(&format!("{device}/{name}"))?;
        Some(Self {
            device: device.to_string(),
            name: name.to_string(),
            member,
            triggers,
            reference: object_reference(device, name),
            root,
            path,
        })
    }

    /// The value of the variable now. The served value is always present: the path comes from the layout of the node.
    fn value(&self) -> Data {
        match self.root.lock() {
            Ok(state) => value_at(&state.data, &self.path)
                .cloned()
                .unwrap_or(Data::Unsigned(0)),
            Err(_) => Data::Unsigned(0),
        }
    }
}

/// One unbuffered report control block of a served model: its state machine, its variables, and its owner.
#[cfg_attr(not(feature = "scl"), allow(dead_code))]
pub(crate) struct Block {
    name: String,
    device: String,
    node: String,
    reference: String,
    urcb: Urcb,
    wires: Vec<Wire>,
    by_variable: BTreeMap<String, Vec<usize>>,
    timers: Vec<(Timer, Duration)>,
    owner: Option<PeerId>,
}

#[cfg_attr(not(feature = "scl"), allow(dead_code))]
impl Block {
    /// A block named `name` of the logical node `node` of the device `device`, with this configuration and these variables, in list order.
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
                .entry(format!("{}/{}", wire.device, wire.name))
                .or_default()
                .push(index);
        }
        Self {
            name: name.to_string(),
            device: device.to_string(),
            node: node.to_string(),
            reference: format!("{device}/{node}.{name}"),
            urcb: Urcb::new(config),
            wires,
            by_variable,
            timers: Vec::new(),
            owner: None,
        }
    }

    /// The component of the block in the model: `node$RP$name` (8-1 §17.1.3).
    fn component(&self) -> String {
        format!("{}$RP${}", self.node, self.name)
    }

    /// The report as the MMS PDU of the list `RPT`, with the options and the variables of the block.
    fn pdu(&self, report: &Report) -> UnconfirmedService {
        let variables: Vec<Variable> = self
            .wires
            .iter()
            .map(|wire| Variable {
                member: wire.member,
                reference: wire.reference.clone(),
                value: wire.value(),
            })
            .collect();
        let config = self.urcb.config();
        let content = Content {
            rpt_id: &config.rpt_id,
            block_reference: &self.reference,
            data_set: config.data_set.as_deref().unwrap_or_default(),
            conf_rev: config.conf_rev,
            opt_flds: config.opt_flds,
            time_of_entry: since_1984(SystemTime::now()),
            sequence: report.sequence,
            variables: &variables,
        };
        let results = information_report(&content, report);
        UnconfirmedService::InformationReport(InformationReport {
            variable_access_specification: VariableAccessSpecification::VariableListName(
                ObjectName::VmdSpecific(Identifier(REPORT_LIST.to_string())),
            ),
            list_of_access_result: results.into_iter().map(AccessResult::Success).collect(),
        })
    }
}

/// The shared state of the reports of a served model: the blocks, the admitted associations, and the outbound reports.
pub(crate) struct ReportState {
    blocks: Vec<Block>,
    outbox: VecDeque<Outbound>,
    peers: Vec<PeerId>,
    origin: Instant,
    dropped: u64,
}

impl ReportState {
    pub(crate) fn new() -> Self {
        Self {
            blocks: Vec::new(),
            outbox: VecDeque::new(),
            peers: Vec::new(),
            origin: Instant::now(),
            dropped: 0,
        }
    }

    #[cfg_attr(not(feature = "scl"), allow(dead_code))]
    pub(crate) fn add_block(&mut self, block: Block) {
        self.blocks.push(block);
    }

    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    /// Records an admitted association.
    pub(crate) fn admitted(&mut self, peer: PeerId) {
        if !self.peers.contains(&peer) {
            self.peers.push(peer);
        }
    }

    /// A write of an association to a block: the block answers it, and its reports are queued for the owner.
    pub(crate) fn request(
        &mut self,
        peer: PeerId,
        block: &str,
        write: Write,
    ) -> Result<(), ReportError> {
        let now = self.now();
        let target = self
            .blocks
            .iter_mut()
            .find(|candidate| candidate.name == block)
            .ok_or_else(|| ReportError::UnknownBlock(block.to_string()))?;
        let actions = target.urcb.handle(
            now,
            Event::Request {
                client: ClientId(peer.0),
                write,
            },
        );
        let answer = apply(target, actions, &mut self.outbox);
        match answer {
            Some(Err(refusal)) => Err(ReportError::Refused(refusal)),
            _ => Ok(()),
        }
    }

    /// The association was closed: the blocks it reserved release it, and a block it enabled is disabled.
    pub(crate) fn released(&mut self, peer: PeerId) {
        let now = self.now();
        for block in &mut self.blocks {
            if block.urcb.reserved_by() == Some(ClientId(peer.0)) {
                let actions = block
                    .urcb
                    .handle(now, Event::AssociationLost(ClientId(peer.0)));
                apply(block, actions, &mut self.outbox);
            }
        }
    }

    /// One poll of the model: the notices of the updates, the timers that have expired, and the reports to send.
    pub(crate) fn poll(&mut self, notices: Vec<Notice>, dropped: u64) -> Vec<Outbound> {
        self.dropped = self.dropped.saturating_add(dropped);
        let now = self.now();
        for block in &mut self.blocks {
            while let Some(index) = due(&block.timers, now) {
                let (timer, _) = block.timers.remove(index);
                let actions = block.urcb.handle(now, Event::Timer(timer));
                apply(block, actions, &mut self.outbox);
            }
        }
        for notice in notices {
            for block in &mut self.blocks {
                let Some(indexes) = block.by_variable.get(&notice.name).cloned() else {
                    continue;
                };
                for index in indexes {
                    let Some(wire) = block.wires.get(index) else {
                        continue;
                    };
                    let Some(trigger) = trigger_of(wire.triggers, notice.changed) else {
                        continue;
                    };
                    let member = wire.member;
                    let actions = block
                        .urcb
                        .handle(now, Event::Notification { member, trigger });
                    apply(block, actions, &mut self.outbox);
                }
            }
        }
        self.outbox.drain(..).collect()
    }

    /// The block and the attribute that an MMS item names (`node$RP$name$attribute` in the device's domain), if it names one.
    pub(crate) fn locate(&self, domain: &str, item: &str) -> Option<(usize, String)> {
        let (component, attribute) = item.rsplit_once('$')?;
        self.blocks
            .iter()
            .position(|block| block.device == domain && block.component() == component)
            .map(|index| (index, attribute.to_string()))
    }

    /// The value of an attribute of a block (8-1 table 73), as a read of the model gives it.
    pub(crate) fn read_attribute(&self, index: usize, attribute: &str) -> AccessResult {
        let Some(block) = self.blocks.get(index) else {
            return AccessResult::Failure(DataAccessError::OBJECT_NON_EXISTENT);
        };
        let config = block.urcb.config();
        let value = match attribute {
            "RptID" => Data::VisibleString(config.rpt_id.clone()),
            "RptEna" => Data::Boolean(block.urcb.is_enabled()),
            "Resv" => Data::Boolean(block.urcb.reserved_by().is_some()),
            "DatSet" => Data::VisibleString(config.data_set.clone().unwrap_or_default()),
            "ConfRev" => Data::Unsigned(u128::from(config.conf_rev)),
            "OptFlds" => Data::BitString(optional_fields_bits(config.opt_flds)),
            "BufTm" => Data::Unsigned(config.buf_tm.as_millis()),
            "SqNum" => Data::Unsigned(u128::from(block.urcb.sequence())),
            "TrgOps" => Data::BitString(triggers_to_bits(config.trg_ops)),
            "IntgPd" => Data::Unsigned(config.intg_pd.as_millis()),
            "GI" => Data::Boolean(false),
            "Owner" => Data::OctetString(Vec::new()),
            _ => return AccessResult::Failure(DataAccessError::OBJECT_NON_EXISTENT),
        };
        AccessResult::Success(value)
    }

    /// A write of an attribute of a block: the write of the state machine, with its refusal as the access result. The
    /// attributes that the standard does not let a client write (`ConfRev`, `SqNum`, `Owner`) are access denied.
    pub(crate) fn write_attribute(
        &mut self,
        peer: PeerId,
        index: usize,
        attribute: &str,
        data: &Data,
    ) -> WriteResponseItemItem {
        let Some(name) = self.blocks.get(index).map(|block| block.name.clone()) else {
            return WriteResponseItemItem::Failure(DataAccessError::OBJECT_NON_EXISTENT);
        };
        let write = match (attribute, data) {
            ("RptID", Data::VisibleString(id)) => Write::RptId(id.clone()),
            ("RptEna", Data::Boolean(on)) => Write::RptEna(*on),
            ("Resv", Data::Boolean(on)) => Write::Reserve(*on),
            ("DatSet", Data::VisibleString(set)) => {
                Write::DataSet((!set.is_empty()).then(|| set.clone()))
            }
            ("OptFlds", Data::BitString(bits)) => Write::OptFields(optional_fields_from_bits(bits)),
            ("BufTm", Data::Unsigned(millis)) => Write::BufTm(Duration::from_millis(
                u64::try_from(*millis).unwrap_or(u64::MAX),
            )),
            ("TrgOps", Data::BitString(bits)) => Write::TrgOps(triggers_from_bits(bits)),
            ("IntgPd", Data::Unsigned(millis)) => Write::IntgPd(Duration::from_millis(
                u64::try_from(*millis).unwrap_or(u64::MAX),
            )),
            ("GI", Data::Boolean(true)) => Write::GeneralInterrogation,
            ("GI", Data::Boolean(false)) => return WriteResponseItemItem::Success,
            ("ConfRev" | "SqNum" | "Owner", _) => {
                return WriteResponseItemItem::Failure(DataAccessError::OBJECT_ACCESS_DENIED)
            }
            (
                "RptID" | "RptEna" | "Resv" | "DatSet" | "OptFlds" | "BufTm" | "TrgOps" | "IntgPd",
                _,
            ) => return WriteResponseItemItem::Failure(DataAccessError::TYPE_INCONSISTENT),
            _ => return WriteResponseItemItem::Failure(DataAccessError::OBJECT_NON_EXISTENT),
        };
        match self.request(peer, &name, write) {
            Ok(()) => WriteResponseItemItem::Success,
            Err(ReportError::Refused(refusal)) => {
                WriteResponseItemItem::Failure(refusal_error(refusal))
            }
            Err(_) => WriteResponseItemItem::Failure(DataAccessError::OBJECT_NON_EXISTENT),
        }
    }

    /// The names of the blocks, in document order.
    pub(crate) fn names(&self) -> Vec<String> {
        self.blocks.iter().map(|block| block.name.clone()).collect()
    }

    /// The owner of a block: the association that holds it while it is enabled.
    pub(crate) fn owner(&self, block: &str) -> Option<Option<PeerId>> {
        self.blocks
            .iter()
            .find(|candidate| candidate.name == block)
            .map(|candidate| candidate.owner)
    }

    pub(crate) fn peers(&self) -> Vec<PeerId> {
        self.peers.clone()
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// The application's handle to the reports of a served model. Cheap to clone; safe to use from other tasks.
#[derive(Clone)]
pub struct Reports {
    state: Arc<Mutex<ReportState>>,
}

impl Reports {
    pub(crate) fn new(state: Arc<Mutex<ReportState>>) -> Self {
        Self { state }
    }

    fn with<T>(&self, action: impl FnOnce(&mut ReportState) -> T) -> Result<T, ReportError> {
        let mut state = self.state.lock().map_err(|_| ReportError::Poisoned)?;
        Ok(action(&mut state))
    }

    /// The names of the unbuffered blocks of the served model, in document order.
    pub fn blocks(&self) -> Result<Vec<String>, ReportError> {
        self.with(|state| state.names())
    }

    /// The associations admitted so far, in order of admission.
    pub fn peers(&self) -> Result<Vec<PeerId>, ReportError> {
        self.with(|state| state.peers())
    }

    /// The association that owns a block (it reserved it and enabled it), if the block is enabled; `None` for an unknown block.
    pub fn owner(&self, block: &str) -> Result<Option<PeerId>, ReportError> {
        self.with(|state| state.owner(block))?
            .ok_or_else(|| ReportError::UnknownBlock(block.to_string()))
    }

    /// The notices dropped at the limit of the queue (RUN-001): a count, never a silent loss.
    pub fn dropped_notices(&self) -> Result<u64, ReportError> {
        self.with(|state| state.dropped())
    }

    /// A write of the association `peer` to the block `block`: reservation, enabling, configuration, or general interrogation.
    /// The block answers it as the state machine does; a refused write changes nothing.
    pub fn request(&self, peer: PeerId, block: &str, write: Write) -> Result<(), ReportError> {
        self.with(|state| state.request(peer, block, write))?
    }
}

/// The access result of a refusal of a write of a block (table 100 for the control blocks, and the same causes here): a
/// reservation held by another client is temporarily unavailable; the attributes that a block does not take while enabled, or a
/// general interrogation while disabled, are access denied; a data set that does not exist is an invalid value.
fn refusal_error(refusal: Refusal) -> DataAccessError {
    match refusal {
        Refusal::Reserved => DataAccessError::TEMPORARILY_UNAVAILABLE,
        Refusal::NotReserved | Refusal::Enabled | Refusal::GeneralInterrogationNotAllowed => {
            DataAccessError::OBJECT_ACCESS_DENIED
        }
        Refusal::DataSetNull | Refusal::UnknownDataSet => DataAccessError::OBJECT_VALUE_INVALID,
        #[allow(unreachable_patterns)]
        _ => DataAccessError::OBJECT_ACCESS_DENIED,
    }
}

/// Applies the actions of a block: its reports are queued for its owner, its timers are armed and disarmed, and the owner
/// follows the reservation of an enabled block. Returns the answer to the request, if the actions have one.
fn apply(
    block: &mut Block,
    actions: Vec<Action>,
    outbox: &mut VecDeque<Outbound>,
) -> Option<Result<(), Refusal>> {
    let mut answer = None;
    for action in actions {
        match action {
            Action::Respond(outcome) => answer = Some(outcome),
            Action::Report(report) => {
                if let Some(owner) = block.owner {
                    outbox.push_back(Outbound {
                        peer: Some(owner),
                        report: block.pdu(&report),
                    });
                }
            }
            Action::Arm { timer, at } => {
                block.timers.retain(|(armed, _)| *armed != timer);
                block.timers.push((timer, at));
            }
            Action::Disarm(timer) => block.timers.retain(|(armed, _)| *armed != timer),
        }
    }
    block.owner = if block.urcb.is_enabled() {
        block.urcb.reserved_by().map(|client| PeerId(client.0))
    } else {
        None
    };
    answer
}

/// The index of the first timer that has expired at `now`.
fn due(timers: &[(Timer, Duration)], now: Duration) -> Option<usize> {
    timers.iter().position(|(_, at)| *at <= now)
}

/// The trigger of a change of a variable: the data-change or the quality-change when the value changed, and the data-update
/// on any update. The first trigger of the variable is the one notified (a notice carries one trigger).
fn trigger_of(triggers: TriggerOptions, changed: bool) -> Option<Trigger> {
    if changed && triggers.data_change {
        Some(Trigger::DataChange)
    } else if changed && triggers.quality_change {
        Some(Trigger::QualityChange)
    } else if triggers.data_update {
        Some(Trigger::DataUpdate)
    } else {
        None
    }
}

/// The time since 1 January 1984 of a system time (zero before that epoch).
fn since_1984(now: SystemTime) -> Duration {
    now.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .saturating_sub(Duration::from_secs(SECONDS_1970_TO_1984))
}
