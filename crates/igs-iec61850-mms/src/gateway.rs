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
use std::time::Duration;

use crate::client::IecClient;
use crate::updates::{Change, Updater};

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

/// One step of a mirror: read the downstream variables of one client, and set the upstream variables through the updater. The values
/// are taken in order; an item that the downstream does not answer is left out of the step (it is not guessed). Returns the number of
/// upstream variables set.
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
    }
    for changes in by_node.values() {
        count = count.saturating_add(changes.len());
        updater
            .update(changes)
            .map_err(|error| MirrorError::Update(error.to_string()))?;
    }
    Ok(count)
}

/// Runs the mirror of one downstream client every `period`, until `stop` is set. An error is returned to the caller of the step, which
/// decides what to do with it: this loop does not hide it.
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
                mirror_step(&downstream, &updater, &mirrors).await?;
            }
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    return Ok(());
                }
            }
        }
    }
}
