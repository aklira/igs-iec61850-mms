// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The typed control API of the client (task K3; ACSI-001, CTL-004): the four services of the control models over the MMS services of
//! IEC 61850-8-1 §20, and the decoding of the outcomes the server reports (a `CommandTermination`, or a `LastApplError`).
//!
//! A select is a read of `SBO` (8-1 §20.5): a positive answer is the object reference, a refusal is the NULL value. The services that
//! carry a command (SelectWithValue, Operate, Cancel) are writes of their structure (table 94). The outcome of an operation is reported
//! asynchronously: the client decodes it with [`termination`] from the reports that its association receives.

use igs_iec61850_mms_core::acsi::{
    enumeration_from_data, timestamp_to_data, CheckConditions, Originator,
};
use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::names::{MmsIdentifier, MmsName, Name, Scope};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, Identifier, ObjectName, UnconfirmedService, VariableAccessSpecification,
    VariableSpecification,
};
use igs_iso9506_mms_codec::mms::UtcTimestamp;

use crate::client::{ClientError, IecClient, ItemError};

/// The parameters of a control command, as the client sends them (table 94): the control value, the originator, the control
/// number, the test flag, and the checks. The time of the command is the zero timestamp: the server does not check it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The control value (`ctlVal`).
    pub ctl_val: Data,
    /// The originator (`origin`).
    pub origin: Originator,
    /// The control number (`ctlNum`).
    pub ctl_num: u8,
    /// Whether the command is a test (`Test`).
    pub test: bool,
    /// The checks the command asks for (`Check`).
    pub check: CheckConditions,
}

impl Command {
    /// The structure of the command, in the order of table 94.
    fn to_data(&self) -> Result<Data, ClientError> {
        let time = UtcTimestamp::from_octets([0; 8])
            .map_err(|_| ClientError::Response("the time of the command"))?;
        let origin = self
            .origin
            .to_data()
            .map_err(|_| ClientError::Response("the originator cannot be encoded"))?;
        Ok(Data::Structure(vec![
            self.ctl_val.clone(),
            origin,
            Data::Unsigned(u128::from(self.ctl_num)),
            timestamp_to_data(time),
            Data::Boolean(self.test),
            self.check.to_data(),
        ]))
    }
}

/// The outcome of a command, as the server reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Termination {
    /// The command succeeded: the `CommandTermination` on `Oper`.
    Positive {
        /// The control object (`LD/LN$CO$DO`).
        object: String,
        /// The control number of the command.
        ctl_num: u8,
    },
    /// The command failed: the `LastApplError` (8-1 table 109).
    Negative {
        /// The control object (`LD/LN$CO$DO`).
        object: String,
        /// The error (0 no error, 1 unknown, 2 timeout test not OK, 3 operator test not OK).
        error: i64,
        /// The control number of the command.
        ctl_num: u8,
        /// The additional cause, as the value of `ControlServiceStatusKind`.
        cause: i64,
        /// The name of the additional cause, when the catalogue has the value.
        cause_name: Option<&'static str>,
    },
}

/// The edition of IEC 61850-7-2 that a peer follows, where the editions differ (task E2). The default of the crate is the second
/// edition (the catalogue); the first edition differs in the additional causes 26 and 27, which the corrections of the tissues
/// exchanged (see `docs/edition1` in the private notes, decision D-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edition {
    /// The first edition (2003 and its 2007 revision).
    One,
    /// The second edition (2007B and the amendments): the catalogue of the crate.
    #[default]
    Two,
}

/// The name of an additional cause, from its value in the catalogue enumeration `ControlServiceStatusKind` (the second edition).
pub fn cause_name(value: i64) -> Option<&'static str> {
    cause_name_in(Edition::Two, value)
}

/// The name of an additional cause in an edition: the catalogue, with the values of the first edition for 26 and 27.
pub fn cause_name_in(edition: Edition, value: i64) -> Option<&'static str> {
    if edition == Edition::One {
        match value {
            26 => return Some("Locked-by-other-client"),
            27 => return Some("Inconsistent-parameters"),
            _ => {}
        }
    }
    catalogue::enumeration("ControlServiceStatusKind")?
        .literals
        .iter()
        .find(|literal| i64::from(literal.value) == value)
        .map(|literal| literal.name)
}

/// The outcome that a report carries, if the report is a `CommandTermination` or a `LastApplError` (the second edition).
pub fn termination(report: &UnconfirmedService) -> Option<Termination> {
    termination_in(Edition::Two, report)
}

/// The outcome that a report carries, read in an edition: the names of the additional causes follow the edition.
pub fn termination_in(edition: Edition, report: &UnconfirmedService) -> Option<Termination> {
    let UnconfirmedService::InformationReport(info) = report else {
        return None;
    };
    let AccessResult::Success(Data::Structure(components)) = info.list_of_access_result.first()?
    else {
        return None;
    };
    match &info.variable_access_specification {
        VariableAccessSpecification::VariableListName(ObjectName::VmdSpecific(Identifier(
            name,
        ))) if name == "LastApplError" => {
            let [Data::VisibleString(object), error, _origin, Data::Unsigned(ctl_num), cause] =
                components.as_slice()
            else {
                return None;
            };
            let cause = enumeration_from_data(cause).ok()?;
            Some(Termination::Negative {
                object: object.clone(),
                error: enumeration_from_data(error).ok()?,
                ctl_num: u8::try_from(*ctl_num).ok()?,
                cause,
                cause_name: cause_name_in(edition, cause),
            })
        }
        VariableAccessSpecification::ListOfVariable(items) => {
            let [item] = items.as_slice() else {
                return None;
            };
            let VariableSpecification::Name(ObjectName::DomainSpecific(name)) =
                &item.variable_specification
            else {
                return None;
            };
            let component = name.item_id.0.strip_suffix("$Oper")?;
            let [_, _, Data::Unsigned(ctl_num), ..] = components.as_slice() else {
                return None;
            };
            Some(Termination::Positive {
                object: format!("{}/{}", name.domain_id.0, component),
                ctl_num: u8::try_from(*ctl_num).ok()?,
            })
        }
        _ => None,
    }
}

/// The MMS name of a component of a control object: `LN$CO$DO$component`, in the device's domain.
pub(crate) fn component_item(
    device: &str,
    object: &str,
    component: &str,
) -> Result<MmsName, ClientError> {
    let device = Name::new(device)?;
    let identifier = MmsIdentifier::new(&format!("{object}${component}"))?;
    Ok(MmsName::new(Scope::Domain(device), identifier))
}

impl IecClient {
    /// Select (SBO, a read): `Some(reference)` when the object is selected for this client, `None` when it is refused.
    pub async fn select(&self, device: &str, object: &str) -> Result<Option<String>, ClientError> {
        let item = component_item(device, object, "SBO")?;
        let results = self.read(&[item]).await?;
        Ok(match results.into_iter().next() {
            Some(Ok(Data::VisibleString(reference))) if !reference.is_empty() => Some(reference),
            _ => None,
        })
    }

    /// SelectWithValue (SBOw, a write): the result of the selection of the command.
    pub async fn select_with_value(
        &self,
        device: &str,
        object: &str,
        command: &Command,
    ) -> Result<Result<(), ItemError>, ClientError> {
        self.command(device, object, "SBOw", command).await
    }

    /// Operate (Oper, a write): the result of the operation. The outcome follows as a termination (see [`termination`]).
    pub async fn operate(
        &self,
        device: &str,
        object: &str,
        command: &Command,
    ) -> Result<Result<(), ItemError>, ClientError> {
        self.command(device, object, "Oper", command).await
    }

    /// Cancel (a write): the result of the cancel of the selection or of the operation.
    pub async fn cancel(
        &self,
        device: &str,
        object: &str,
        command: &Command,
    ) -> Result<Result<(), ItemError>, ClientError> {
        self.command(device, object, "Cancel", command).await
    }

    async fn command(
        &self,
        device: &str,
        object: &str,
        component: &str,
        command: &Command,
    ) -> Result<Result<(), ItemError>, ClientError> {
        let item = component_item(device, object, component)?;
        let data = command.to_data()?;
        let mut results = self.write(&[(item, data)]).await?;
        match results.pop() {
            Some(result) => Ok(result),
            None => Err(ClientError::Response("the write answers no item")),
        }
    }
}

/// The edition that a logical device declares in its namespace string (`LLN0.NamPlt.ldNs`, task E2): the first edition names
/// the 2003 namespace of IEC 61850-7-4, the second the 2007 one. `None` when the string is not one of them: the edition is not
/// assumed then.
pub fn edition_of_namespace(text: &str) -> Option<Edition> {
    if text.contains("2003") {
        Some(Edition::One)
    } else if text.contains("2007") {
        Some(Edition::Two)
    } else {
        None
    }
}

impl IecClient {
    /// The edition of a logical device, from its namespace string `LLN0.NamPlt.ldNs` (E2). `None` when the device does not give one of
    /// the known namespaces, or does not give the attribute.
    pub async fn detect_edition(&self, device: &str) -> Result<Option<Edition>, ClientError> {
        let item = component_item(device, "LLN0$EX$NamPlt", "ldNs")?;
        let results = self.read(&[item]).await?;
        Ok(match results.into_iter().next() {
            Some(Ok(Data::VisibleString(text))) => edition_of_namespace(&text),
            _ => None,
        })
    }
}
