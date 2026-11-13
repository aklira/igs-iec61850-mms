// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The buffered report control block (BRCB), as a sans-I/O state machine (task B1; BRCB-001).
//!
//! A BRCB keeps a bounded history of entries, one per notification (or per buffer-time window, or per integrity period), so that a
//! client that was away can resume without losing an entry it has not seen. The machine takes the same events as the unbuffered block
//! (a request, a notification, a timer, the loss of an association) with the current time, and returns the actions: the responses,
//! the reports to send, and the timers to arm or disarm. It reads no clock and waits for nothing.
//!
//! The states and their transitions follow IEC 61850-7-2 §17.2.2.5 and figure 26: `disabled` (the entries are buffered, nothing is
//! sent), `resync` (the client has set an `EntryID` that is in the history), and `enabled` (the reports are sent as the entries are
//! made). An association loss returns the block to `disabled` and keeps the history (§17.2.3.2.3.6). The history is bounded: when it
//! is full the oldest entry is discarded, and an entry that was not yet sent makes the next report carry `BufOvfl` (§17.2.3.2.2.8).
//! The rules and the decisions where the text is open are in `PROVENANCE.md` (D-037).
//!
//! The reservation (`ResvTms`, §17.2.2.17) and the general interrogation (`GI`, figure 30) are in task Q4 (D-044). Not in this release:
//! the owner of the block (`Owner`), the restart of a general interrogation that is being sent, and the MMS encoding of the entries (task B2).
//! The sequence number is 16 bits (table 70, `SqNum` INT16U).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use super::urcb::{Entry, Reason, ReportKind, Timer, Trigger, TriggerOptions};
use crate::control::ClientId;

/// An entry identifier (`EntryID`). The value zero is the NULL identifier, reserved for an empty buffer and for the resynchronisation
/// to the oldest entry (§17.2.2.15); no entry has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct EntryId(pub u64);

impl EntryId {
    /// The NULL identifier.
    pub const NULL: EntryId = EntryId(0);

    /// Whether this is the NULL identifier.
    pub fn is_null(self) -> bool {
        self.0 == 0
    }
}

/// The optional fields of a buffered report (table 41 of 8-1, with the bits of the buffered block: buffer overflow, entry identifier and
/// segmentation as well as the fields of the unbuffered one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BufferedFields {
    /// `seq-num`: the sequence number is included.
    pub seq_num: bool,
    /// `report-time-stamp`: the time of entry is included.
    pub time_stamp: bool,
    /// `reason-for-inclusion`: the reasons are included.
    pub reason_code: bool,
    /// `data-set-name`: the data set is included.
    pub data_set_name: bool,
    /// `data-reference`: the reference of each value is included.
    pub data_ref: bool,
    /// `buffer-overflow`: `BufOvfl` is included.
    pub buf_ovfl: bool,
    /// `entryID`: the entry identifier is included.
    pub entry_id: bool,
    /// `conf-revision`: the configuration revision is included.
    pub conf_rev: bool,
    /// `segmentation`: the subsequence number and `MoreSegmentsFollow` are included.
    pub segmentation: bool,
}

/// The configuration of a buffered block (table 70).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The report identifier (`RptID`).
    pub rpt_id: String,
    /// The data set, by name; `None` is the NULL value.
    pub data_set: Option<String>,
    /// The data sets that exist in the logical node, with their numbers of members.
    pub data_sets: BTreeMap<String, usize>,
    /// The optional fields of the reports.
    pub opt_flds: BufferedFields,
    /// The buffer time (zero: each notification is one entry).
    pub buf_tm: Duration,
    /// The integrity period (zero: no integrity entries).
    pub intg_pd: Duration,
    /// The trigger options.
    pub trg_ops: TriggerOptions,
    /// The configuration revision.
    pub conf_rev: u32,
}

/// The state of a buffered block (figure 26).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The entries are buffered; no report is sent.
    Disabled,
    /// The client has set an `EntryID` that is in the history; the next enabling starts after it.
    Resync,
    /// The reports are sent for the buffered entries and for the new ones.
    Enabled,
}

/// An entry of the history: its identifier, its time of entry (the time of its notification or window), its kind, and the members
/// it includes with the reasons of each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buffered {
    /// The identifier of the entry.
    pub id: EntryId,
    /// The time at which the entry was created (`TimeOfEntry`).
    pub time_of_entry: Duration,
    /// Why the entry was made.
    pub kind: ReportKind,
    /// The members of the entry, in the order of the data set.
    pub entries: Vec<Entry>,
}

/// A write of a client to the block: one attribute, with its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// `RptEna`: enable (`true`) or disable (`false`).
    RptEna(bool),
    /// `RptID` (c1, c2 of table 70: refused while enabled, and a change purges the buffer).
    RptId(String),
    /// `DatSet`: the data set, or `None` for the NULL value (a change purges the buffer).
    DataSet(Option<String>),
    /// `OptFlds` (refused while enabled).
    OptFields(BufferedFields),
    /// `BufTm` (a change purges the buffer).
    BufTm(Duration),
    /// `TrgOps` (a change purges the buffer).
    TrgOps(TriggerOptions),
    /// `IntgPd` (a change purges the buffer).
    IntgPd(Duration),
    /// `PurgeBuf`: discard the buffered entries (refused while enabled).
    PurgeBuf,
    /// `EntryID`: the entry to resynchronise to (the NULL identifier for the oldest one). Refused while enabled.
    EntryId(EntryId),
    /// `ResvTms`: the reservation of the writer for this many seconds, kept after the loss of its association; zero releases it
    /// (§17.2.2.17).
    ReserveTime(u16),
    /// `GI`: the request of a general interrogation (figure 30).
    GeneralInterrogation,
}

/// Why a write is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The attribute is written only while the block is disabled (`RptEna` is FALSE).
    Enabled,
    /// The block cannot be enabled without a data set.
    DataSetNull,
    /// The data set does not exist in the logical node.
    UnknownDataSet,
    /// The `EntryID` is not in the history. From `disabled` the state stays; from `resync` the state goes back to `disabled`
    /// (§17.2.2.15, figure 26). The MMS error depends on the state, which the encoder reports.
    EntryNotInQueue {
        /// The state before the write.
        from: State,
    },
    /// The block is reserved by another client: no other client writes it (§17.2.2.17).
    Reserved,
    /// A general interrogation is refused while the block is disabled (figure 30: NO GI ALLOWED).
    GeneralInterrogationNotAllowed,
}

/// Events for the block.
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

/// A report of the block: one entry, with the sequence number and the flags of its report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferedReport {
    /// The time at which the report is formatted.
    pub at: Duration,
    /// The sequence number (`SqNum`), before this report.
    pub sequence: u16,
    /// The identifier of the entry sent.
    pub entry_id: EntryId,
    /// The time of entry (`TimeOfEntry`).
    pub time_of_entry: Duration,
    /// Whether entries may have been lost before this one (`BufOvfl`).
    pub buf_ovfl: bool,
    /// Why the entry was made.
    pub kind: ReportKind,
    /// The members of the entry.
    pub entries: Vec<Entry>,
}

/// An action of the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The answer to the write of a client.
    Respond(Result<(), Refusal>),
    /// A report to send.
    Report(BufferedReport),
    /// Arm a timer, to fire at this time.
    Arm {
        /// The timer.
        timer: Timer,
        /// The time of its expiry.
        at: Duration,
    },
    /// Disarm a timer.
    Disarm(Timer),
    /// An entry was made: the caller reads the members now and keeps their values with the entry. A notification or an integrity entry
    /// carries the values of its moment, not those of the report (D-037).
    Entered {
        /// The identifier of the entry.
        id: EntryId,
        /// The members of the entry, in the order of the data set.
        members: Vec<usize>,
    },
}

/// The window of buffer time that collects the notifications of one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Window {
    /// The time at which the window closes.
    deadline: Duration,
    /// The time of entry: the first notification of the window.
    start: Duration,
    /// The members of the window, with their reasons.
    members: BTreeMap<usize, BTreeSet<Reason>>,
}

/// A buffered report control block.
#[derive(Debug, Clone)]
pub struct Brcb {
    config: Config,
    capacity: usize,
    state: State,
    /// The history, the oldest entry first.
    queue: VecDeque<Buffered>,
    /// The identifier of the next entry to be made; identifiers start at 1.
    next_id: u64,
    /// The identifier of the next entry to format for transmission.
    pointer: u64,
    /// Whether the next report carries `BufOvfl`.
    overflow: bool,
    /// The sequence number of the next report.
    sequence: u16,
    /// The last entry formatted for transmission, while enabled.
    last_formatted: EntryId,
    window: Option<Window>,
    /// The time of the next integrity entry, when the integrity period is set.
    integrity_due: Option<Duration>,
    /// The client that holds the reservation (`ResvTms`), and the time it asked for in seconds.
    reserved_by: Option<ClientId>,
    reserve_time: u16,
    /// The client that enabled the block (`Owner`): its association loss disables the block.
    owner: Option<ClientId>,
}

impl Brcb {
    /// A block with the configuration, which keeps at most `capacity` entries (at least one).
    pub fn new(config: Config, capacity: usize) -> Self {
        Self {
            config,
            capacity: capacity.max(1),
            state: State::Disabled,
            queue: VecDeque::new(),
            next_id: 1,
            pointer: 1,
            overflow: false,
            sequence: 0,
            last_formatted: EntryId::NULL,
            window: None,
            integrity_due: None,
            reserved_by: None,
            reserve_time: 0,
            owner: None,
        }
    }

    /// Starts the integrity period, when one is configured; the caller calls it once, when the block is served.
    pub fn start(&mut self, now: Duration) -> Vec<Action> {
        self.integrity_actions(now)
    }

    /// The state of the block.
    pub fn state(&self) -> State {
        self.state
    }

    /// The configuration of the block.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Whether the block is enabled (`RptEna`).
    pub fn is_enabled(&self) -> bool {
        self.state == State::Enabled
    }

    /// The sequence number of the next report (`SqNum`).
    pub fn sequence(&self) -> u16 {
        self.sequence
    }

    /// The entries of the history, the oldest first.
    pub fn history(&self) -> impl Iterator<Item = &Buffered> {
        self.queue.iter()
    }

    /// The `EntryID` that `GetBRCBValues` reports (§17.2.2.15): the newest entry made while disabled, and the last one formatted
    /// while enabled. The NULL identifier when there is none.
    pub fn last_entry_id(&self) -> EntryId {
        match self.state {
            State::Enabled => self.last_formatted,
            State::Disabled | State::Resync => {
                self.queue.back().map_or(EntryId::NULL, |entry| entry.id)
            }
        }
    }

    /// Handles an event at the given time, and returns the actions to take.
    pub fn handle(&mut self, now: Duration, event: Event) -> Vec<Action> {
        match event {
            Event::Request { client, write } => self.request(now, client, write),
            Event::Notification { member, trigger } => self.notification(now, member, trigger),
            Event::Timer(Timer::BufferTime) => self.buffer_timer(now),
            Event::Timer(Timer::Integrity) => self.integrity(now),
            Event::Timer(Timer::Reservation) => self.expire_reservation(),
            Event::AssociationLost(client) => {
                // Only the client that enabled the block stops its reports; the reservation is handled for any client.
                if self.owner == Some(client) {
                    self.state = State::Disabled;
                    self.owner = None;
                }
                self.lost(now, client)
            }
        }
    }

    /// The client that holds the reservation (`ResvTms`), if any. A holder whose association is lost keeps it for `ResvTms` seconds.
    pub fn reserved_by(&self) -> Option<ClientId> {
        self.reserved_by
    }

    /// The reservation time of the block (`ResvTms`) in seconds: the time the holder asked for, or zero when the block is free.
    pub fn reserve_time(&self) -> u16 {
        self.reserve_time
    }

    fn request(&mut self, now: Duration, client: ClientId, write: Write) -> Vec<Action> {
        if let Write::ReserveTime(seconds) = write {
            return self.reserve(client, seconds);
        }
        // While a block is reserved, no other client writes it (§17.2.2.17).
        if self.reserved_by.is_some_and(|holder| holder != client) {
            return respond(Err(Refusal::Reserved));
        }
        match write {
            Write::GeneralInterrogation => self.general_interrogation(now),
            Write::RptEna(true) => self.enable(now, client),
            Write::RptEna(false) => {
                self.state = State::Disabled;
                self.owner = None;
                respond(Ok(()))
            }
            Write::EntryId(id) => self.resynchronise(id),
            Write::PurgeBuf if self.state == State::Enabled => respond(Err(Refusal::Enabled)),
            Write::PurgeBuf => {
                let mut actions = self.purge();
                actions.extend(respond(Ok(())));
                actions
            }
            configuration => self.configure(now, configuration),
        }
    }

    /// `RptEna` = TRUE: from `disabled` the reports start with the oldest entry, and the first one carries `BufOvfl` (the block is
    /// not resynchronised, §17.2.3.2.2.8); from `resync` they start after the entry the client set.
    fn enable(&mut self, now: Duration, client: ClientId) -> Vec<Action> {
        if self.state == State::Enabled {
            return respond(Ok(()));
        }
        match self.config.data_set.as_ref() {
            None => return respond(Err(Refusal::DataSetNull)),
            Some(name) if !self.config.data_sets.contains_key(name) => {
                return respond(Err(Refusal::UnknownDataSet))
            }
            Some(_) => {}
        }
        if self.state == State::Disabled {
            self.pointer = self.queue.front().map_or(self.next_id, |entry| entry.id.0);
            self.overflow = true;
        }
        self.state = State::Enabled;
        self.owner = Some(client);
        self.sequence = 0;
        let mut actions = respond(Ok(()));
        actions.extend(self.transmit(now));
        actions
    }

    /// `EntryID`: the NULL identifier moves to the oldest entry (`disabled` to `resync`, and `resync` back to `disabled`, as the text
    /// of figure 26 has it); an identifier in the history moves to `resync` after it; any other identifier is refused.
    fn resynchronise(&mut self, id: EntryId) -> Vec<Action> {
        if self.state == State::Enabled {
            return respond(Err(Refusal::Enabled));
        }
        if id.is_null() {
            self.pointer = self.queue.front().map_or(self.next_id, |entry| entry.id.0);
            self.state = match self.state {
                State::Resync => State::Disabled,
                _ => State::Resync,
            };
            return respond(Ok(()));
        }
        if self.queue.iter().any(|entry| entry.id == id) {
            self.pointer = id.0.saturating_add(1);
            self.state = State::Resync;
            return respond(Ok(()));
        }
        let from = self.state;
        if from == State::Resync {
            self.state = State::Disabled;
        }
        respond(Err(Refusal::EntryNotInQueue { from }))
    }

    /// The configuration attributes: refused while enabled; a change of a purging attribute discards the buffer.
    fn configure(&mut self, now: Duration, write: Write) -> Vec<Action> {
        if self.state == State::Enabled {
            return respond(Err(Refusal::Enabled));
        }
        // The integrity period restarts only when its trigger option or its period changes.
        let mut schedule = false;
        let changed = match write {
            Write::RptId(id) => {
                let changed = id != self.config.rpt_id;
                self.config.rpt_id = id;
                changed
            }
            Write::DataSet(name) => {
                if let Some(name) = name.as_ref() {
                    if !self.config.data_sets.contains_key(name) {
                        return respond(Err(Refusal::UnknownDataSet));
                    }
                }
                let changed = name != self.config.data_set;
                self.config.data_set = name;
                changed
            }
            Write::OptFields(fields) => {
                self.config.opt_flds = fields;
                false
            }
            Write::BufTm(period) => {
                let changed = period != self.config.buf_tm;
                self.config.buf_tm = period;
                changed
            }
            Write::TrgOps(options) => {
                let changed = options != self.config.trg_ops;
                self.config.trg_ops = options;
                schedule = true;
                changed
            }
            Write::IntgPd(period) => {
                let changed = period != self.config.intg_pd;
                self.config.intg_pd = period;
                schedule = true;
                changed
            }
            // Handled by `request`; these are not configuration.
            Write::RptEna(_)
            | Write::EntryId(_)
            | Write::PurgeBuf
            | Write::ReserveTime(_)
            | Write::GeneralInterrogation => false,
        };
        let mut actions = Vec::new();
        if changed {
            actions.extend(self.purge());
        }
        if schedule {
            actions.extend(self.integrity_actions(now));
        }
        actions.extend(respond(Ok(())));
        actions
    }

    /// `ResvTms` (§17.2.2.17): a free block is reserved for the writer, the holder changes its time, and zero releases the block
    /// (a release of a free block is a positive answer). Another client is refused while the block is reserved.
    fn reserve(&mut self, client: ClientId, seconds: u16) -> Vec<Action> {
        if self.reserved_by.is_some_and(|holder| holder != client) {
            return respond(Err(Refusal::Reserved));
        }
        let mut actions = Vec::new();
        // A holder that writes again has its association: a reservation kept after a loss is not needed any more.
        if self.reserved_by.is_some() {
            actions.push(Action::Disarm(Timer::Reservation));
        }
        if seconds == 0 {
            self.reserved_by = None;
            self.reserve_time = 0;
        } else {
            self.reserved_by = Some(client);
            self.reserve_time = seconds;
        }
        actions.extend(respond(Ok(())));
        actions
    }

    /// The association of a client is lost. A holder that asked for a time keeps the reservation for that many seconds (the timer
    /// releases it, `expire_reservation`); a holder that asked for none loses it at once.
    fn lost(&mut self, now: Duration, client: ClientId) -> Vec<Action> {
        if self.reserved_by != Some(client) {
            return Vec::new();
        }
        if self.reserve_time == 0 {
            self.reserved_by = None;
            return Vec::new();
        }
        let at = now.saturating_add(Duration::from_secs(u64::from(self.reserve_time)));
        vec![Action::Arm {
            timer: Timer::Reservation,
            at,
        }]
    }

    /// The reservation timer expired: the block is free, and `ResvTms` reads zero (§17.2.2.17).
    fn expire_reservation(&mut self) -> Vec<Action> {
        self.reserved_by = None;
        self.reserve_time = 0;
        Vec::new()
    }

    /// `GI` TRUE (figure 30). Refused while the block is disabled. While enabled, with the general-interrogation trigger on, the
    /// buffered entries are sent first (the open window is closed), then an entry of all the members with the reason
    /// general-interrogation, which is sent with its `EntryID` (D-044).
    fn general_interrogation(&mut self, now: Duration) -> Vec<Action> {
        if self.state != State::Enabled {
            return respond(Err(Refusal::GeneralInterrogationNotAllowed));
        }
        let mut actions = Vec::new();
        if self.config.trg_ops.general_interrogation {
            actions.extend(self.close_window(now));
            actions.extend(self.general_report(now));
        }
        actions.extend(respond(Ok(())));
        actions
    }

    /// The general-interrogation entry: every member, with the reason general-interrogation. The entry is made in the history like the
    /// others (the reference peer gives it an `EntryID`, D-044): it is sent now, while enabled, with the next sequence number.
    fn general_report(&mut self, now: Duration) -> Vec<Action> {
        let Some(count) = self.member_count() else {
            return Vec::new();
        };
        let members: BTreeMap<usize, BTreeSet<Reason>> = (0..count)
            .map(|member| (member, BTreeSet::from([Reason::GeneralInterrogation])))
            .collect();
        self.push(now, now, ReportKind::GeneralInterrogation, members)
    }

    /// `PurgeBuf`: the history, the open window and the overflow are discarded; the next entry starts a new history.
    fn purge(&mut self) -> Vec<Action> {
        self.queue.clear();
        self.overflow = false;
        self.pointer = self.next_id;
        self.last_formatted = EntryId::NULL;
        match self.window.take() {
            Some(_) => vec![Action::Disarm(Timer::BufferTime)],
            None => Vec::new(),
        }
    }

    fn notification(&mut self, now: Duration, member: usize, trigger: Trigger) -> Vec<Action> {
        let options = self.config.trg_ops;
        let triggered = match trigger {
            Trigger::DataChange => options.data_change,
            Trigger::QualityChange => options.quality_change,
            Trigger::DataUpdate => options.data_update,
        };
        if !triggered {
            return Vec::new();
        }
        let Some(count) = self.member_count() else {
            return Vec::new();
        };
        if member >= count {
            return Vec::new();
        }
        let reason = Reason::from(trigger);
        if self.config.buf_tm.is_zero() {
            let mut members = BTreeMap::new();
            members.insert(member, BTreeSet::from([reason]));
            return self.push(now, now, ReportKind::Notification, members);
        }
        let mut actions = Vec::new();
        // A second notification of a member within the window ends the window at once, and starts the next one (§17.2.2.9, the rule
        // for status values).
        if self
            .window
            .as_ref()
            .is_some_and(|window| window.members.contains_key(&member))
        {
            actions.extend(self.close_window(now));
        }
        match self.window.as_mut() {
            Some(window) => {
                window.members.entry(member).or_default().insert(reason);
            }
            None => {
                let deadline = now.saturating_add(self.config.buf_tm);
                let members = BTreeMap::from([(member, BTreeSet::from([reason]))]);
                self.window = Some(Window {
                    deadline,
                    start: now,
                    members,
                });
                actions.push(Action::Arm {
                    timer: Timer::BufferTime,
                    at: deadline,
                });
            }
        }
        actions
    }

    /// The buffer-time timer: a timer that fires before the deadline of the open window is stale, and is ignored.
    fn buffer_timer(&mut self, now: Duration) -> Vec<Action> {
        match self.window.as_ref() {
            Some(window) if window.deadline <= now => self.close_window(now),
            _ => Vec::new(),
        }
    }

    /// Closes the open window: its members become one entry, timed at the window's start.
    fn close_window(&mut self, now: Duration) -> Vec<Action> {
        let Some(window) = self.window.take() else {
            return Vec::new();
        };
        let mut actions = vec![Action::Disarm(Timer::BufferTime)];
        actions.extend(self.push(now, window.start, ReportKind::Notification, window.members));
        actions
    }

    /// The integrity timer: an entry of all members, then the next period.
    fn integrity(&mut self, now: Duration) -> Vec<Action> {
        match self.integrity_due {
            Some(due) if due <= now => {}
            _ => return Vec::new(),
        }
        let Some(count) = self.member_count() else {
            return Vec::new();
        };
        let members: BTreeMap<usize, BTreeSet<Reason>> = (0..count)
            .map(|member| (member, BTreeSet::from([Reason::Integrity])))
            .collect();
        let mut actions = self.push(now, now, ReportKind::Integrity, members);
        actions.extend(self.integrity_actions(now));
        actions
    }

    /// Arms the integrity timer for the next period, or disarms it when the trigger option or the period is not set.
    fn integrity_actions(&mut self, now: Duration) -> Vec<Action> {
        if self.config.trg_ops.integrity && !self.config.intg_pd.is_zero() {
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

    /// The number of members of the configured data set, when it exists.
    fn member_count(&self) -> Option<usize> {
        let name = self.config.data_set.as_ref()?;
        self.config.data_sets.get(name).copied()
    }

    /// Makes an entry of the members, enters it into the history, and sends what is due.
    fn push(
        &mut self,
        now: Duration,
        time_of_entry: Duration,
        kind: ReportKind,
        members: BTreeMap<usize, BTreeSet<Reason>>,
    ) -> Vec<Action> {
        if members.is_empty() {
            return Vec::new();
        }
        let id = EntryId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let listed: Vec<usize> = members.keys().copied().collect();
        let entries = members
            .into_iter()
            .map(|(member, reasons)| Entry { member, reasons })
            .collect();
        self.queue.push_back(Buffered {
            id,
            time_of_entry,
            kind,
            entries,
        });
        let mut actions = vec![Action::Entered {
            id,
            members: listed,
        }];
        while self.queue.len() > self.capacity {
            let Some(oldest) = self.queue.pop_front() else {
                break;
            };
            // An entry that was not yet formatted is lost: the next report says so. One already sent is not a loss.
            if oldest.id.0 >= self.pointer {
                self.overflow = true;
                self.pointer = oldest.id.0.saturating_add(1);
            }
        }
        actions.extend(self.transmit(now));
        actions
    }

    /// While enabled, formats every entry that is not yet sent, in the order of the history.
    fn transmit(&mut self, now: Duration) -> Vec<Action> {
        if self.state != State::Enabled {
            return Vec::new();
        }
        let due: Vec<Buffered> = self
            .queue
            .iter()
            .filter(|entry| entry.id.0 >= self.pointer)
            .cloned()
            .collect();
        let mut actions = Vec::with_capacity(due.len());
        for entry in due {
            self.pointer = entry.id.0.saturating_add(1);
            self.last_formatted = entry.id;
            let sequence = self.sequence;
            self.sequence = self.sequence.wrapping_add(1);
            let buf_ovfl = std::mem::take(&mut self.overflow);
            actions.push(Action::Report(BufferedReport {
                at: now,
                sequence,
                entry_id: entry.id,
                time_of_entry: entry.time_of_entry,
                buf_ovfl,
                kind: entry.kind,
                entries: entry.entries,
            }));
        }
        actions
    }
}

/// The response to a write.
fn respond(result: Result<(), Refusal>) -> Vec<Action> {
    vec![Action::Respond(result)]
}
