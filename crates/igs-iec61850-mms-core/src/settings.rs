// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The setting group control block (SGCB) of a logical device, as a sans-I/O state machine (task O3; IEC 61850-7-2 §16).
//!
//! The SGCB holds the number of setting groups, the active one (`ActSG`), the one that is edited (`EditSG`), the confirmation of
//! the editing (`CnfEdit`), the time of the last active selection (`LActTm`), and the reservation of the editing (`ResvTms`). The
//! services of §16.3 are specialisations of the MMS reads and writes of these attributes (8-1 §16.2): SelectActiveSG writes `ActSG`,
//! SelectEditSG writes `EditSG`, ConfirmEditSGValues writes `CnfEdit`, and SetEditSGValue writes the attributes with FC `SE`. This
//! module holds the state and the rules; the values of the groups belong to the served model.
//!
//! Decisions: the reservation of the editing lasts `RESERVATION` seconds after the last SelectEditSG (the value of `ResvTms` is not
//! configured by the SCL file in this release); a confirmation releases the editing and sets `EditSG` back to zero; the loss of the
//! association of the holder releases the editing. See `PROVENANCE.md` (D-050).

use std::time::Duration;

use crate::control::ClientId;

/// The reservation of the editing, in seconds, after the last SelectEditSG (`ResvTms` while it is held).
pub const RESERVATION: Duration = Duration::from_secs(60);

/// Why a service of the SGCB is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The setting group number is outside the groups of the device (1 to `NumOfSG`; 0 is allowed only for `EditSG`).
    OutOfRange,
    /// The editing is reserved by another client (§16.3.3).
    Reserved,
    /// The client does not hold the editing reservation.
    NotReserved,
    /// There is no edited group to confirm: `EditSG` is zero.
    NothingToConfirm,
}

/// The result of a confirmation: the group that was edited, and whether it is the active one, which is then re-activated (§16.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Confirmation {
    /// The group whose values are confirmed.
    pub group: u8,
    /// Whether that group is the active one.
    pub active: bool,
}

/// The setting group control block of a logical device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sgcb {
    num_of_sgs: u8,
    act_sg: u8,
    edit_sg: u8,
    cnf_edit: bool,
    lact_tm: Duration,
    holder: Option<ClientId>,
    reserved_since: Duration,
}

impl Sgcb {
    /// A control block with `num_of_sgs` groups (at least one), and the group `act_sg` active (clamped into the groups). `EditSG` is zero
    /// at power-up (§16.2).
    pub fn new(num_of_sgs: u8, act_sg: u8) -> Self {
        let num_of_sgs = num_of_sgs.max(1);
        Self {
            num_of_sgs,
            act_sg: act_sg.clamp(1, num_of_sgs),
            edit_sg: 0,
            cnf_edit: false,
            lact_tm: Duration::ZERO,
            holder: None,
            reserved_since: Duration::ZERO,
        }
    }

    /// `NumOfSG`: the number of setting groups.
    pub fn num_of_sgs(&self) -> u8 {
        self.num_of_sgs
    }

    /// `ActSG`: the active setting group.
    pub fn act_sg(&self) -> u8 {
        self.act_sg
    }

    /// `EditSG`: the group being edited, or zero.
    pub fn edit_sg(&self) -> u8 {
        self.edit_sg
    }

    /// `CnfEdit`: whether the editing is confirmed and being stored.
    pub fn cnf_edit(&self) -> bool {
        self.cnf_edit
    }

    /// `LActTm`: the time of the last successful SelectActiveSG, or of a confirmation of the active group.
    pub fn lact_tm(&self) -> Duration {
        self.lact_tm
    }

    /// The client that holds the editing reservation, if any.
    pub fn holder(&self) -> Option<ClientId> {
        self.holder
    }

    /// `ResvTms`: the reservation time while the editing is held, zero when it is free (§16.2).
    pub fn resv_tms(&self) -> Duration {
        if self.holder.is_some() {
            RESERVATION
        } else {
            Duration::ZERO
        }
    }

    /// SelectActiveSG (§16.3.2): the group `group` (1 to `NumOfSG`) becomes the active one at time `now`.
    pub fn select_active(&mut self, now: Duration, group: u8) -> Result<(), Refusal> {
        if !(1..=self.num_of_sgs).contains(&group) {
            return Err(Refusal::OutOfRange);
        }
        self.act_sg = group;
        self.lact_tm = now;
        Ok(())
    }

    /// SelectEditSG (§16.3.3): `group` (0 to `NumOfSG`) is the group to edit. A non-zero group reserves the editing for the client, when
    /// it is free or held by this client; zero cancels the editing of this client.
    pub fn select_edit(
        &mut self,
        now: Duration,
        client: ClientId,
        group: u8,
    ) -> Result<(), Refusal> {
        if group > self.num_of_sgs {
            return Err(Refusal::OutOfRange);
        }
        self.expire(now);
        if self.holder.is_some_and(|holder| holder != client) {
            return Err(Refusal::Reserved);
        }
        if group == 0 {
            self.release();
            return Ok(());
        }
        self.holder = Some(client);
        self.reserved_since = now;
        self.edit_sg = group;
        self.cnf_edit = false;
        Ok(())
    }

    /// Whether this client may set a value of the group being edited (SetEditSGValue, §16.3.4): it holds the editing, and a group is
    /// selected for it.
    pub fn may_set_edit(&mut self, now: Duration, client: ClientId) -> Result<(), Refusal> {
        self.expire(now);
        if self.edit_sg == 0 {
            return Err(Refusal::NothingToConfirm);
        }
        if self.holder != Some(client) {
            return Err(Refusal::NotReserved);
        }
        Ok(())
    }

    /// ConfirmEditSGValues (§16.3.5): the client that holds the editing confirms the values of the edited group, which then overwrite
    /// that group. The editing is released; `EditSG` returns to zero, and `LActTm` is set when the active group is confirmed.
    pub fn confirm(&mut self, now: Duration, client: ClientId) -> Result<Confirmation, Refusal> {
        self.expire(now);
        if self.holder != Some(client) {
            return Err(Refusal::NotReserved);
        }
        if self.edit_sg == 0 {
            return Err(Refusal::NothingToConfirm);
        }
        let group = self.edit_sg;
        let active = group == self.act_sg;
        if active {
            self.lact_tm = now;
        }
        self.release();
        self.cnf_edit = true;
        Ok(Confirmation { group, active })
    }

    /// The storage of a confirmed group is complete: `CnfEdit` returns to FALSE (§16.3.5). The moment of the storage is a local issue.
    pub fn storage_done(&mut self) {
        self.cnf_edit = false;
    }

    /// The association of a client is lost: its editing reservation is released (§16.3.3).
    pub fn lost(&mut self, client: ClientId) {
        if self.holder == Some(client) {
            self.release();
        }
    }

    /// The reservation expires when no confirmation came within `RESERVATION` of the last SelectEditSG.
    pub fn expire(&mut self, now: Duration) {
        if self.holder.is_some() && now.saturating_sub(self.reserved_since) >= RESERVATION {
            self.release();
        }
    }

    fn release(&mut self) {
        self.holder = None;
        self.edit_sg = 0;
        self.cnf_edit = false;
    }
}
