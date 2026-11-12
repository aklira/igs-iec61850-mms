// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The buffered report control block with controlled time (task B1; BRCB-001): the history and its bound, the purge, the overflow,
//! the resynchronisation to an entry, and the resumption after the loss of an association.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use igs_iec61850_mms_core::control::ClientId;
use igs_iec61850_mms_core::report::brcb::{
    Action, Brcb, BufferedFields, BufferedReport, Config, EntryId, Event, Refusal, State, Write,
};
use igs_iec61850_mms_core::report::urcb::{Reason, ReportKind, Timer, Trigger, TriggerOptions};

const ALICE: ClientId = ClientId(1);

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn dchg() -> TriggerOptions {
    TriggerOptions {
        data_change: true,
        ..Default::default()
    }
}

/// A block on the data set DS1 of three members, which keeps `capacity` entries, with the given buffer time, integrity period and
/// trigger options.
fn block(capacity: usize, buf_ms: u64, intg_ms: u64, trg: TriggerOptions) -> Brcb {
    Brcb::new(
        Config {
            rpt_id: "RPT1".to_string(),
            data_set: Some("DS1".to_string()),
            data_sets: BTreeMap::from([("DS1".to_string(), 3)]),
            opt_flds: BufferedFields::default(),
            buf_tm: ms(buf_ms),
            intg_pd: ms(intg_ms),
            trg_ops: trg,
            conf_rev: 1,
        },
        capacity,
    )
}

fn write(at: u64, brcb: &mut Brcb, value: Write) -> Vec<Action> {
    brcb.handle(
        ms(at),
        Event::Request {
            client: ALICE,
            write: value,
        },
    )
}

fn change(at: u64, brcb: &mut Brcb, member: usize) -> Vec<Action> {
    brcb.handle(
        ms(at),
        Event::Notification {
            member,
            trigger: Trigger::DataChange,
        },
    )
}

/// The responses of a list of actions.
fn answers(actions: &[Action]) -> Vec<Result<(), Refusal>> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Respond(result) => Some(*result),
            _ => None,
        })
        .collect()
}

/// The reports of a list of actions.
fn reports(actions: &[Action]) -> Vec<BufferedReport> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Report(report) => Some(report.clone()),
            _ => None,
        })
        .collect()
}

fn ids(reports: &[BufferedReport]) -> Vec<u64> {
    reports.iter().map(|report| report.entry_id.0).collect()
}

/// A notification of a member is an entry on its own when there is no buffer time.
#[test]
fn a_notification_is_buffered_in_every_state_and_sent_only_when_enabled() {
    let mut brcb = block(8, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    assert!(
        reports(&change(10, &mut brcb, 0)).is_empty(),
        "disabled: nothing is sent"
    );
    assert_eq!(brcb.history().count(), 1);
    assert_eq!(brcb.state(), State::Disabled);

    let actions = write(20, &mut brcb, Write::RptEna(true));
    assert_eq!(answers(&actions), [Ok(())]);
    let sent = reports(&actions);
    assert_eq!(ids(&sent), [1], "the oldest entry is the first sent");
    assert!(
        sent[0].buf_ovfl,
        "a block enabled without resynchronisation says so in the first report"
    );
    assert_eq!(sent[0].sequence, 0);
    assert_eq!(sent[0].kind, ReportKind::Notification);
    assert_eq!(sent[0].entries[0].member, 0);
    assert_eq!(
        sent[0].entries[0].reasons,
        BTreeSet::from([Reason::DataChange])
    );
    assert_eq!(brcb.state(), State::Enabled);

    let live = reports(&change(30, &mut brcb, 2));
    assert_eq!(ids(&live), [2]);
    assert!(!live[0].buf_ovfl, "the flag is said once");
    assert_eq!(live[0].sequence, 1);
}

/// The buffer time combines the notifications of its window into one entry, timed at the window's start; a second notification of a
/// member closes the window at once, and a timer that fires before the deadline is ignored.
#[test]
fn a_buffer_time_combines_notifications_of_the_window_into_one_entry() {
    let mut brcb = block(8, 100, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    let opened = change(10, &mut brcb, 0);
    assert_eq!(
        opened,
        [Action::Arm {
            timer: Timer::BufferTime,
            at: ms(110)
        }]
    );
    assert!(reports(&change(20, &mut brcb, 1)).is_empty());
    assert_eq!(brcb.history().count(), 0, "the window is still open");

    let early = brcb.handle(ms(50), Event::Timer(Timer::BufferTime));
    assert!(early.is_empty(), "a stale timer is ignored");

    // Member 0 again inside the window: the window closes with {0, 1} and a new one opens at 30.
    let second = change(30, &mut brcb, 0);
    assert!(second.contains(&Action::Disarm(Timer::BufferTime)));
    assert!(second.contains(&Action::Arm {
        timer: Timer::BufferTime,
        at: ms(130)
    }));
    let entries: Vec<_> = brcb.history().cloned().collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, EntryId(1));
    assert_eq!(
        entries[0].time_of_entry,
        ms(10),
        "timed at the window's start"
    );
    assert_eq!(
        entries[0]
            .entries
            .iter()
            .map(|e| e.member)
            .collect::<Vec<_>>(),
        [0, 1]
    );

    let closed = brcb.handle(ms(130), Event::Timer(Timer::BufferTime));
    assert!(closed.contains(&Action::Disarm(Timer::BufferTime)));
    assert_eq!(brcb.history().count(), 2);
    assert_eq!(brcb.history().last().unwrap().time_of_entry, ms(30));
}

/// The history is bounded: the oldest entry goes when it is full, and a loss is reported when the entry had not been sent.
#[test]
fn the_history_is_bounded_and_an_unsent_entry_discarded_is_reported_as_a_loss() {
    let mut brcb = block(2, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    for (at, member) in [(1, 0), (2, 1), (3, 2)] {
        change(at, &mut brcb, member);
    }
    assert_eq!(
        brcb.history().map(|entry| entry.id.0).collect::<Vec<_>>(),
        [2, 3],
        "entry 1 was discarded"
    );
    let actions = write(4, &mut brcb, Write::RptEna(true));
    let sent = reports(&actions);
    assert_eq!(ids(&sent), [2, 3]);
    assert!(
        sent[0].buf_ovfl,
        "entry 1 was never sent: the next report says so"
    );
    assert!(!sent[1].buf_ovfl);
}

/// An entry already sent and then discarded is not a loss.
#[test]
fn a_discarded_entry_already_sent_is_not_a_loss() {
    let mut brcb = block(1, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    write(1, &mut brcb, Write::RptEna(true));
    assert_eq!(ids(&reports(&change(2, &mut brcb, 0))), [1]);
    let next = reports(&change(3, &mut brcb, 1));
    assert_eq!(ids(&next), [2]);
    assert!(
        !next[0].buf_ovfl,
        "entry 1 was sent before it was discarded"
    );
}

/// Resynchronisation: an entry of the history moves the block to resync, and the next enabling starts after that entry, without the
/// loss flag.
#[test]
fn a_resynchronisation_to_an_entry_starts_the_reports_after_it() {
    let mut brcb = block(8, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    for (at, member) in [(1, 0), (2, 1), (3, 2)] {
        change(at, &mut brcb, member);
    }
    assert_eq!(
        answers(&write(4, &mut brcb, Write::EntryId(EntryId(1)))),
        [Ok(())]
    );
    assert_eq!(brcb.state(), State::Resync);
    let sent = reports(&write(5, &mut brcb, Write::RptEna(true)));
    assert_eq!(ids(&sent), [2, 3], "after the entry the client set");
    assert!(
        sent.iter().all(|report| !report.buf_ovfl),
        "resynchronised: no loss flag"
    );
}

/// An identifier that is not in the history is refused; from resync the block goes back to disabled.
#[test]
fn an_entry_that_is_not_in_the_history_is_refused_and_resync_goes_back_to_disabled() {
    let mut brcb = block(8, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    change(1, &mut brcb, 0);
    let refused = write(2, &mut brcb, Write::EntryId(EntryId(99)));
    assert_eq!(
        answers(&refused),
        [Err(Refusal::EntryNotInQueue {
            from: State::Disabled
        })]
    );
    assert_eq!(brcb.state(), State::Disabled);

    write(3, &mut brcb, Write::EntryId(EntryId(1)));
    assert_eq!(brcb.state(), State::Resync);
    let refused = write(4, &mut brcb, Write::EntryId(EntryId(99)));
    assert_eq!(
        answers(&refused),
        [Err(Refusal::EntryNotInQueue {
            from: State::Resync
        })]
    );
    assert_eq!(brcb.state(), State::Disabled);
}

/// The NULL identifier resynchronises to the oldest entry; the state follows figure 26 (disabled to resync, resync to disabled).
#[test]
fn the_null_entry_identifier_resynchronises_to_the_oldest_entry() {
    let mut brcb = block(8, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    for (at, member) in [(1, 0), (2, 1)] {
        change(at, &mut brcb, member);
    }
    assert_eq!(
        answers(&write(3, &mut brcb, Write::EntryId(EntryId::NULL))),
        [Ok(())]
    );
    assert_eq!(brcb.state(), State::Resync);
    let sent = reports(&write(4, &mut brcb, Write::RptEna(true)));
    assert_eq!(ids(&sent), [1, 2]);
    assert!(sent.iter().all(|report| !report.buf_ovfl));
}

/// The loss of the association keeps the history; the client resumes from its last entry after it reconnects.
#[test]
fn a_lost_association_keeps_the_history_and_the_client_resumes_after_its_entry() {
    let mut brcb = block(8, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    write(1, &mut brcb, Write::RptEna(true));
    assert_eq!(ids(&reports(&change(2, &mut brcb, 0))), [1]);
    assert_eq!(ids(&reports(&change(3, &mut brcb, 1))), [2]);
    assert_eq!(brcb.last_entry_id(), EntryId(2), "the last formatted entry");

    assert!(brcb.handle(ms(4), Event::AssociationLost(ALICE)).is_empty());
    assert_eq!(brcb.state(), State::Disabled);
    assert!(
        reports(&change(5, &mut brcb, 2)).is_empty(),
        "entries are buffered while the association is down"
    );
    assert_eq!(
        brcb.last_entry_id(),
        EntryId(3),
        "the newest entry made while disabled"
    );

    // The reconnected client resumes after entry 2, which it received.
    write(10, &mut brcb, Write::EntryId(EntryId(2)));
    let sent = reports(&write(11, &mut brcb, Write::RptEna(true)));
    assert_eq!(ids(&sent), [3], "nothing is repeated, nothing is lost");
    assert!(sent.iter().all(|report| !report.buf_ovfl));
}

/// A block enabled from disabled without resynchronisation starts with the oldest entry (not the last one sent), and says so in its first
/// report only.
#[test]
fn enabling_from_disabled_starts_with_the_oldest_entry_and_says_so() {
    let mut brcb = block(8, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    change(1, &mut brcb, 0);
    change(2, &mut brcb, 1);
    let sent = reports(&write(3, &mut brcb, Write::RptEna(true)));
    assert_eq!(ids(&sent), [1, 2]);
    assert!(sent[0].buf_ovfl && !sent[1].buf_ovfl);

    // Disabled again, and enabled again without resynchronisation: the entries are sent again from the oldest one.
    write(4, &mut brcb, Write::RptEna(false));
    let again = reports(&write(5, &mut brcb, Write::RptEna(true)));
    assert_eq!(ids(&again), [1, 2]);
    assert!(again[0].buf_ovfl);
    assert_eq!(again[0].sequence, 0);
}

/// The purge discards the history and the open window; it is refused while enabled, and a change of a purging attribute discards the
/// history while disabled.
#[test]
fn purge_discards_the_history_and_is_refused_while_enabled() {
    let mut brcb = block(8, 100, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    change(1, &mut brcb, 0);
    write(2, &mut brcb, Write::RptEna(true));
    assert_eq!(
        answers(&write(3, &mut brcb, Write::PurgeBuf)),
        [Err(Refusal::Enabled)]
    );
    write(4, &mut brcb, Write::RptEna(false));

    let actions = write(5, &mut brcb, Write::PurgeBuf);
    assert_eq!(answers(&actions), [Ok(())]);
    assert_eq!(brcb.history().count(), 0);
    assert!(
        actions.contains(&Action::Disarm(Timer::BufferTime)),
        "the open window is discarded"
    );
    let late = brcb.handle(ms(200), Event::Timer(Timer::BufferTime));
    assert!(late.is_empty());
    assert_eq!(brcb.history().count(), 0);

    // A change of the trigger options purges too (table 70, c1); the entries made after it are numbered on.
    change(210, &mut brcb, 1);
    change(211, &mut brcb, 2);
    write(212, &mut brcb, Write::BufTm(ms(0)));
    assert_eq!(
        brcb.history().count(),
        0,
        "a change of the buffer time purges the history"
    );
    change(213, &mut brcb, 0);
    let configured = write(
        220,
        &mut brcb,
        Write::TrgOps(TriggerOptions {
            data_update: true,
            ..Default::default()
        }),
    );
    assert_eq!(answers(&configured), [Ok(())]);
    assert_eq!(
        brcb.history().count(),
        0,
        "a change of the trigger options purges the history"
    );
    // Only the data update is triggered now; the numbering goes on after the purged entries (entry 1 was purged).
    brcb.handle(
        ms(221),
        Event::Notification {
            member: 0,
            trigger: Trigger::DataUpdate,
        },
    );
    assert_eq!(
        brcb.history().map(|entry| entry.id.0).collect::<Vec<_>>(),
        [2]
    );
}

/// The configuration attributes are refused while enabled, and a change of value while disabled discards the history.
#[test]
fn a_configuration_is_refused_while_enabled_and_a_change_purges_the_history() {
    let mut brcb = block(8, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    change(1, &mut brcb, 0);
    assert_eq!(
        answers(&write(2, &mut brcb, Write::RptId("RPT2".to_string()))),
        [Ok(())]
    );
    assert_eq!(
        brcb.history().count(),
        0,
        "a change of the report identifier purges"
    );
    change(3, &mut brcb, 0);
    write(4, &mut brcb, Write::RptEna(true));
    assert_eq!(
        answers(&write(
            5,
            &mut brcb,
            Write::OptFields(BufferedFields::default())
        )),
        [Err(Refusal::Enabled)]
    );
    assert_eq!(
        answers(&write(6, &mut brcb, Write::BufTm(ms(10)))),
        [Err(Refusal::Enabled)]
    );
}

/// The data set must exist to enable, and an unknown one is refused when it is written.
#[test]
fn the_data_set_must_exist_to_enable_and_to_be_written() {
    let mut brcb = block(8, 0, 0, dchg());
    assert_eq!(
        answers(&write(0, &mut brcb, Write::DataSet(None))),
        [Ok(())]
    );
    assert_eq!(
        answers(&write(0, &mut brcb, Write::RptEna(true))),
        [Err(Refusal::DataSetNull)]
    );
    assert_eq!(
        answers(&write(
            1,
            &mut brcb,
            Write::DataSet(Some("NOPE".to_string()))
        )),
        [Err(Refusal::UnknownDataSet)]
    );
    assert_eq!(
        answers(&write(
            2,
            &mut brcb,
            Write::DataSet(Some("DS1".to_string()))
        )),
        [Ok(())]
    );
    assert_eq!(answers(&write(3, &mut brcb, Write::RptEna(true))), [Ok(())]);
}

/// The integrity period buffers an entry of all members in every state, and restarts on each period; a stale timer is ignored.
#[test]
fn the_integrity_entries_are_buffered_in_every_state_and_the_timer_is_rearmed() {
    let trg = TriggerOptions {
        integrity: true,
        ..Default::default()
    };
    let mut brcb = block(8, 0, 500, trg);
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    let started = brcb.start(ms(0));
    assert_eq!(
        started,
        [Action::Arm {
            timer: Timer::Integrity,
            at: ms(500)
        }]
    );
    assert!(brcb
        .handle(ms(300), Event::Timer(Timer::Integrity))
        .is_empty());

    let due = brcb.handle(ms(500), Event::Timer(Timer::Integrity));
    assert!(due.contains(&Action::Arm {
        timer: Timer::Integrity,
        at: ms(1000)
    }));
    let entries: Vec<_> = brcb.history().cloned().collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, ReportKind::Integrity);
    assert_eq!(entries[0].entries.len(), 3, "all members");
    assert!(entries[0]
        .entries
        .iter()
        .all(|entry| entry.reasons == BTreeSet::from([Reason::Integrity])));

    write(600, &mut brcb, Write::RptEna(true));
    let sent = reports(&brcb.handle(ms(1000), Event::Timer(Timer::Integrity)));
    assert_eq!(
        sent.len(),
        1,
        "enabled: the next integrity entry is sent when it is made"
    );
    assert_eq!(sent[0].kind, ReportKind::Integrity);
}

/// The sequence number starts at zero each time the block is enabled.
#[test]
fn the_sequence_number_restarts_at_each_enabling() {
    let mut brcb = block(8, 0, 0, dchg());
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    write(1, &mut brcb, Write::RptEna(true));
    change(2, &mut brcb, 0);
    let second = reports(&change(3, &mut brcb, 1));
    assert_eq!(second[0].sequence, 1);
    write(4, &mut brcb, Write::RptEna(false));
    write(5, &mut brcb, Write::EntryId(EntryId(1)));
    let again = reports(&write(6, &mut brcb, Write::RptEna(true)));
    assert_eq!(again[0].sequence, 0);
}

/// The last entry identifier: the newest entry while disabled, the last one formatted while enabled; the NULL identifier when there is
/// none.
#[test]
fn the_last_entry_identifier_is_the_newest_or_the_last_formatted() {
    let mut brcb = block(8, 0, 0, dchg());
    assert_eq!(brcb.last_entry_id(), EntryId::NULL);
    write(0, &mut brcb, Write::DataSet(Some("DS1".to_string())));
    change(1, &mut brcb, 0);
    change(2, &mut brcb, 1);
    assert_eq!(brcb.last_entry_id(), EntryId(2));
    write(3, &mut brcb, Write::RptEna(true));
    assert_eq!(brcb.last_entry_id(), EntryId(2));
}
