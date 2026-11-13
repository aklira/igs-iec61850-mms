// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The log (`LOG`) and its control block (`LCB`), as sans-I/O state machines (task Q5; 7-2 §17.3, figure 36).
//!
//! A log is a first-in first-out buffer of bounded size: when it is full the oldest entry is overwritten, and the identifiers keep
//! increasing (§17.3.1.2). A control block records the changes of the members of its data set into a log, with the trigger options it
//! monitors, and records each transition of `LogEna` (§17.3.2, §17.3.5). Logging does not depend on the associations of the clients:
//! the loss of an association changes nothing here. The general interrogation is not supported for logging (§17.3.2, `TrgOps`), and the
//! buffer time is not supported by this release. The decisions are in `PROVENANCE.md` (D-045). The values of the members are not in
//! this module: the caller reads them when a record is appended, as for the buffered reports.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use crate::report::brcb::EntryId;
use crate::report::urcb::{Reason, Timer, Trigger, TriggerOptions};

// ---------------------------------------------------------------------------------------------------------------------------------
// The log
// ---------------------------------------------------------------------------------------------------------------------------------

/// A member of a data set, as a log entry holds it: its position in the data set, and the reasons it was logged for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The position of the member in the data set.
    pub member: usize,
    /// The reasons for the inclusion of the member (`ReasonCode`, §17.3.5).
    pub reasons: BTreeSet<Reason>,
}

/// What an entry holds (§17.3.4, `EntryData`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// The members that a control block logged, in the order of its data set (`EntryData`, the first choice).
    Members(Vec<Member>),
    /// A transition of `LogEna` of the control block with this name (`EntryData`, the second choice).
    Transition {
        /// The name of the control block.
        lcb: String,
        /// The value of `LogEna` after the transition.
        enabled: bool,
    },
}

/// An entry of a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// The identifier of the entry (`EntryID`). Identifiers increase; they are not reused.
    pub id: EntryId,
    /// The time at which the entry was added to the log (`TimeOfEntry`).
    pub time_of_entry: Duration,
    /// What the entry holds.
    pub content: Content,
}

/// The status of a log (`GetLogStatusValues`, §17.3.6.4): the oldest and the newest entries. An empty log has zero times and the NULL
/// identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogStatus {
    /// The time at which the oldest entry was added (`OldEntrTm`).
    pub oldest_time: Duration,
    /// The time at which the newest entry was added (`NewEntrTm`).
    pub newest_time: Duration,
    /// The identifier of the oldest entry (`OldEntr`).
    pub oldest: EntryId,
    /// The identifier of the newest entry (`NewEntr`).
    pub newest: EntryId,
}

/// Why a query is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryError {
    /// The entry of a query after an entry is not in the log, or it does not have the time given with it (§17.3.6.3).
    UnknownEntry,
}

/// A log: a first-in first-out buffer of bounded size, whose oldest entry is overwritten when it is full (§17.3.1.2).
#[derive(Debug, Clone)]
pub struct Log {
    capacity: usize,
    entries: VecDeque<LogEntry>,
    next_id: u64,
}

impl Log {
    /// An empty log that keeps at most `capacity` entries (at least one).
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
            next_id: 1,
        }
    }

    /// Adds an entry at the time given, and returns its identifier. The oldest entry is overwritten when the log is full.
    pub fn append(&mut self, time_of_entry: Duration, content: Content) -> EntryId {
        let id = EntryId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push_back(LogEntry {
            id,
            time_of_entry,
            content,
        });
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        id
    }

    /// The entries of the log, the oldest first.
    pub fn entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }

    /// The number of entries held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log holds no entry.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries whose time of entry is in the range, both ends included. A missing end is open (§17.3.6.2).
    pub fn query_by_time(&self, start: Option<Duration>, stop: Option<Duration>) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                start.is_none_or(|start| entry.time_of_entry >= start)
                    && stop.is_none_or(|stop| entry.time_of_entry <= stop)
            })
            .collect()
    }

    /// The entries that follow the entry given: the entry must be in the log, with the time given (§17.3.6.3). The entry itself is
    /// not returned.
    pub fn query_after(
        &self,
        start: Duration,
        entry: EntryId,
    ) -> Result<Vec<&LogEntry>, QueryError> {
        let position = self
            .entries
            .iter()
            .position(|held| held.id == entry && held.time_of_entry == start)
            .ok_or(QueryError::UnknownEntry)?;
        Ok(self
            .entries
            .iter()
            .skip(position.saturating_add(1))
            .collect())
    }

    /// The status of the log (§17.3.6.4).
    pub fn status(&self) -> LogStatus {
        let oldest = self.entries.front();
        let newest = self.entries.back();
        LogStatus {
            oldest_time: oldest.map_or(Duration::ZERO, |entry| entry.time_of_entry),
            newest_time: newest.map_or(Duration::ZERO, |entry| entry.time_of_entry),
            oldest: oldest.map_or(EntryId::NULL, |entry| entry.id),
            newest: newest.map_or(EntryId::NULL, |entry| entry.id),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// The log control block
// ---------------------------------------------------------------------------------------------------------------------------------

/// The configuration of a log control block (§17.3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LcbConfig {
    /// The name of the control block, within its logical node.
    pub name: String,
    /// The log the control block records into (`LogRef`), as its name in the logical node.
    pub log_ref: Option<String>,
    /// The data set whose members are logged (`DatSet`).
    pub data_set: Option<String>,
    /// The data sets of the logical node, with their number of members.
    pub data_sets: BTreeMap<String, usize>,
    /// The trigger options (`TrgOps`).
    pub trg_ops: TriggerOptions,
    /// The integrity period (`IntgPd`); zero implies no integrity logging.
    pub intg_pd: Duration,
}

/// A write of a client to a log control block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// `LogEna`: start (`true`) or stop the recording.
    LogEna(bool),
    /// `LogRef`: the log to record into.
    LogRef(Option<String>),
    /// `DatSet`: the data set to log.
    DataSet(Option<String>),
    /// `TrgOps`.
    TrgOps(TriggerOptions),
    /// `IntgPd`.
    IntgPd(Duration),
}

/// Why a write is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The attribute is written only while the control block is disabled (`LogEna` is FALSE, §17.3.2).
    Enabled,
    /// The data set does not exist in the logical node.
    UnknownDataSet,
    /// The control block cannot record without its log and its data set.
    NotConfigured,
    /// The general interrogation is not supported for logging (§17.3.2).
    GeneralInterrogation,
}

/// An event for the control block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A write of a client.
    Request(Write),
    /// An internal notification of a member of the data set.
    Notification {
        /// The position of the member.
        member: usize,
        /// The trigger that produced it.
        trigger: Trigger,
    },
    /// The integrity period expired.
    Timer(Timer),
}

/// An action of the control block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The answer to the write of a client.
    Respond(Result<(), Refusal>),
    /// An entry to add to the log, at the time given.
    Record {
        /// The time of entry.
        time: Duration,
        /// What the entry holds.
        content: Content,
    },
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

/// A log control block: it records the changes of a data set into a log while it is enabled (`LogEna`).
#[derive(Debug, Clone)]
pub struct Lcb {
    config: LcbConfig,
    enabled: bool,
    integrity_due: Option<Duration>,
}

impl Lcb {
    /// A disabled control block with this configuration.
    pub fn new(config: LcbConfig) -> Self {
        Self {
            config,
            enabled: false,
            integrity_due: None,
        }
    }

    /// Whether the control block records (`LogEna`).
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The configuration of the control block.
    pub fn config(&self) -> &LcbConfig {
        &self.config
    }

    /// Handles an event at the given time, and returns the actions to take.
    pub fn handle(&mut self, now: Duration, event: Event) -> Vec<Action> {
        match event {
            Event::Request(write) => self.request(now, write),
            Event::Notification { member, trigger } => self.notification(now, member, trigger),
            Event::Timer(Timer::Integrity) => self.integrity(now),
            // Only the integrity period is timed for logging; the other timers belong to the reports.
            Event::Timer(_) => Vec::new(),
        }
    }

    fn request(&mut self, now: Duration, write: Write) -> Vec<Action> {
        match write {
            Write::LogEna(true) => self.start(now),
            Write::LogEna(false) => self.stop(now),
            configuration => self.configure(configuration),
        }
    }

    /// `LogEna` TRUE: the log and the data set must be set; the transition is recorded (§17.3.2).
    fn start(&mut self, now: Duration) -> Vec<Action> {
        if self.enabled {
            return respond(Ok(()));
        }
        let configured = self.config.log_ref.is_some()
            && self
                .config
                .data_set
                .as_ref()
                .is_some_and(|name| self.config.data_sets.contains_key(name));
        if !configured {
            return respond(Err(Refusal::NotConfigured));
        }
        self.enabled = true;
        let mut actions = self.transition(now, true);
        actions.extend(self.integrity_actions(now));
        actions.extend(respond(Ok(())));
        actions
    }

    /// `LogEna` FALSE: the recording stops, and the transition is recorded.
    fn stop(&mut self, now: Duration) -> Vec<Action> {
        if !self.enabled {
            return respond(Ok(()));
        }
        self.enabled = false;
        self.integrity_due = None;
        let mut actions = vec![Action::Disarm(Timer::Integrity)];
        actions.extend(self.transition(now, false));
        actions.extend(respond(Ok(())));
        actions
    }

    /// A configuration attribute: refused while enabled, and the general interrogation is not a trigger of logging.
    fn configure(&mut self, write: Write) -> Vec<Action> {
        if self.enabled {
            return respond(Err(Refusal::Enabled));
        }
        match write {
            Write::LogRef(name) => self.config.log_ref = name,
            Write::DataSet(name) => {
                if let Some(set) = name.as_ref() {
                    if !self.config.data_sets.contains_key(set) {
                        return respond(Err(Refusal::UnknownDataSet));
                    }
                }
                self.config.data_set = name;
            }
            Write::TrgOps(options) => {
                if options.general_interrogation {
                    return respond(Err(Refusal::GeneralInterrogation));
                }
                self.config.trg_ops = options;
            }
            Write::IntgPd(period) => self.config.intg_pd = period,
            Write::LogEna(_) => {}
        }
        // The integrity period follows the options when the block starts (`start`); nothing is timed while disabled.
        respond(Ok(()))
    }

    /// An internal notification: the change of a member is recorded when the trigger is monitored and the member is in the data set.
    fn notification(&mut self, now: Duration, member: usize, trigger: Trigger) -> Vec<Action> {
        let monitored = match trigger {
            Trigger::DataChange => self.config.trg_ops.data_change,
            Trigger::QualityChange => self.config.trg_ops.quality_change,
            Trigger::DataUpdate => self.config.trg_ops.data_update,
        };
        if !self.enabled || !monitored || member >= self.members() {
            return Vec::new();
        }
        vec![Action::Record {
            time: now,
            content: Content::Members(vec![Member {
                member,
                reasons: BTreeSet::from([Reason::from(trigger)]),
            }]),
        }]
    }

    /// The integrity period: every member is recorded with the reason integrity, then the next period is armed.
    fn integrity(&mut self, now: Duration) -> Vec<Action> {
        match self.integrity_due {
            Some(due) if due <= now => {}
            _ => return Vec::new(),
        }
        if !self.enabled || !self.config.trg_ops.integrity || self.members() == 0 {
            return Vec::new();
        }
        let members = (0..self.members())
            .map(|member| Member {
                member,
                reasons: BTreeSet::from([Reason::Integrity]),
            })
            .collect();
        let mut actions = vec![Action::Record {
            time: now,
            content: Content::Members(members),
        }];
        actions.extend(self.integrity_actions(now));
        actions
    }

    /// Arms the integrity timer for the next period, or disarms it when the trigger option or the period is not set.
    fn integrity_actions(&mut self, now: Duration) -> Vec<Action> {
        if self.enabled && self.config.trg_ops.integrity && !self.config.intg_pd.is_zero() {
            let due = now.saturating_add(self.config.intg_pd);
            self.integrity_due = Some(due);
            vec![Action::Arm {
                timer: Timer::Integrity,
                at: due,
            }]
        } else {
            self.integrity_due = None;
            vec![Action::Disarm(Timer::Integrity)]
        }
    }

    /// The number of members of the data set, when it exists.
    fn members(&self) -> usize {
        self.config
            .data_set
            .as_ref()
            .and_then(|name| self.config.data_sets.get(name))
            .copied()
            .unwrap_or(0)
    }

    fn transition(&self, now: Duration, enabled: bool) -> Vec<Action> {
        vec![Action::Record {
            time: now,
            content: Content::Transition {
                lcb: self.config.name.clone(),
                enabled,
            },
        }]
    }
}

/// The response to a write.
fn respond(result: Result<(), Refusal>) -> Vec<Action> {
    vec![Action::Respond(result)]
}
