// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The log and its control block, with controlled time (task Q5; 7-2 §17.3): the bounded buffer, the queries by time and after an
//! entry, the status, and the recording of changes, of transitions of LogEna and of the integrity period.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use igs_iec61850_mms_core::logs::{
    Action, Content, Lcb, LcbConfig, Log, Member, QueryError, Refusal, Write,
};
use igs_iec61850_mms_core::report::brcb::EntryId;
use igs_iec61850_mms_core::report::urcb::{Reason, Timer, Trigger, TriggerOptions};

fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

fn note(name: &str) -> Content {
    Content::Transition {
        lcb: name.to_string(),
        enabled: true,
    }
}

fn config(trg: TriggerOptions, intg: Duration) -> LcbConfig {
    LcbConfig {
        name: "LCB1".to_string(),
        log_ref: Some("LOG1".to_string()),
        data_set: Some("DS1".to_string()),
        data_sets: BTreeMap::from([("DS1".to_string(), 3)]),
        trg_ops: trg,
        intg_pd: intg,
    }
}

fn change_trigger() -> TriggerOptions {
    TriggerOptions {
        data_change: true,
        ..Default::default()
    }
}

fn responses(actions: &[Action]) -> Vec<Result<(), Refusal>> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Respond(result) => Some(*result),
            _ => None,
        })
        .collect()
}

fn records(actions: &[Action]) -> Vec<Content> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Record { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

// ----------------------------------------------------------------------------------------------------------------------------------
// The log
// ----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_log_overwrites_its_oldest_entry_and_the_identifiers_keep_increasing() {
    let mut log = Log::new(2);
    let first = log.append(secs(1), note("A"));
    let second = log.append(secs(2), note("A"));
    let third = log.append(secs(3), note("A"));
    assert_eq!((first, second, third), (EntryId(1), EntryId(2), EntryId(3)));
    assert_eq!(log.len(), 2);
    let held: Vec<EntryId> = log.entries().map(|entry| entry.id).collect();
    assert_eq!(
        held,
        [EntryId(2), EntryId(3)],
        "the oldest entry is overwritten"
    );
}

#[test]
fn a_query_by_time_includes_both_ends_and_open_ends() {
    let mut log = Log::new(10);
    for time in [1, 2, 3] {
        log.append(secs(time), note("A"));
    }
    let between: Vec<Duration> = log
        .query_by_time(Some(secs(2)), Some(secs(3)))
        .iter()
        .map(|entry| entry.time_of_entry)
        .collect();
    assert_eq!(between, [secs(2), secs(3)]);
    assert_eq!(log.query_by_time(None, None).len(), 3);
    assert_eq!(log.query_by_time(None, Some(secs(1))).len(), 1);
    assert!(log.query_by_time(Some(secs(4)), None).is_empty());
}

#[test]
fn a_query_after_an_entry_returns_the_entries_that_follow_it() {
    let mut log = Log::new(10);
    let first = log.append(secs(1), note("A"));
    let second = log.append(secs(2), note("A"));
    let third = log.append(secs(3), note("A"));
    let after = log.query_after(secs(2), second).expect("the entry is held");
    let ids: Vec<EntryId> = after.iter().map(|entry| entry.id).collect();
    assert_eq!(ids, [third], "the entry itself is not returned");
    assert!(log
        .query_after(secs(3), third)
        .expect("the newest entry is held")
        .is_empty());
    // The entry must be held with the time given with it.
    assert_eq!(
        log.query_after(secs(2), first).map(|_| ()),
        Err(QueryError::UnknownEntry)
    );
    assert_eq!(
        log.query_after(secs(9), EntryId(99)).map(|_| ()),
        Err(QueryError::UnknownEntry)
    );
}

#[test]
fn the_status_of_an_empty_log_is_zero_and_the_status_of_a_filled_one_gives_its_ends() {
    let mut log = Log::new(10);
    let empty = log.status();
    assert_eq!(empty.oldest_time, Duration::ZERO);
    assert_eq!(empty.oldest, EntryId::NULL);
    assert_eq!(empty.newest, EntryId::NULL);
    log.append(secs(5), note("A"));
    let newest = log.append(secs(9), note("A"));
    let status = log.status();
    assert_eq!(status.oldest_time, secs(5));
    assert_eq!(status.newest_time, secs(9));
    assert_eq!(status.oldest, EntryId(1));
    assert_eq!(status.newest, newest);
}

// ----------------------------------------------------------------------------------------------------------------------------------
// The control block
// ----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_change_is_recorded_only_while_enabled_and_with_its_reason() {
    let mut lcb = Lcb::new(config(change_trigger(), Duration::ZERO));
    // Disabled: the change is not recorded.
    let before = lcb.handle(
        secs(1),
        igs_iec61850_mms_core::logs::Event::Notification {
            member: 0,
            trigger: Trigger::DataChange,
        },
    );
    assert!(records(&before).is_empty());
    lcb.handle(
        secs(2),
        igs_iec61850_mms_core::logs::Event::Request(Write::LogEna(true)),
    );
    let during = lcb.handle(
        secs(3),
        igs_iec61850_mms_core::logs::Event::Notification {
            member: 1,
            trigger: Trigger::DataChange,
        },
    );
    assert_eq!(
        records(&during),
        [Content::Members(vec![Member {
            member: 1,
            reasons: BTreeSet::from([Reason::DataChange]),
        }])]
    );
    // A quality change is not monitored by these options.
    let unmonitored = lcb.handle(
        secs(4),
        igs_iec61850_mms_core::logs::Event::Notification {
            member: 1,
            trigger: Trigger::QualityChange,
        },
    );
    assert!(records(&unmonitored).is_empty());
}

#[test]
fn each_transition_of_log_ena_is_recorded() {
    use igs_iec61850_mms_core::logs::Event;
    let mut lcb = Lcb::new(config(change_trigger(), Duration::ZERO));
    let on = lcb.handle(secs(1), Event::Request(Write::LogEna(true)));
    assert_eq!(responses(&on), [Ok(())]);
    assert_eq!(
        records(&on),
        [Content::Transition {
            lcb: "LCB1".to_string(),
            enabled: true,
        }]
    );
    let off = lcb.handle(secs(5), Event::Request(Write::LogEna(false)));
    assert_eq!(
        records(&off),
        [Content::Transition {
            lcb: "LCB1".to_string(),
            enabled: false,
        }]
    );
    assert!(!lcb.is_enabled());
    // Writing the same value again changes nothing.
    assert!(records(&lcb.handle(secs(6), Event::Request(Write::LogEna(false)))).is_empty());
}

#[test]
fn the_configuration_is_refused_while_enabled() {
    use igs_iec61850_mms_core::logs::Event;
    let mut lcb = Lcb::new(config(change_trigger(), Duration::ZERO));
    lcb.handle(secs(0), Event::Request(Write::LogEna(true)));
    assert_eq!(
        responses(&lcb.handle(secs(1), Event::Request(Write::DataSet(None)))),
        [Err(Refusal::Enabled)]
    );
    lcb.handle(secs(2), Event::Request(Write::LogEna(false)));
    assert_eq!(
        responses(&lcb.handle(secs(3), Event::Request(Write::DataSet(None)))),
        [Ok(())]
    );
}

#[test]
fn the_general_interrogation_is_not_a_trigger_of_logging() {
    use igs_iec61850_mms_core::logs::Event;
    let mut lcb = Lcb::new(config(change_trigger(), Duration::ZERO));
    let refused = TriggerOptions {
        general_interrogation: true,
        ..change_trigger()
    };
    assert_eq!(
        responses(&lcb.handle(secs(0), Event::Request(Write::TrgOps(refused)))),
        [Err(Refusal::GeneralInterrogation)]
    );
}

#[test]
fn enabling_needs_a_log_and_a_data_set_that_exists() {
    use igs_iec61850_mms_core::logs::Event;
    let mut without_log = config(change_trigger(), Duration::ZERO);
    without_log.log_ref = None;
    let mut lcb = Lcb::new(without_log);
    assert_eq!(
        responses(&lcb.handle(secs(0), Event::Request(Write::LogEna(true)))),
        [Err(Refusal::NotConfigured)]
    );
    assert_eq!(
        responses(&lcb.handle(
            secs(0),
            Event::Request(Write::LogRef(Some("LOG1".to_string())))
        )),
        [Ok(())]
    );
    assert_eq!(
        responses(&lcb.handle(
            secs(0),
            Event::Request(Write::DataSet(Some("NOPE".to_string())))
        )),
        [Err(Refusal::UnknownDataSet)]
    );
    assert_eq!(
        responses(&lcb.handle(secs(0), Event::Request(Write::DataSet(None)))),
        [Ok(())]
    );
    assert_eq!(
        responses(&lcb.handle(secs(0), Event::Request(Write::LogEna(true)))),
        [Err(Refusal::NotConfigured)],
        "no data set is set"
    );
}

#[test]
fn the_integrity_period_records_every_member_and_then_arms_the_next_period() {
    use igs_iec61850_mms_core::logs::Event;
    let trg = TriggerOptions {
        integrity: true,
        ..Default::default()
    };
    let mut lcb = Lcb::new(config(trg, secs(1)));
    let started = lcb.handle(secs(0), Event::Request(Write::LogEna(true)));
    assert!(started.contains(&Action::Arm {
        timer: Timer::Integrity,
        at: secs(1),
    }));
    let due = lcb.handle(secs(1), Event::Timer(Timer::Integrity));
    let every = BTreeSet::from([Reason::Integrity]);
    assert_eq!(
        records(&due),
        [Content::Members(
            (0..3)
                .map(|member| Member {
                    member,
                    reasons: every.clone(),
                })
                .collect()
        )]
    );
    assert!(due.contains(&Action::Arm {
        timer: Timer::Integrity,
        at: secs(2),
    }));
}
