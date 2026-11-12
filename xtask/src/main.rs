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

mod catalogue;
mod statements;

const USAGE: &str = "usage: cargo xtask <catalogue | statements> [--check]";

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("catalogue") => {
            let check = std::env::args().nth(2).as_deref() == Some("--check");
            match catalogue::run(&catalogue::workspace_root(), check) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("cargo xtask catalogue: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("statements") => {
            let check = std::env::args().nth(2).as_deref() == Some("--check");
            match statements::run(&catalogue::workspace_root(), check) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("cargo xtask statements: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
