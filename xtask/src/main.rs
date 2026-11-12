// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Maintenance commands of the workspace: `cargo xtask <command>`.
//!
//! - `catalogue`: generate the CDC and logical-node catalogue from the NSD files (task F3).
//! - `statements`: write the capability statements (task V1).
//!
//! Both read the licensed inputs of `docs/spec/`, which is absent in CI.

#![forbid(unsafe_code)]

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <catalogue | statements> [--check]";

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("catalogue") => not_yet("catalogue", "F3"),
        Some("statements") => not_yet("statements", "V1"),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn not_yet(command: &str, task: &str) -> ExitCode {
    eprintln!("cargo xtask {command}: not implemented yet (task {task} of IMPLEMENTATION.md)");
    ExitCode::FAILURE
}
