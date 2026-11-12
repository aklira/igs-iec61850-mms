// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! IEC 61850 client and server over MMS.
//!
//! The crate binds the sans-I/O core of `igs-iec61850-mms-core` to the client and to the model
//! interface of the MMS crates of `igs-iso9506-mms`, and provides the gateway helpers and the clock
//! source. See IMPLEMENTATION.md section 2.

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

pub mod model;
pub mod reports;

pub mod clock;
pub mod control;
pub mod updates;

pub mod client;
pub mod client_control;
pub mod client_report;

pub mod demo;

use igs_iso9506_mms::vmd::Identity;

/// The identity the demonstration servers report: vendor, model and revision of the VMD.
pub fn server_identity() -> Identity {
    Identity {
        vendor: "igs".to_string(),
        model: "igs61850-demo".to_string(),
        revision: env!("CARGO_PKG_VERSION").to_string(),
    }
}
