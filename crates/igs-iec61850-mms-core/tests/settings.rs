// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The setting group control block (task O3; IEC 61850-7-2 §16.3), with controlled time: the selection of the active group, the
//! reservation of the editing, the writes of the edited group, its confirmation, the timeout and the loss of an association.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms_core::control::ClientId;
use igs_iec61850_mms_core::settings::{Confirmation, Refusal, Sgcb, RESERVATION};

const ALICE: ClientId = ClientId(1);
const BOB: ClientId = ClientId(2);

fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

#[test]
fn the_block_starts_with_its_groups_and_no_edited_group() {
    let sgcb = Sgcb::new(3, 2);
    assert_eq!(sgcb.num_of_sgs(), 3);
    assert_eq!(sgcb.act_sg(), 2);
    assert_eq!(sgcb.edit_sg(), 0, "EditSG is zero at power-up (7-2 16.2)");
    assert!(!sgcb.cnf_edit());
    assert_eq!(sgcb.resv_tms(), Duration::ZERO);
}

#[test]
fn the_active_group_is_selected_within_the_groups_and_dates_the_selection() {
    let mut sgcb = Sgcb::new(3, 1);
    assert_eq!(sgcb.select_active(secs(5), 3), Ok(()));
    assert_eq!((sgcb.act_sg(), sgcb.lact_tm()), (3, secs(5)));
    assert_eq!(sgcb.select_active(secs(6), 0), Err(Refusal::OutOfRange));
    assert_eq!(sgcb.select_active(secs(6), 4), Err(Refusal::OutOfRange));
    assert_eq!(sgcb.act_sg(), 3, "a refused selection changes nothing");
}

#[test]
fn the_editing_is_reserved_for_one_client_and_zero_cancels_it() {
    let mut sgcb = Sgcb::new(3, 1);
    assert_eq!(sgcb.select_edit(secs(0), ALICE, 2), Ok(()));
    assert_eq!((sgcb.edit_sg(), sgcb.holder()), (2, Some(ALICE)));
    assert_eq!(sgcb.resv_tms(), RESERVATION);
    assert_eq!(sgcb.select_edit(secs(1), BOB, 3), Err(Refusal::Reserved));
    assert_eq!(
        sgcb.select_edit(secs(2), ALICE, 0),
        Ok(()),
        "zero cancels the editing"
    );
    assert_eq!((sgcb.edit_sg(), sgcb.holder()), (0, None));
    assert_eq!(
        sgcb.select_edit(secs(3), ALICE, 4),
        Err(Refusal::OutOfRange)
    );
}

#[test]
fn only_the_holder_sets_values_of_a_selected_group() {
    let mut sgcb = Sgcb::new(2, 1);
    assert_eq!(
        sgcb.may_set_edit(secs(0), ALICE),
        Err(Refusal::NothingToConfirm)
    );
    sgcb.select_edit(secs(0), ALICE, 2).unwrap();
    assert_eq!(sgcb.may_set_edit(secs(1), ALICE), Ok(()));
    assert_eq!(sgcb.may_set_edit(secs(1), BOB), Err(Refusal::NotReserved));
}

#[test]
fn a_confirmation_overwrites_the_group_releases_the_editing_and_reactivates_the_active_group() {
    let mut sgcb = Sgcb::new(2, 1);
    sgcb.select_edit(secs(0), ALICE, 1).unwrap();
    assert_eq!(
        sgcb.confirm(secs(10), ALICE),
        Ok(Confirmation {
            group: 1,
            active: true
        })
    );
    assert_eq!(
        sgcb.lact_tm(),
        secs(10),
        "the active group is re-activated at the confirmation"
    );
    assert!(sgcb.cnf_edit());
    assert_eq!((sgcb.edit_sg(), sgcb.holder()), (0, None));
    sgcb.storage_done();
    assert!(!sgcb.cnf_edit());
}

#[test]
fn a_confirmation_of_another_group_does_not_change_the_active_selection_time() {
    let mut sgcb = Sgcb::new(2, 1);
    sgcb.select_edit(secs(0), ALICE, 2).unwrap();
    assert_eq!(
        sgcb.confirm(secs(10), ALICE),
        Ok(Confirmation {
            group: 2,
            active: false
        })
    );
    assert_eq!(sgcb.lact_tm(), Duration::ZERO);
}

#[test]
fn a_confirmation_needs_the_editing_and_a_selected_group() {
    let mut sgcb = Sgcb::new(2, 1);
    assert_eq!(sgcb.confirm(secs(0), ALICE), Err(Refusal::NotReserved));
    sgcb.select_edit(secs(0), ALICE, 0).unwrap();
    sgcb.select_edit(secs(1), ALICE, 2).unwrap();
    assert_eq!(sgcb.confirm(secs(2), BOB), Err(Refusal::NotReserved));
}

#[test]
fn the_reservation_expires_when_no_confirmation_comes_in_time() {
    let mut sgcb = Sgcb::new(2, 1);
    sgcb.select_edit(secs(0), ALICE, 2).unwrap();
    assert_eq!(
        sgcb.select_edit(RESERVATION - secs(1), BOB, 1),
        Err(Refusal::Reserved)
    );
    // A late answer is refused, and the editing is free once the time is over.
    assert_eq!(sgcb.confirm(RESERVATION, ALICE), Err(Refusal::NotReserved));
    assert_eq!(sgcb.holder(), None);
    assert_eq!(sgcb.select_edit(RESERVATION, BOB, 1), Ok(()));
}

#[test]
fn the_loss_of_the_holder_releases_the_editing_and_only_the_holder() {
    let mut sgcb = Sgcb::new(2, 1);
    sgcb.select_edit(secs(0), ALICE, 2).unwrap();
    sgcb.lost(BOB);
    assert_eq!(
        sgcb.holder(),
        Some(ALICE),
        "another client's loss changes nothing"
    );
    sgcb.lost(ALICE);
    assert_eq!((sgcb.holder(), sgcb.edit_sg()), (None, 0));
}
