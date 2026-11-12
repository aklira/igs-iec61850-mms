// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! `igs61850-server`: serves the demonstration model over MMS on the IEC 61850 profile (task A7). It runs until it is
//! interrupted (Ctrl-C), then closes its associations.

use std::process::ExitCode;

use clap::Parser;
use igs_iec61850_mms::demo::demo_model;
use igs_iec61850_mms::model::{server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms_link::config::LinkConfig;

#[derive(Parser)]
#[command(
    name = "igs61850-server",
    version,
    about = "Serves the demonstration IEC 61850 model over MMS"
)]
struct Cli {
    /// The address to listen on, as host:port.
    #[arg(long, default_value = "127.0.0.1:11102")]
    listen: String,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli.listen).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("igs61850-server: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(listen: &str) -> Result<(), Box<dyn std::error::Error>> {
    let model = demo_model()?;
    let served = ServedModel::new(&model, server_identity())?;
    let server = Server::bind_model(listen, server_config(LinkConfig::default()), served).await?;
    println!("igs61850-server: listening on {}", server.local_address());
    tokio::signal::ctrl_c().await?;
    server.shutdown().await;
    Ok(())
}
