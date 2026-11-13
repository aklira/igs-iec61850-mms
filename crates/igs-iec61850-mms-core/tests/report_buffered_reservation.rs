// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The general interrogation (figure 30) and the reservation (`ResvTms`, 7-2 §17.2.2.17) of the buffered report control block, with
//! controlled time (task Q4; D-044).

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
    Action, Brcb, BufferedFields, BufferedReport, Config, EntryId, Event, Refusal, Write,
};
use igs_iec61850_mms_core::report::urcb::{Reason, ReportKind, Timer, Trigger, TriggerOptions};

const ALICE: ClientId = ClientId(1);
const BOB: ClientId = ClientId(2);

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

/// A block on the data set DS1 of three members, with the given buffer time and trigger options.
fn block(buf_ms: u64, trg: TriggerOptions) -> Brcb {
    Brcb::new(
        Config {
            rpt_id: "RPT1".to_string(),
            data_set: Some("DS1".to_string()),
            data_sets: BTreeMap::from([("DS1".to_string(), 3)]),
            opt_flds: BufferedFields::default(),
            buf_tm: ms(buf_ms),
            intg_pd: Duration::ZERO,
            trg_ops: trg,
            conf_rev: 1,
        },
        8,
    )
}

fn triggers(general: bool) -> TriggerOptions {
    TriggerOptions {
        data_change: true,
        general_interrogation: general,
        ..Default::default()
    }
}

fn write_by(client: ClientId, at: Duration, brcb: &mut Brcb, value: Write) -> Vec<Action> {
    brcb.handle(
        at,
        Event::Request {
            client,
            write: value,
        },
    )
}

fn change(at: Duration, brcb: &mut Brcb, member: usize) -> Vec<Action> {
    brcb.handle(
        at,
        Event::Notification {
            member,
            trigger: Trigger::DataChange,
        },
    )
}

fn answers(actions: &[Action]) -> Vec<Result<(), Refusal>> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Respond(result) => Some(*result),
            _ => None,
        })
        .collect()
}

fn reports(actions: &[Action]) -> Vec<&BufferedReport> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Report(report) => Some(report),
            _ => None,
        })
        .collect()
}

// ----------------------------------------------------------------------------------------------------------------------------------
// General interrogation (figure 30)
// ----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_general_interrogation_is_refused_while_the_block_is_disabled() {
    // The initial state of figure 30 is NO GI ALLOWED: the request gets a negative response, and nothing is sent.
    let mut brcb = block(0, triggers(true));
    let actions = write_by(ALICE, ms(0), &mut brcb, Write::GeneralInterrogation);
    assert_eq!(
        answers(&actions),
        [Err(Refusal::GeneralInterrogationNotAllowed)]
    );
    assert!(reports(&actions).is_empty());
}

#[test]
fn a_general_interrogation_is_an_entry_of_every_member_with_its_reason() {
    // The reference peer keeps the general-interrogation report as an entry, with its EntryID (bench row Q4, D-044): so does the block.
    let mut brcb = block(0, triggers(true));
    write_by(ALICE, ms(0), &mut brcb, Write::RptEna(true));
    let actions = write_by(ALICE, ms(10), &mut brcb, Write::GeneralInterrogation);
    assert_eq!(answers(&actions), [Ok(())]);
    let sent = reports(&actions);
    assert_eq!(sent.len(), 1);
    let report = sent[0];
    assert_eq!(report.kind, ReportKind::GeneralInterrogation);
    assert_eq!(report.entry_id, EntryId(1));
    let members: Vec<usize> = report.entries.iter().map(|entry| entry.member).collect();
    assert_eq!(members, [0, 1, 2]);
    for entry in &report.entries {
        assert_eq!(
            entry.reasons,
            BTreeSet::from([Reason::GeneralInterrogation])
        );
    }
    assert_eq!(brcb.history().count(), 1);
    assert_eq!(brcb.last_entry_id(), EntryId(1));
}

#[test]
fn a_general_interrogation_sends_the_open_window_first_and_takes_the_next_sequence_number() {
    // A notification in an open buffer-time window is not an entry yet: the interrogation closes the window, so the entry is sent
    // first, and the general-interrogation report comes after it, with the next sequence number (figure 30, 17.2.3.2.3.4).
    let mut brcb = block(1000, triggers(true));
    write_by(ALICE, ms(0), &mut brcb, Write::RptEna(true));
    assert!(reports(&change(ms(0), &mut brcb, 0)).is_empty());
    let actions = write_by(ALICE, ms(100), &mut brcb, Write::GeneralInterrogation);
    let sent = reports(&actions);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].kind, ReportKind::Notification);
    assert_eq!(sent[0].sequence, 0);
    assert_eq!(sent[1].kind, ReportKind::GeneralInterrogation);
    assert_eq!(sent[1].sequence, 1);
    assert_eq!(brcb.sequence(), 2);
}

#[test]
fn a_general_interrogation_is_not_sent_when_its_trigger_is_off() {
    // With TrgOps.general-interrogation FALSE the request is answered and nothing is sent (17.2.3.2.3.4).
    let mut brcb = block(0, triggers(false));
    write_by(ALICE, ms(0), &mut brcb, Write::RptEna(true));
    let actions = write_by(ALICE, ms(10), &mut brcb, Write::GeneralInterrogation);
    assert_eq!(answers(&actions), [Ok(())]);
    assert!(reports(&actions).is_empty());
}

// ----------------------------------------------------------------------------------------------------------------------------------
// Reservation (ResvTms, 7-2 §17.2.2.17)
// ----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_reserved_block_refuses_the_writes_of_other_clients() {
    let mut brcb = block(0, triggers(false));
    assert_eq!(
        answers(&write_by(ALICE, ms(0), &mut brcb, Write::ReserveTime(60))),
        [Ok(())]
    );
    assert_eq!(brcb.reserved_by(), Some(ALICE));
    assert_eq!(brcb.reserve_time(), 60);
    // Another client cannot enable, configure, or reserve the block.
    assert_eq!(
        answers(&write_by(BOB, ms(1), &mut brcb, Write::RptEna(true))),
        [Err(Refusal::Reserved)]
    );
    assert_eq!(
        answers(&write_by(BOB, ms(1), &mut brcb, Write::BufTm(ms(5)))),
        [Err(Refusal::Reserved)]
    );
    assert_eq!(
        answers(&write_by(BOB, ms(1), &mut brcb, Write::ReserveTime(0))),
        [Err(Refusal::Reserved)]
    );
    // The holder writes.
    assert_eq!(
        answers(&write_by(ALICE, ms(2), &mut brcb, Write::RptEna(true))),
        [Ok(())]
    );
    assert!(brcb.is_enabled());
}

#[test]
fn the_holder_releases_the_reservation_with_a_zero_time() {
    let mut brcb = block(0, triggers(false));
    write_by(ALICE, ms(0), &mut brcb, Write::ReserveTime(60));
    assert_eq!(
        answers(&write_by(ALICE, ms(1), &mut brcb, Write::ReserveTime(0))),
        [Ok(())]
    );
    assert_eq!(brcb.reserved_by(), None);
    assert_eq!(brcb.reserve_time(), 0);
    assert_eq!(
        answers(&write_by(BOB, ms(2), &mut brcb, Write::RptEna(true))),
        [Ok(())]
    );
}

#[test]
fn a_zero_time_on_a_free_block_is_a_positive_answer() {
    let mut brcb = block(0, triggers(false));
    assert_eq!(
        answers(&write_by(BOB, ms(0), &mut brcb, Write::ReserveTime(0))),
        [Ok(())]
    );
    assert_eq!(brcb.reserved_by(), None);
}

#[test]
fn the_reservation_of_a_lost_holder_lasts_its_time_and_then_the_block_is_free() {
    // ResvTms seconds after the loss of the association of the holder, the reservation is released by its timer (§17.2.2.17).
    let mut brcb = block(0, triggers(false));
    write_by(ALICE, secs(0), &mut brcb, Write::ReserveTime(10));
    let lost = brcb.handle(secs(2), Event::AssociationLost(ALICE));
    assert!(lost.contains(&Action::Arm {
        timer: Timer::Reservation,
        at: secs(12),
    }));
    // Still reserved for another client, until the timer expires.
    assert_eq!(
        answers(&write_by(BOB, secs(5), &mut brcb, Write::RptEna(true))),
        [Err(Refusal::Reserved)]
    );
    assert_eq!(brcb.reserved_by(), Some(ALICE));
    brcb.handle(secs(12), Event::Timer(Timer::Reservation));
    assert_eq!(brcb.reserved_by(), None);
    assert_eq!(brcb.reserve_time(), 0);
    assert_eq!(
        answers(&write_by(BOB, secs(13), &mut brcb, Write::RptEna(true))),
        [Ok(())]
    );
}

#[test]
fn the_loss_of_a_client_that_is_neither_owner_nor_holder_changes_nothing() {
    let mut brcb = block(0, triggers(false));
    write_by(ALICE, ms(0), &mut brcb, Write::RptEna(true));
    let actions = brcb.handle(ms(5), Event::AssociationLost(BOB));
    assert!(actions.is_empty());
    assert!(brcb.is_enabled());
}

#[test]
fn the_loss_of_the_owner_disables_the_block_and_keeps_its_reservation_rule() {
    // The client that enabled the block owns it: its loss disables the block, while a reservation of another client is untouched.
    let mut brcb = block(0, triggers(false));
    write_by(ALICE, ms(0), &mut brcb, Write::RptEna(true));
    write_by(BOB, ms(1), &mut brcb, Write::ReserveTime(30));
    brcb.handle(ms(2), Event::AssociationLost(ALICE));
    assert!(!brcb.is_enabled());
    assert_eq!(brcb.reserved_by(), Some(BOB));
}
