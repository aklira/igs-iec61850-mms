// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Sans-I/O core of IEC 61850 over MMS.
//!
//! Object references and their MMS names, ACSI types and their MMS mapping, the model tree and its
//! catalogue, datasets, and the report, control and later log and setting-group state machines. The
//! crate performs no I/O and has no async runtime: state machines take events and the current instant,
//! and return actions. See IMPLEMENTATION.md section 2 and PROVENANCE.md D-001.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )
)]

pub mod acsi;
pub mod catalogue;
pub mod control;
pub mod discovery;
pub mod logs;
pub mod model;

pub mod names;
pub mod reference;
pub mod report;
#[cfg(feature = "scl")]
pub mod scl;
pub mod settings;
pub mod sntp;
pub mod time;
