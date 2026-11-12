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

/// The name of an additional cause, from its value in the catalogue enumeration `ControlServiceStatusKind`.
pub fn cause_name(value: i64) -> Option<&'static str> {
    catalogue::enumeration("ControlServiceStatusKind")?
        .literals
        .iter()
        .find(|literal| i64::from(literal.value) == value)
        .map(|literal| literal.name)
}

/// The outcome that a report carries, if the report is a `CommandTermination` or a `LastApplError`.
pub fn termination(report: &UnconfirmedService) -> Option<Termination> {
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
                cause_name: cause_name(cause),
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
fn component_item(device: &str, object: &str, component: &str) -> Result<MmsName, ClientError> {
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
