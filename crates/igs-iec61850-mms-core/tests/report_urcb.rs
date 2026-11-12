// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The unbuffered report control block with controlled time (task D3; RPT-001): reservation, configuration writes, enabling,
//! each trigger, the buffer time, the integrity period, the general interrogation, and the loss of an association.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use igs_iec61850_mms_core::control::ClientId;
use igs_iec61850_mms_core::report::urcb::{
    Action, Config, Entry, Event, Reason, Refusal, Report, ReportKind, Timer, Trigger,
    TriggerOptions, Urcb, Write,
};

const A: ClientId = ClientId(1);
const B: ClientId = ClientId(2);

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// A block on the data set DS1 of three members, with the given buffer time, integrity period and trigger options.
fn block(buf_ms: u64, intg_ms: u64, trg: TriggerOptions) -> Urcb {
    Urcb::new(Config {
        rpt_id: "RPT1".to_string(),
        data_set: Some("DS1".to_string()),
        data_sets: BTreeMap::from([("DS1".to_string(), 3)]),
        opt_flds: Default::default(),
        buf_tm: ms(buf_ms),
        intg_pd: ms(intg_ms),
        trg_ops: trg,
        conf_rev: 1,
    })
}

fn dchg() -> TriggerOptions {
    TriggerOptions {
        data_change: true,
        ..Default::default()
    }
}

/// The client reserves and enables the block at time zero.
fn enabled(urcb: &mut Urcb) {
    assert!(answers(urcb.handle(ms(0), request(A, Write::Reserve(true)))) == [Ok(())]);
    assert!(answers(urcb.handle(ms(0), request(A, Write::RptEna(true)))) == [Ok(())]);
}

fn request(client: ClientId, write: Write) -> Event {
    Event::Request { client, write }
}

fn notify(member: usize, trigger: Trigger) -> Event {
    Event::Notification { member, trigger }
}

/// The answers of the actions, in order.
fn answers(actions: Vec<Action>) -> Vec<Result<(), Refusal>> {
    actions
        .into_iter()
        .filter_map(|action| match action {
            Action::Respond(outcome) => Some(outcome),
            _ => None,
        })
        .collect()
}

fn reports(actions: &[Action]) -> Vec<&Report> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Report(report) => Some(report),
            _ => None,
        })
        .collect()
}

fn entry(member: usize, reasons: &[Reason]) -> Entry {
    Entry {
        member,
        reasons: reasons.iter().copied().collect::<BTreeSet<_>>(),
    }
}

#[test]
fn a_block_is_reserved_by_one_client_at_a_time() {
    let mut urcb = block(0, 0, dchg());
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::Reserve(true)))),
        [Ok(())]
    );
    assert_eq!(
        answers(urcb.handle(ms(0), request(B, Write::Reserve(true)))),
        [Err(Refusal::Reserved)]
    );
    assert_eq!(
        answers(urcb.handle(ms(0), request(B, Write::Reserve(false)))),
        [Err(Refusal::NotReserved)]
    );
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::Reserve(false)))),
        [Ok(())]
    );
    assert_eq!(
        answers(urcb.handle(ms(0), request(B, Write::Reserve(true)))),
        [Ok(())]
    );
    assert_eq!(urcb.reserved_by(), Some(B));
}

#[test]
fn enabling_needs_the_reservation_and_a_data_set() {
    let mut urcb = block(0, 0, dchg());
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::RptEna(true)))),
        [Err(Refusal::NotReserved)]
    );
    urcb.handle(ms(0), request(A, Write::Reserve(true)));
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::DataSet(None)))),
        [Ok(())]
    );
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::RptEna(true)))),
        [Err(Refusal::DataSetNull)]
    );
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::DataSet(Some("NOPE".to_string()))))),
        [Err(Refusal::UnknownDataSet)]
    );
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::DataSet(Some("DS1".to_string()))))),
        [Ok(())]
    );
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::RptEna(true)))),
        [Ok(())]
    );
    assert!(urcb.is_enabled());
}

#[test]
fn configuration_is_written_only_by_the_holder_and_only_while_disabled() {
    let mut urcb = block(0, 0, dchg());
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::RptId("X".into())))),
        [Err(Refusal::NotReserved)]
    );
    enabled(&mut urcb);
    assert_eq!(
        answers(urcb.handle(ms(1), request(B, Write::BufTm(ms(5))))),
        [Err(Refusal::Reserved)]
    );
    assert_eq!(
        answers(urcb.handle(ms(1), request(A, Write::BufTm(ms(5))))),
        [Err(Refusal::Enabled)]
    );
    assert_eq!(
        answers(urcb.handle(ms(1), request(A, Write::Reserve(false)))),
        [Err(Refusal::Enabled)]
    );
    assert_eq!(
        answers(urcb.handle(ms(1), request(A, Write::RptEna(false)))),
        [Ok(())]
    );
    assert_eq!(
        answers(urcb.handle(ms(1), request(A, Write::BufTm(ms(5))))),
        [Ok(())]
    );
    assert_eq!(urcb.config().buf_tm, ms(5));
}

#[test]
fn the_sequence_number_restarts_at_each_enabling_and_wraps_at_256() {
    let mut urcb = block(0, 0, dchg());
    enabled(&mut urcb);
    for expected in 0..3u8 {
        let actions = urcb.handle(ms(0), notify(0, Trigger::DataChange));
        assert_eq!(reports(&actions)[0].sequence, expected);
    }
    urcb.handle(ms(1), request(A, Write::RptEna(false)));
    urcb.handle(ms(2), request(A, Write::RptEna(true)));
    let actions = urcb.handle(ms(2), notify(0, Trigger::DataChange));
    assert_eq!(
        reports(&actions)[0].sequence,
        0,
        "a new enabling restarts SqNum"
    );
    for _ in 0..255 {
        urcb.handle(ms(3), notify(0, Trigger::DataChange));
    }
    let actions = urcb.handle(ms(3), notify(0, Trigger::DataChange));
    assert_eq!(
        reports(&actions)[0].sequence,
        0,
        "SqNum is INT8U: it wraps after 255"
    );
}

#[test]
fn a_change_without_buffer_time_is_reported_at_once_with_its_reason() {
    let mut urcb = block(0, 0, dchg());
    enabled(&mut urcb);
    let actions = urcb.handle(ms(10), notify(1, Trigger::DataChange));
    let report = reports(&actions)[0];
    assert_eq!(report.kind, ReportKind::Notification);
    assert_eq!(report.at, ms(10));
    assert_eq!(report.entries, [entry(1, &[Reason::DataChange])]);
}

#[test]
fn a_trigger_that_is_not_enabled_or_a_disabled_block_reports_nothing() {
    let mut urcb = block(0, 0, dchg());
    assert!(
        urcb.handle(ms(0), notify(0, Trigger::DataChange))
            .is_empty(),
        "disabled"
    );
    enabled(&mut urcb);
    assert!(
        urcb.handle(ms(0), notify(0, Trigger::QualityChange))
            .is_empty(),
        "qchg is off"
    );
    assert!(
        urcb.handle(ms(0), notify(0, Trigger::DataUpdate))
            .is_empty(),
        "dupd is off"
    );
    assert!(
        urcb.handle(ms(0), notify(9, Trigger::DataChange))
            .is_empty(),
        "no such member"
    );
}

#[test]
fn the_buffer_time_combines_the_notifications_of_its_interval_into_one_report() {
    let trg = TriggerOptions {
        data_change: true,
        quality_change: true,
        ..Default::default()
    };
    let mut urcb = block(300, 0, trg);
    enabled(&mut urcb);
    let first = urcb.handle(ms(0), notify(0, Trigger::DataChange));
    assert_eq!(
        first,
        [Action::Arm {
            timer: Timer::BufferTime,
            at: ms(300)
        }]
    );
    assert!(urcb
        .handle(ms(100), notify(2, Trigger::QualityChange))
        .is_empty());
    let timer = urcb.handle(ms(300), Event::Timer(Timer::BufferTime));
    let report = reports(&timer)[0];
    assert_eq!(report.at, ms(300));
    assert_eq!(
        report.entries,
        [
            entry(0, &[Reason::DataChange]),
            entry(2, &[Reason::QualityChange])
        ]
    );
    // The next notification after the expiry starts a new interval.
    let next = urcb.handle(ms(350), notify(1, Trigger::DataChange));
    assert_eq!(
        next,
        [Action::Arm {
            timer: Timer::BufferTime,
            at: ms(650)
        }]
    );
}

#[test]
fn a_second_notification_of_the_same_member_sends_the_pending_report_at_once() {
    let mut urcb = block(300, 0, dchg());
    enabled(&mut urcb);
    urcb.handle(ms(0), notify(0, Trigger::DataChange));
    let actions = urcb.handle(ms(100), notify(0, Trigger::DataChange));
    let report = reports(&actions)[0];
    assert_eq!(report.at, ms(100));
    assert_eq!(report.entries, [entry(0, &[Reason::DataChange])]);
    assert!(actions.contains(&Action::Arm {
        timer: Timer::BufferTime,
        at: ms(400)
    }));
}

#[test]
fn the_integrity_period_reports_every_member_and_repeats() {
    let trg = TriggerOptions {
        integrity: true,
        ..Default::default()
    };
    let mut urcb = block(0, 1000, trg);
    let armed = answers_and_arms(&mut urcb, ms(0));
    assert_eq!(
        armed,
        [Action::Arm {
            timer: Timer::Integrity,
            at: ms(1000)
        }]
    );
    let fired = urcb.handle(ms(1000), Event::Timer(Timer::Integrity));
    let report = reports(&fired)[0];
    assert_eq!(report.kind, ReportKind::Integrity);
    assert_eq!(
        report.entries,
        [
            entry(0, &[Reason::Integrity]),
            entry(1, &[Reason::Integrity]),
            entry(2, &[Reason::Integrity]),
        ]
    );
    assert!(fired.contains(&Action::Arm {
        timer: Timer::Integrity,
        at: ms(2000)
    }));
}

/// The block is enabled at time zero with integrity on: the actions of the enabling.
fn answers_and_arms(urcb: &mut Urcb, _at: Duration) -> Vec<Action> {
    urcb.handle(ms(0), request(A, Write::Reserve(true)));
    urcb.handle(ms(0), request(A, Write::RptEna(true)))
        .into_iter()
        .filter(|action| !matches!(action, Action::Respond(_)))
        .collect()
}

#[test]
fn the_buffered_notifications_are_sent_before_an_integrity_report() {
    let trg = TriggerOptions {
        data_change: true,
        integrity: true,
        ..Default::default()
    };
    let mut urcb = block(800, 1000, trg);
    answers_and_arms(&mut urcb, ms(0));
    urcb.handle(ms(500), notify(0, Trigger::DataChange));
    let fired = urcb.handle(ms(1000), Event::Timer(Timer::Integrity));
    let all = reports(&fired);
    assert_eq!(all.len(), 2, "{fired:?}");
    assert_eq!(all[0].kind, ReportKind::Notification);
    assert_eq!(all[0].entries, [entry(0, &[Reason::DataChange])]);
    assert_eq!(all[1].kind, ReportKind::Integrity);
    assert!(fired.contains(&Action::Disarm(Timer::BufferTime)));
    assert_eq!(
        all[1].sequence,
        all[0].sequence.wrapping_add(1),
        "the reports are in order"
    );
}

#[test]
fn a_general_interrogation_reports_every_member_when_enabled_and_is_refused_when_disabled() {
    let trg = TriggerOptions {
        general_interrogation: true,
        ..Default::default()
    };
    let mut urcb = block(0, 0, trg);
    assert_eq!(
        answers(urcb.handle(ms(0), request(A, Write::GeneralInterrogation))),
        [Err(Refusal::GeneralInterrogationNotAllowed)]
    );
    enabled(&mut urcb);
    let actions = urcb.handle(ms(5), request(A, Write::GeneralInterrogation));
    let report = reports(&actions)[0];
    assert_eq!(report.kind, ReportKind::GeneralInterrogation);
    assert_eq!(
        report.entries,
        [
            entry(0, &[Reason::GeneralInterrogation]),
            entry(1, &[Reason::GeneralInterrogation]),
            entry(2, &[Reason::GeneralInterrogation]),
        ]
    );
    assert_eq!(answers(actions), [Ok(())]);
}

#[test]
fn a_general_interrogation_without_its_trigger_answers_without_a_report() {
    let mut urcb = block(0, 0, dchg());
    enabled(&mut urcb);
    let actions = urcb.handle(ms(5), request(A, Write::GeneralInterrogation));
    assert!(reports(&actions).is_empty());
    assert_eq!(answers(actions), [Ok(())]);
}

#[test]
fn disabling_stops_the_timers_and_drops_the_pending_notifications() {
    let mut urcb = block(300, 0, dchg());
    enabled(&mut urcb);
    urcb.handle(ms(0), notify(0, Trigger::DataChange));
    let actions = urcb.handle(ms(50), request(A, Write::RptEna(false)));
    assert!(actions.contains(&Action::Disarm(Timer::BufferTime)));
    assert!(
        reports(&actions).is_empty(),
        "the pending notification is dropped"
    );
    assert!(reports(&urcb.handle(ms(300), Event::Timer(Timer::BufferTime))).is_empty());
}

#[test]
fn the_loss_of_the_holder_disables_the_block_and_releases_it() {
    let mut urcb = block(0, 0, dchg());
    enabled(&mut urcb);
    urcb.handle(ms(10), Event::AssociationLost(B));
    assert!(
        urcb.is_enabled(),
        "another client's association does not matter"
    );
    urcb.handle(ms(20), Event::AssociationLost(A));
    assert!(!urcb.is_enabled());
    assert_eq!(urcb.reserved_by(), None);
    assert!(urcb
        .handle(ms(30), notify(0, Trigger::DataChange))
        .is_empty());
    assert_eq!(
        answers(urcb.handle(ms(30), request(B, Write::Reserve(true)))),
        [Ok(())]
    );
}

#[test]
fn a_block_reserved_by_another_client_cannot_be_disabled_by_a_third() {
    let mut urcb = block(0, 0, dchg());
    enabled(&mut urcb);
    assert_eq!(
        answers(urcb.handle(ms(1), request(B, Write::RptEna(false)))),
        [Err(Refusal::Reserved)]
    );
    assert!(urcb.is_enabled());
}

#[test]
fn the_entries_of_a_report_are_in_the_order_of_the_members() {
    let trg = TriggerOptions {
        data_change: true,
        ..Default::default()
    };
    let mut urcb = block(200, 0, trg);
    enabled(&mut urcb);
    urcb.handle(ms(0), notify(2, Trigger::DataChange));
    urcb.handle(ms(10), notify(0, Trigger::DataChange));
    let report = reports(&urcb.handle(ms(200), Event::Timer(Timer::BufferTime)))[0].clone();
    let members: Vec<usize> = report.entries.iter().map(|e| e.member).collect();
    assert_eq!(members, [0, 2]);
}
