// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! A server that serves the demonstration model, and changes its status every second through the updater of the model, as an
//! application does. It runs until Ctrl-C.
//!
//! Run `cargo run --example server -- 127.0.0.1:11102`, then read it with `cargo run --example client`.

use std::time::Duration;

use igs_iec61850_mms::demo::{demo_model, DEVICE};
use igs_iec61850_mms::model::{server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iec61850_mms::updates::Change;
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listen = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:11102".to_string());
    let served = ServedModel::new(&demo_model()?, server_identity())?;
    let updater = served.updater();
    let server = Server::bind_model(&listen, server_config(LinkConfig::default()), served).await?;
    println!("listening on {}", server.local_address());

    let attribute = format!("{DEVICE}/GGIO1$ST$Ind1$stVal");
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    let mut state = false;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                state = !state;
                // One update is one revision: every change of the node is read as one state (MODEL-006).
                updater.update(&[Change::new(attribute.as_str(), Data::Boolean(state))])?;
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    server.shutdown().await;
    Ok(())
}
