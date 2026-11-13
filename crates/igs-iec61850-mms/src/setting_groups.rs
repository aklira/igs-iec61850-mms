// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The setting group control block (SGCB) of a served device (task O3; IEC 61850-7-2 §16; IEC 61850-8-1 §16).
//!
//! The SGCB is the named component `LLN0$SP$SGCB` of its device, with the attributes of 8-1 table 71. Its services are reads and
//! writes of those attributes (8-1 §16.2): SelectActiveSG writes `ActSG`, SelectEditSG writes `EditSG`, ConfirmEditSGValues writes
//! `CnfEdit`, and SetEditSGValue writes the setting values with FC `SE`. The rules are in `igs_iec61850_mms_core::settings`. This module
//! keeps the values of the groups and applies the rules to the served values: the values with FC `SG` show the active group, and the
//! values with FC `SE` show the group that is edited.

use igs_iec61850_mms_core::acsi::timestamp_to_data;
use igs_iec61850_mms_core::control::ClientId;
use igs_iec61850_mms_core::model::layout::value_at;
use igs_iec61850_mms_core::settings::{Refusal, Sgcb};
use igs_iso9506_mms_codec::generated::types::{AccessResult, Data, DataAccessError};
use igs_iso9506_mms_codec::mms::UtcTimestamp;

use crate::clock::HostClock;
use crate::updates::{Change, UpdateError, Updater};
use igs_iec61850_mms_core::time::ClockSource;

/// The name of the SGCB within its device, as an MMS name of the served model.
pub(crate) const SGCB: &str = "LLN0$SP$SGCB";

/// An attribute of the SGCB (8-1 table 71).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attribute {
    NumOfSg,
    ActSg,
    EditSg,
    CnfEdit,
    LActTm,
    ResvTms,
}

/// What an item names, for the SGCB of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Locus {
    /// The whole SGCB: a structure of its attributes.
    Structure,
    /// One attribute of the SGCB.
    Attribute(Attribute),
    /// A setting value with FC `SE`, of the setting at this index: SetEditSGValue writes it.
    SetEdit(usize),
}

/// A setting of the device: its value in the active group (FC `SG`) and its value in the edit buffer (FC `SE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Setting {
    /// The served name of the value with FC `SG`, without the device (`LLN0$SG$MltLev$setVal`).
    pub(crate) sg: String,
    /// The served name of the value with FC `SE` (`LLN0$SE$MltLev$setVal`).
    pub(crate) se: String,
}

/// The setting groups of one device: its control block, its settings, and the values of each group.
#[derive(Debug, Clone)]
pub(crate) struct DeviceSettings {
    device: String,
    sgcb: Sgcb,
    last_selection: Option<UtcTimestamp>,
    settings: Vec<Setting>,
    /// `groups[g - 1][i]` is the value of setting `i` in group `g`.
    groups: Vec<Vec<Data>>,
}

impl DeviceSettings {
    /// The setting groups of a device with `num_of_sgs` groups, the group `active` active, and the given settings, whose values in every group
    /// are the initial values.
    pub(crate) fn new(
        device: &str,
        num_of_sgs: u8,
        active: u8,
        settings: Vec<Setting>,
        initial: Vec<Data>,
    ) -> Self {
        let sgcb = Sgcb::new(num_of_sgs, active);
        let groups = vec![initial; usize::from(sgcb.num_of_sgs())];
        Self {
            device: device.to_string(),
            sgcb,
            last_selection: None,
            settings,
            groups,
        }
    }

    /// The device of these setting groups.
    pub(crate) fn device(&self) -> &str {
        &self.device
    }

    /// What an item of a device names, when it is an item of its SGCB or of its setting values with FC `SE`.
    pub(crate) fn locate(&self, item: &str) -> Option<Locus> {
        if item == SGCB {
            return Some(Locus::Structure);
        }
        if let Some(attribute) = item
            .strip_prefix(SGCB)
            .and_then(|rest| rest.strip_prefix('$'))
        {
            return match attribute {
                "NumOfSG" => Some(Locus::Attribute(Attribute::NumOfSg)),
                "ActSG" => Some(Locus::Attribute(Attribute::ActSg)),
                "EditSG" => Some(Locus::Attribute(Attribute::EditSg)),
                "CnfEdit" => Some(Locus::Attribute(Attribute::CnfEdit)),
                "LActTm" => Some(Locus::Attribute(Attribute::LActTm)),
                "ResvTms" => Some(Locus::Attribute(Attribute::ResvTms)),
                _ => None,
            };
        }
        self.settings
            .iter()
            .position(|setting| setting.se == item)
            .map(Locus::SetEdit)
    }

    /// Reads an item of the SGCB. The reservation is checked against `now` first, so that a timed-out editing reads as free.
    pub(crate) fn read(&mut self, now: std::time::Duration, locus: Locus) -> AccessResult {
        self.sgcb.expire(now);
        let value = match locus {
            Locus::Structure => Data::Structure(vec![
                self.attribute(Attribute::NumOfSg),
                self.attribute(Attribute::ActSg),
                self.attribute(Attribute::EditSg),
                self.attribute(Attribute::CnfEdit),
                self.attribute(Attribute::LActTm),
                self.attribute(Attribute::ResvTms),
            ]),
            Locus::Attribute(attribute) => self.attribute(attribute),
            // A setting value is read through the VMD, not through the SGCB.
            Locus::SetEdit(_) => {
                return AccessResult::Failure(DataAccessError::OBJECT_ACCESS_DENIED)
            }
        };
        AccessResult::Success(value)
    }

    fn attribute(&self, attribute: Attribute) -> Data {
        match attribute {
            Attribute::NumOfSg => Data::Unsigned(u128::from(self.sgcb.num_of_sgs())),
            Attribute::ActSg => Data::Unsigned(u128::from(self.sgcb.act_sg())),
            Attribute::EditSg => Data::Unsigned(u128::from(self.sgcb.edit_sg())),
            Attribute::CnfEdit => Data::Boolean(self.sgcb.cnf_edit()),
            Attribute::LActTm => timestamp_to_data(self.last_selection.unwrap_or(UtcTimestamp {
                seconds: 0,
                fraction: 0,
                quality: HostClock::default().quality(),
            })),
            Attribute::ResvTms => Data::Unsigned(u128::from(self.sgcb.resv_tms().as_secs())),
        }
    }

    /// Writes an item of the SGCB, or a setting value with FC `SE` (SetEditSGValue). The service rules of the core decide; the values
    /// of the served groups follow them.
    pub(crate) fn write(
        &mut self,
        updater: &Updater,
        now: std::time::Duration,
        client: ClientId,
        locus: Locus,
        data: &Data,
    ) -> Result<(), DataAccessError> {
        match (locus, data) {
            (Locus::Attribute(Attribute::ActSg), Data::Unsigned(value)) => {
                let group = group_number(*value)?;
                self.sgcb.select_active(now, group).map_err(refusal_error)?;
                self.last_selection = Some(stamp_now());
                self.apply_active(updater)
            }
            (Locus::Attribute(Attribute::EditSg), Data::Unsigned(value)) => {
                let group = group_number(*value)?;
                self.sgcb
                    .select_edit(now, client, group)
                    .map_err(refusal_error)?;
                if group > 0 {
                    self.apply_edit(updater, group)?;
                }
                Ok(())
            }
            (Locus::Attribute(Attribute::CnfEdit), Data::Boolean(true)) => {
                let confirmation = self.sgcb.confirm(now, client).map_err(refusal_error)?;
                let edited = self.read_edit_buffer(updater)?;
                let index = usize::from(confirmation.group).saturating_sub(1);
                if let Some(group) = self.groups.get_mut(index) {
                    *group = edited.clone();
                }
                if confirmation.active {
                    self.last_selection = Some(stamp_now());
                    self.apply_active(updater)?;
                }
                self.sgcb.storage_done();
                Ok(())
            }
            // A confirmation of FALSE changes nothing (`CnfEdit` returns to FALSE after the storage).
            (Locus::Attribute(Attribute::CnfEdit), Data::Boolean(false)) => Ok(()),
            (Locus::SetEdit(index), value) => {
                self.sgcb.may_set_edit(now, client).map_err(refusal_error)?;
                let Some(setting) = self.settings.get(index) else {
                    return Err(DataAccessError::OBJECT_NON_EXISTENT);
                };
                let change = Change::new(format!("{}/{}", self.device, setting.se), value.clone());
                updater.update(&[change]).map(|_| ()).map_err(update_error)
            }
            (Locus::Attribute(Attribute::ActSg | Attribute::EditSg | Attribute::CnfEdit), _) => {
                Err(DataAccessError::TYPE_INCONSISTENT)
            }
            _ => Err(DataAccessError::OBJECT_ACCESS_DENIED),
        }
    }

    /// The association of a client is lost: its editing reservation is released (7-2 §16.3.3).
    pub(crate) fn released(&mut self, client: ClientId) {
        self.sgcb.lost(client);
    }

    /// The values with FC `SG` take the active group.
    fn apply_active(&self, updater: &Updater) -> Result<(), DataAccessError> {
        let group = usize::from(self.sgcb.act_sg()).saturating_sub(1);
        let values = self.groups.get(group).cloned().unwrap_or_default();
        self.apply(updater, values, false)
    }

    /// The values with FC `SE` take the group that is edited.
    fn apply_edit(&self, updater: &Updater, group: u8) -> Result<(), DataAccessError> {
        let values = self
            .groups
            .get(usize::from(group).saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        self.apply(updater, values, true)
    }

    fn apply(
        &self,
        updater: &Updater,
        values: Vec<Data>,
        edit: bool,
    ) -> Result<(), DataAccessError> {
        if self.settings.is_empty() {
            return Ok(());
        }
        let changes: Vec<Change> = self
            .settings
            .iter()
            .zip(values)
            .map(|(setting, value)| {
                let name = if edit { &setting.se } else { &setting.sg };
                Change::new(format!("{}/{name}", self.device), value)
            })
            .collect();
        updater.update(&changes).map(|_| ()).map_err(update_error)
    }

    /// The values with FC `SE`, in the order of the settings: the edit buffer, which a confirmation copies into the group.
    fn read_edit_buffer(&self, updater: &Updater) -> Result<Vec<Data>, DataAccessError> {
        self.settings
            .iter()
            .map(|setting| read_served(updater, &self.device, &setting.se))
            .collect::<Option<Vec<Data>>>()
            .ok_or(DataAccessError::OBJECT_VALUE_INVALID)
    }
}

/// The value of a served item, read from its node.
pub(crate) fn read_served(updater: &Updater, device: &str, item: &str) -> Option<Data> {
    let (root, path) = updater.point(&format!("{device}/{item}"))?;
    let state = root.lock().ok()?;
    value_at(&state.data, &path).cloned()
}

/// The setting groups of a configuration: one per device with a SettingControl. Each setting is a value with FC `SG` of the LLN0 of the
/// device, with its value with FC `SE`; the values of each group start at the values served at the start.
#[cfg(feature = "scl")]
pub(crate) fn devices_of(
    configuration: &igs_iec61850_mms_core::scl::build::Configuration,
    updater: &Updater,
) -> Vec<DeviceSettings> {
    let mut devices = Vec::new();
    for declared in &configuration.setting_groups {
        let Some(device) = configuration
            .model
            .devices()
            .iter()
            .find(|device| device.name().to_string() == declared.device)
        else {
            continue;
        };
        let Some(node) = device
            .nodes()
            .iter()
            .find(|node| node.name().to_string() == "LLN0")
        else {
            continue;
        };
        let Ok(layout) = igs_iec61850_mms_core::model::layout::layout(
            node,
            igs_iec61850_mms_core::model::layout::IDENTIFIER_LIMIT,
        ) else {
            continue;
        };
        let sg_names: Vec<String> = layout
            .variables
            .iter()
            .filter(|variable| variable.functional_constraint == Some("SG"))
            .map(|variable| variable.name.clone())
            .collect();
        let mut settings = Vec::new();
        let mut initial = Vec::new();
        for sg in sg_names {
            let se = sg.replacen("$SG$", "$SE$", 1);
            let Some(value) = read_served(updater, &declared.device, &sg) else {
                continue;
            };
            if layout.variables.iter().any(|variable| variable.name == se) {
                settings.push(Setting { sg, se });
                initial.push(value);
            }
        }
        let num = u8::try_from(declared.num_of_sgs).unwrap_or(u8::MAX);
        let active = u8::try_from(declared.active).unwrap_or(1);
        devices.push(DeviceSettings::new(
            &declared.device,
            num,
            active,
            settings,
            initial,
        ));
    }
    devices
}

fn group_number(value: u128) -> Result<u8, DataAccessError> {
    u8::try_from(value).map_err(|_| DataAccessError::OBJECT_VALUE_INVALID)
}

fn stamp_now() -> UtcTimestamp {
    HostClock::default().now()
}

fn refusal_error(refusal: Refusal) -> DataAccessError {
    match refusal {
        Refusal::OutOfRange | Refusal::NothingToConfirm => DataAccessError::OBJECT_VALUE_INVALID,
        Refusal::Reserved => DataAccessError::TEMPORARILY_UNAVAILABLE,
        _ => DataAccessError::OBJECT_ACCESS_DENIED,
    }
}

fn update_error(error: UpdateError) -> DataAccessError {
    match error {
        UpdateError::TypeMismatch(_) => DataAccessError::TYPE_INCONSISTENT,
        _ => DataAccessError::OBJECT_VALUE_INVALID,
    }
}
