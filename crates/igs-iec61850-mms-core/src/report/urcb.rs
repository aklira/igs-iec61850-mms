// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The unbuffered report control block (URCB), as a sans-I/O state machine (task D3; RPT-001).
//!
//! The machine takes a request of a client, an internal notification of a member of the data set, a timer, or the loss of
//! an association, with the current time, and returns the actions to take: the responses, the reports to send, and the
//! timers to arm or disarm. It never waits and reads no clock: the application fires the timers at the times it was given.
//!
//! The rules follow IEC 61850-7-2 §17.2 and 8-1 §17.1.3: reservation before any configuration or enabling (17.2.4.5), the
//! configuration attributes writable only while disabled, the data set that must exist before enabling (8-1 17.1.3), the
//! trigger options data-change, quality-change and data-update (17.2.3.2.3.2), the buffer time that combines notifications
//! into one report (17.2.2.9), the integrity period (17.2.3.2.3.3), the general interrogation (17.2.3.2.3.4 and figure 30),
//! and the sequence number that restarts at each enabling (8-1 17.1.3).
//!
//! The unbuffered block keeps no event history (figure 30, NOTE): a report that cannot be sent is not kept. The delivery is the
//! task D4; here a report is an action with its content.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::control::ClientId;

/// The options of the report: the parts of each entry that the report carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OptionalFields {
    /// The sequence number of the report.
    pub seq_num: bool,
    /// The time of the report.
    pub time_stamp: bool,
    /// The reason for inclusion of each entry.
    pub reason_code: bool,
    /// The name of the data set.
    pub data_set_name: bool,
    /// The reference of each data attribute of the report.
    pub data_ref: bool,
    /// The configuration revision.
    pub conf_rev: bool,
}

/// The trigger options of the block. The buffer-overflow and entry options of the buffered block do not apply here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TriggerOptions {
    /// Data change (dchg).
    pub data_change: bool,
    /// Quality change (qchg).
    pub quality_change: bool,
    /// Data update (dupd).
    pub data_update: bool,
    /// Integrity: a report of all members at each integrity period.
    pub integrity: bool,
    /// General interrogation (GI): a report of all members on request.
    pub general_interrogation: bool,
}

/// An internal notification of a member of the data set: the three triggers that a notification can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trigger {
    /// A change of a process value (dchg).
    DataChange,
    /// A change of a quality value (qchg).
    QualityChange,
    /// A freeze or an update (dupd).
    DataUpdate,
}

/// The reason for the inclusion of an entry in a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// A data change.
    DataChange,
    /// A quality change.
    QualityChange,
    /// A data update.
    DataUpdate,
    /// An integrity period.
    Integrity,
    /// A general interrogation.
    GeneralInterrogation,
}

impl From<Trigger> for Reason {
    fn from(trigger: Trigger) -> Self {
        match trigger {
            Trigger::DataChange => Self::DataChange,
            Trigger::QualityChange => Self::QualityChange,
            Trigger::DataUpdate => Self::DataUpdate,
        }
    }
}

/// The configuration of the block, as the SCL and the application give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The report identifier (`RptID`).
    pub rpt_id: String,
    /// The data set, by name; `None` is the NULL value.
    pub data_set: Option<String>,
    /// The data sets that exist in the logical node, with their numbers of members.
    pub data_sets: BTreeMap<String, usize>,
    /// The optional fields of the report.
    pub opt_flds: OptionalFields,
    /// The buffer time: the time during which the notifications are combined into one report. Zero sends each one alone.
    pub buf_tm: Duration,
    /// The integrity period. Zero means no integrity report.
    pub intg_pd: Duration,
    /// The trigger options.
    pub trg_ops: TriggerOptions,
    /// The configuration revision.
    pub conf_rev: u32,
}

/// A write of the client to the block: one attribute, with its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// `Resv`: reserve (`true`) or release (`false`) the block.
    Reserve(bool),
    /// `RptEna`: enable (`true`) or disable (`false`) the block.
    RptEna(bool),
    /// `RptID`.
    RptId(String),
    /// `DatSet`: the data set, or `None` for the NULL value.
    DataSet(Option<String>),
    /// `OptFlds`.
    OptFields(OptionalFields),
    /// `BufTm`.
    BufTm(Duration),
    /// `TrgOps`.
    TrgOps(TriggerOptions),
    /// `IntgPd`.
    IntgPd(Duration),
    /// `GI`: the request of a general interrogation.
    GeneralInterrogation,
}

/// Why a write is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The block is reserved by another client.
    Reserved,
    /// The block is not reserved by the client that writes it (a reservation comes first, 17.2.4.5).
    NotReserved,
    /// The attribute is written only while the block is disabled (`RptEna` is FALSE).
    Enabled,
    /// The block cannot be enabled without a data set.
    DataSetNull,
    /// The data set does not exist in the logical node.
    UnknownDataSet,
    /// A general interrogation is refused while the block is disabled (figure 30: NO GI ALLOWED).
    GeneralInterrogationNotAllowed,
}

/// The kind of a report: why it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    /// The notifications of the members, after their buffer time (or at once without a buffer time).
    Notification,
    /// An integrity report: all members.
    Integrity,
    /// A general-interrogation report: all members.
    GeneralInterrogation,
}

/// One entry of a report: a member of the data set and the reasons for its inclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The index of the member in the data set (its position in the document order).
    pub member: usize,
    /// The reasons for the inclusion of the member.
    pub reasons: BTreeSet<Reason>,
}

/// A report to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The time at which the report is generated.
    pub at: Duration,
    /// The sequence number (`SqNum`, INT8U, before this report).
    pub sequence: u8,
    /// Why the report is sent.
    pub kind: ReportKind,
    /// The entries, in the order of the members.
    pub entries: Vec<Entry>,
}

/// A timer of the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    /// The buffer time of the pending notifications.
    BufferTime,
    /// The integrity period.
    Integrity,
    /// The reservation kept after the association of its holder is lost (`ResvTms`, 7-2 §17.2.2.17); buffered blocks only.
    Reservation,
}

/// An event for the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A write of a client.
    Request {
        /// The client that writes.
        client: ClientId,
        /// The write.
        write: Write,
    },
    /// An internal notification of a member of the data set.
    Notification {
        /// The index of the member.
        member: usize,
        /// The trigger that produced it.
        trigger: Trigger,
    },
    /// A timer expired.
    Timer(Timer),
    /// The association of a client is lost.
    AssociationLost(ClientId),
}

/// An action of the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The answer to the write of a client.
    Respond(Result<(), Refusal>),
    /// A report to send.
    Report(Report),
    /// Arm a timer, to fire at this time.
    Arm {
        /// The timer.
        timer: Timer,
        /// The time of its expiry.
        at: Duration,
    },
    /// Disarm a timer.
    Disarm(Timer),
}

/// An unbuffered report control block.
#[derive(Debug, Clone)]
pub struct Urcb {
    config: Config,
    enabled: bool,
    reserved_by: Option<ClientId>,
    sequence: u8,
    pending: BTreeMap<usize, BTreeSet<Reason>>,
    buffering: bool,
}

impl Urcb {
    /// A block, disabled and not reserved, with this configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            enabled: false,
            reserved_by: None,
            sequence: 0,
            pending: BTreeMap::new(),
            buffering: false,
        }
    }

    /// The configuration of the block.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Whether the block is enabled (`RptEna`).
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The client that reserves the block (`Resv`), if any.
    pub fn reserved_by(&self) -> Option<ClientId> {
        self.reserved_by
    }

    /// The sequence number of the next report (`SqNum`).
    pub fn sequence(&self) -> u8 {
        self.sequence
    }

    /// The number of members of the data set of the block (zero without a data set).
    fn members(&self) -> usize {
        self.config
            .data_set
            .as_ref()
            .and_then(|name| self.config.data_sets.get(name).copied())
            .unwrap_or(0)
    }

    /// The next event of the block, at time `now`, and the actions to take.
    pub fn handle(&mut self, now: Duration, event: Event) -> Vec<Action> {
        match event {
            Event::Request { client, write } => self.request(now, client, write),
            Event::Notification { member, trigger } => self.notification(now, member, trigger),
            Event::Timer(Timer::BufferTime) => self.flush(now).into_iter().collect(),
            Event::Timer(Timer::Integrity) => self.integrity(now),
            // The unbuffered block keeps no reservation after a loss, so it never arms this timer.
            Event::Timer(Timer::Reservation) => Vec::new(),
            Event::AssociationLost(client) => self.lost(client),
        }
    }

    fn request(&mut self, now: Duration, client: ClientId, write: Write) -> Vec<Action> {
        match write {
            Write::Reserve(_) if self.enabled => respond(Err(Refusal::Enabled)),
            Write::Reserve(true) => match self.reserved_by {
                None => {
                    self.reserved_by = Some(client);
                    respond(Ok(()))
                }
                Some(holder) if holder == client => respond(Ok(())),
                Some(_) => respond(Err(Refusal::Reserved)),
            },
            Write::Reserve(false) => match self.reserved_by {
                Some(holder) if holder == client => {
                    self.reserved_by = None;
                    respond(Ok(()))
                }
                _ => respond(Err(Refusal::NotReserved)),
            },
            Write::RptEna(true) => self.enable(now, client),
            Write::RptEna(false) => match self.reserved_by {
                Some(holder) if holder != client => respond(Err(Refusal::Reserved)),
                _ => {
                    let mut actions = self.disable();
                    actions.push(Action::Respond(Ok(())));
                    actions
                }
            },
            Write::GeneralInterrogation => self.general_interrogation(now, client),
            configuration => self.configure(client, configuration),
        }
    }

    /// `RptEna` TRUE: the client holds the reservation, the data set is set, and the block starts: the sequence number
    /// restarts at zero, and the integrity timer starts when integrity is on (8-1 17.1.3, figure 26).
    fn enable(&mut self, now: Duration, client: ClientId) -> Vec<Action> {
        if self.reserved_by != Some(client) {
            return respond(Err(Refusal::NotReserved));
        }
        if self.enabled {
            return respond(Ok(()));
        }
        if self.config.data_set.is_none() {
            return respond(Err(Refusal::DataSetNull));
        }
        self.enabled = true;
        self.sequence = 0;
        let mut actions = Vec::new();
        if self.config.trg_ops.integrity && !self.config.intg_pd.is_zero() {
            actions.push(Action::Arm {
                timer: Timer::Integrity,
                at: now.saturating_add(self.config.intg_pd),
            });
        }
        actions.push(Action::Respond(Ok(())));
        actions
    }

    /// `RptEna` FALSE: the block stops, its timers stop, and its pending notifications are dropped.
    fn disable(&mut self) -> Vec<Action> {
        self.enabled = false;
        self.pending.clear();
        self.buffering = false;
        vec![
            Action::Disarm(Timer::BufferTime),
            Action::Disarm(Timer::Integrity),
        ]
    }

    /// A configuration attribute: written by the holder of the reservation, and only while the block is disabled.
    fn configure(&mut self, client: ClientId, write: Write) -> Vec<Action> {
        match self.reserved_by {
            None => return respond(Err(Refusal::NotReserved)),
            Some(holder) if holder != client => return respond(Err(Refusal::Reserved)),
            Some(_) => {}
        }
        if self.enabled {
            return respond(Err(Refusal::Enabled));
        }
        match write {
            Write::RptId(id) => self.config.rpt_id = id,
            Write::DataSet(None) => self.config.data_set = None,
            Write::DataSet(Some(name)) => {
                if !self.config.data_sets.contains_key(&name) {
                    return respond(Err(Refusal::UnknownDataSet));
                }
                self.config.data_set = Some(name);
            }
            Write::OptFields(fields) => self.config.opt_flds = fields,
            Write::BufTm(time) => self.config.buf_tm = time,
            Write::TrgOps(options) => self.config.trg_ops = options,
            Write::IntgPd(period) => self.config.intg_pd = period,
            // Reserve and RptEna are handled by `request`; GI by `general_interrogation`.
            Write::Reserve(_) | Write::RptEna(_) | Write::GeneralInterrogation => {}
        }
        respond(Ok(()))
    }

    /// `GI` TRUE (figure 30): refused while disabled; with the general-interrogation trigger on, every member is reported,
    /// after the buffered notifications. The attribute is set back to FALSE in every case.
    fn general_interrogation(&mut self, now: Duration, client: ClientId) -> Vec<Action> {
        if self.reserved_by.is_some_and(|holder| holder != client) {
            return respond(Err(Refusal::Reserved));
        }
        if !self.enabled {
            return respond(Err(Refusal::GeneralInterrogationNotAllowed));
        }
        let mut actions = Vec::new();
        if self.config.trg_ops.general_interrogation {
            actions.extend(self.flush(now));
            let entries = self.all_members(Reason::GeneralInterrogation);
            actions.push(self.report(now, ReportKind::GeneralInterrogation, entries));
        }
        actions.push(Action::Respond(Ok(())));
        actions
    }

    /// An internal notification: with a buffer time, the notifications of the interval are combined; a second notification of
    /// the same member flushes the pending report first (8-1 17.2.2.9, the strict reading for status values).
    fn notification(&mut self, now: Duration, member: usize, trigger: Trigger) -> Vec<Action> {
        if !self.enabled || member >= self.members() || !self.triggers(trigger) {
            return Vec::new();
        }
        let reason = Reason::from(trigger);
        if self.config.buf_tm.is_zero() {
            let entries = vec![Entry {
                member,
                reasons: BTreeSet::from([reason]),
            }];
            return vec![self.report(now, ReportKind::Notification, entries)];
        }
        let mut actions = Vec::new();
        if self.pending.contains_key(&member) {
            actions.extend(self.flush(now));
            actions.push(Action::Arm {
                timer: Timer::BufferTime,
                at: now.saturating_add(self.config.buf_tm),
            });
            self.buffering = true;
        } else if !self.buffering {
            self.buffering = true;
            actions.push(Action::Arm {
                timer: Timer::BufferTime,
                at: now.saturating_add(self.config.buf_tm),
            });
        }
        self.pending.entry(member).or_default().insert(reason);
        actions
    }

    /// The pending notifications, as one report, if there are any.
    fn flush(&mut self, now: Duration) -> Option<Action> {
        if self.pending.is_empty() {
            self.buffering = false;
            return None;
        }
        let pending = std::mem::take(&mut self.pending);
        self.buffering = false;
        let entries = pending
            .into_iter()
            .map(|(member, reasons)| Entry { member, reasons })
            .collect();
        Some(self.report(now, ReportKind::Notification, entries))
    }

    /// The integrity period: the buffered entries first, then every member; the period starts again.
    fn integrity(&mut self, now: Duration) -> Vec<Action> {
        if !self.enabled || !self.config.trg_ops.integrity || self.config.intg_pd.is_zero() {
            return Vec::new();
        }
        let mut actions = Vec::new();
        if let Some(action) = self.flush(now) {
            actions.push(Action::Disarm(Timer::BufferTime));
            actions.push(action);
        }
        let entries = self.all_members(Reason::Integrity);
        actions.push(self.report(now, ReportKind::Integrity, entries));
        actions.push(Action::Arm {
            timer: Timer::Integrity,
            at: now.saturating_add(self.config.intg_pd),
        });
        actions
    }

    /// The association of a client is lost: its reservation is released, and a block it enabled is disabled.
    fn lost(&mut self, client: ClientId) -> Vec<Action> {
        if self.reserved_by != Some(client) {
            return Vec::new();
        }
        self.reserved_by = None;
        if self.enabled {
            self.disable()
        } else {
            Vec::new()
        }
    }

    fn triggers(&self, trigger: Trigger) -> bool {
        match trigger {
            Trigger::DataChange => self.config.trg_ops.data_change,
            Trigger::QualityChange => self.config.trg_ops.quality_change,
            Trigger::DataUpdate => self.config.trg_ops.data_update,
        }
    }

    fn all_members(&self, reason: Reason) -> Vec<Entry> {
        (0..self.members())
            .map(|member| Entry {
                member,
                reasons: BTreeSet::from([reason]),
            })
            .collect()
    }

    /// A report, with the next sequence number (INT8U: it wraps after 255).
    fn report(&mut self, now: Duration, kind: ReportKind, entries: Vec<Entry>) -> Action {
        let sequence = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        Action::Report(Report {
            at: now,
            sequence,
            kind,
            entries,
        })
    }
}

fn respond(outcome: Result<(), Refusal>) -> Vec<Action> {
    vec![Action::Respond(outcome)]
}
