// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The gateway building blocks (task G1; GW-001): the values of downstream servers mirrored into the model of an upstream server, in one
//! runtime. A client of each downstream server reads the mirrored variables, and the values go into the upstream model through the
//! updater, the application's path to the model (task A5), which never waits for the network. The upstream server answers its own
//! clients from that model.
//!
//! A mirror is explicit: each downstream variable (device, name) maps to one upstream variable (device, name), with the same MMS type.
//! The quality and the loss of a downstream association are the task G2.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use igs_iec61850_mms_core::acsi::{Quality, Validity};
use igs_iec61850_mms_core::control::{AddCause, CheckKind};
use igs_iso9506_mms_codec::generated::types::Data;

use crate::client::IecClient;
use crate::client_control::{termination, Command, Termination};
use crate::control::{ControlHandle, ControlHandler};
use crate::updates::{Change, Updater};
use igs_iso9506_mms::client::{Event, Events};

/// One mirrored variable: the downstream device and variable, and the upstream device and variable it sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirror {
    /// The downstream logical device.
    pub downstream_device: String,
    /// The downstream variable, as its name within the device.
    pub downstream_variable: String,
    /// The upstream logical device.
    pub upstream_device: String,
    /// The upstream variable, as its name within the device.
    pub upstream_variable: String,
}

/// Why a mirror step failed: the downstream read, or the upstream update.
#[derive(Debug)]
#[non_exhaustive]
pub enum MirrorError {
    /// The downstream association answered with an error for the whole read.
    Read(String),
    /// The upstream model refused the update (a value of another type, or an unknown variable).
    Update(String),
}

impl std::fmt::Display for MirrorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(text) => write!(f, "the downstream read failed: {text}"),
            Self::Update(text) => write!(f, "the upstream update failed: {text}"),
        }
    }
}

impl std::error::Error for MirrorError {}

/// The quality variable of a mirrored status: the `q` of the same data object as its `stVal`.
fn quality_variable(variable: &str) -> String {
    format!("{}q", variable.trim_end_matches("stVal"))
}

/// Sets the quality of the mirrored statuses (the upstream `q` of each `stVal`), with one update per upstream node.
fn set_quality(
    updater: &Updater,
    mirrors: &[Mirror],
    validity: Validity,
) -> Result<(), MirrorError> {
    let mut by_node: BTreeMap<String, Vec<Change>> = BTreeMap::new();
    for mirror in mirrors {
        let node = mirror
            .upstream_variable
            .split('$')
            .next()
            .unwrap_or_default()
            .to_string();
        let quality = Quality {
            validity,
            ..Quality::good()
        };
        by_node
            .entry(format!("{}/{}", mirror.upstream_device, node))
            .or_default()
            .push(Change::new(
                format!(
                    "{}/{}",
                    mirror.upstream_device,
                    quality_variable(&mirror.upstream_variable)
                ),
                quality.to_data(),
            ));
    }
    for changes in by_node.values() {
        updater
            .update(changes)
            .map_err(|error| MirrorError::Update(error.to_string()))?;
    }
    Ok(())
}

/// One step of a mirror: read the downstream variables of one client, and set the upstream variables through the updater, with the
/// quality good (GW-003). The values are taken in order; an item that the downstream does not answer is left out of the step (it is not
/// guessed). Returns the number of upstream variables set.
pub async fn mirror_step(
    downstream: &IecClient,
    updater: &Updater,
    mirrors: &[Mirror],
) -> Result<usize, MirrorError> {
    let mut by_node: BTreeMap<String, Vec<Change>> = BTreeMap::new();
    let mut count = 0usize;
    for mirror in mirrors {
        let item = igs_iec61850_mms_core::names::MmsName::new(
            igs_iec61850_mms_core::names::Scope::Domain(
                igs_iec61850_mms_core::names::Name::new(&mirror.downstream_device)
                    .map_err(|error| MirrorError::Read(error.to_string()))?,
            ),
            igs_iec61850_mms_core::names::MmsIdentifier::new(&mirror.downstream_variable)
                .map_err(|error| MirrorError::Read(error.to_string()))?,
        );
        let results = downstream
            .read(&[item])
            .await
            .map_err(|error| MirrorError::Read(error.to_string()))?;
        let Some(Ok(value)) = results.into_iter().next() else {
            continue;
        };
        let node = mirror
            .upstream_variable
            .split('$')
            .next()
            .unwrap_or_default()
            .to_string();
        by_node
            .entry(format!("{}/{}", mirror.upstream_device, node))
            .or_default()
            .push(Change::new(
                format!("{}/{}", mirror.upstream_device, mirror.upstream_variable),
                value,
            ));
        let quality = Quality::good();
        by_node
            .entry(format!("{}/{}", mirror.upstream_device, node))
            .or_default()
            .push(Change::new(
                format!(
                    "{}/{}",
                    mirror.upstream_device,
                    quality_variable(&mirror.upstream_variable)
                ),
                quality.to_data(),
            ));
    }
    for changes in by_node.values() {
        count = count.saturating_add(changes.len());
        updater
            .update(changes)
            .map_err(|error| MirrorError::Update(error.to_string()))?;
    }
    Ok(count)
}

/// Runs the mirror of one downstream client every `period`, until `stop` is set. When a step fails, the quality of the mirrored statuses
/// becomes invalid (GW-003: a stale value is not kept as good), and the error is returned to the caller, which decides what to do: this
/// loop does not hide it, and it does not reconnect.
pub async fn run_mirror(
    downstream: IecClient,
    updater: Updater,
    mirrors: Vec<Mirror>,
    period: Duration,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> Result<(), MirrorError> {
    let mut ticker = tokio::time::interval(period);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Err(error) = mirror_step(&downstream, &updater, &mirrors).await {
                    let _ = set_quality(&updater, &mirrors, Validity::Invalid);
                    return Err(error);
                }
            }
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    return Ok(());
                }
            }
        }
    }
}

/// The control forwarding of a gateway (GW-004): a command of the upstream control object is sent to the downstream control object, and
/// the upstream outcome is the downstream one. The upstream command has already answered its operate positively; its termination waits for
/// the downstream outcome, which the outcome pump reports through the handle of the upstream model (task K3).
pub struct ForwardHandler {
    /// The client of the downstream server.
    pub downstream: Arc<IecClient>,
    /// The downstream logical device of the control object.
    pub device: String,
    /// The downstream component of the control object: `node$CO$do`.
    pub component: String,
    /// The handle of the upstream model, for the outcomes.
    pub upstream: ControlHandle,
    /// The reference of the upstream control object (`device/node$CO$do`).
    pub reference: String,
    /// The originator of the forwarded commands (the gateway).
    pub origin: igs_iec61850_mms_core::acsi::Originator,
}

impl ControlHandler for ForwardHandler {
    fn check(
        &mut self,
        _object: &str,
        _kind: CheckKind,
        _ctl_val: Option<&Data>,
    ) -> Result<(), AddCause> {
        Ok(())
    }

    fn execute(&mut self, _object: &str, ctl_val: &Data) {
        let downstream = Arc::clone(&self.downstream);
        let device = self.device.clone();
        let component = self.component.clone();
        let upstream = self.upstream.clone();
        let reference = self.reference.clone();
        let command = Command {
            ctl_val: ctl_val.clone(),
            origin: self.origin.clone(),
            ctl_num: 1,
            test: false,
            check: igs_iec61850_mms_core::acsi::CheckConditions {
                synchrocheck: false,
                interlock_check: false,
            },
        };
        tokio::spawn(async move {
            match downstream.operate(&device, &component, &command).await {
                Ok(Ok(())) => {}
                _ => {
                    let _ = upstream.complete(&reference, Err(AddCause::UNKNOWN));
                }
            }
        });
    }
}

/// The outcome pump of a downstream client: each termination the downstream reports completes the upstream command (the positive one as a
/// change does, the negative one with the downstream cause). The task ends when the association ends.
pub fn forward_outcomes(
    mut events: Events,
    upstream: ControlHandle,
    reference: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = events.next().await {
            let Event::Report(service) = event else {
                continue;
            };
            let Some(outcome) = termination(&service) else {
                continue;
            };
            let result = match outcome {
                Termination::Positive { .. } => Ok(()),
                Termination::Negative { cause_name, .. } => {
                    Err(cause_name.map(AddCause::named).unwrap_or(AddCause::UNKNOWN))
                }
            };
            let _ = upstream.complete(&reference, result);
        }
    })
}
