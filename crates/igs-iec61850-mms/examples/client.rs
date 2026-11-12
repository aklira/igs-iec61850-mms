// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! A client that connects to a server, lists its logical devices, and reads the status of the demonstration model.
//!
//! Run a server first (`cargo run --bin igs61850-server`), then `cargo run --example client -- 127.0.0.1:11102`.

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::demo::DEVICE;
use igs_iec61850_mms::model::client_config;
use igs_iec61850_mms_core::names::{MmsIdentifier, MmsName, Name, Scope};
use igs_iso9506_mms::{ClientConfig, Reconnect};
use igs_iso9506_mms_link::config::LinkConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:11102".to_string());
    let config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, _events) = IecClient::connect(&address, config).await?;

    println!("logical devices: {:?}", client.domains().await?);
    let status = MmsName::new(
        Scope::Domain(Name::new(DEVICE)?),
        MmsIdentifier::new("GGIO1$ST$Ind1$stVal")?,
    );
    for result in client.read(&[status]).await? {
        match result {
            Ok(value) => println!("{DEVICE}/GGIO1.Ind1.stVal: {value:?}"),
            Err(error) => println!("{DEVICE}/GGIO1.Ind1.stVal refused: {error:?}"),
        }
    }
    client.close().await?;
    Ok(())
}
