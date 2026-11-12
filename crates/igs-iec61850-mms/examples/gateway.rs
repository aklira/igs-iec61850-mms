// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! A gateway step in one process: a downstream server changes a status, a client of the gateway mirrors it into the upstream model, and an
//! upstream client reads it. The mirror is explicit (`Mirror`): nothing is copied that is not named.
//!
//! Run `cargo run --example gateway`. It needs no other process: the downstream and upstream servers listen on the loopback interface.

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::demo::{demo_model, DEVICE};
use igs_iec61850_mms::gateway::{mirror_step, Mirror};
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iec61850_mms::updates::Change;
use igs_iec61850_mms_core::names::{MmsIdentifier, MmsName, Name, Scope};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::{ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

const STATUS: &str = "GGIO1$ST$Ind1$stVal";

async fn serve(
    listen: &str,
) -> Result<(Server, igs_iec61850_mms::updates::Updater), Box<dyn std::error::Error>> {
    let served = ServedModel::new(&demo_model()?, server_identity())?;
    let updater = served.updater();
    let server = Server::bind_model(listen, server_config(LinkConfig::default()), served).await?;
    Ok((server, updater))
}

async fn connect(address: &str) -> Result<IecClient, Box<dyn std::error::Error>> {
    let config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, _events) = IecClient::connect(address, config).await?;
    Ok(client)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (downstream, downstream_updater) = serve("127.0.0.1:0").await?;
    let (upstream, upstream_updater) = serve("127.0.0.1:0").await?;
    let downstream_client = connect(&downstream.local_address().to_string()).await?;
    let upstream_client = connect(&upstream.local_address().to_string()).await?;

    // The downstream status changes; the gateway's model is the upstream one, which the mirror sets.
    downstream_updater.update(&[Change::new(
        format!("{DEVICE}/{STATUS}"),
        Data::Boolean(true),
    )])?;
    let mirrors = [Mirror {
        downstream_device: DEVICE.to_string(),
        downstream_variable: STATUS.to_string(),
        upstream_device: DEVICE.to_string(),
        upstream_variable: STATUS.to_string(),
    }];
    let set = mirror_step(&downstream_client, &upstream_updater, &mirrors).await?;
    println!("mirrored {set} variable(s)");

    let item = MmsName::new(
        Scope::Domain(Name::new(DEVICE)?),
        MmsIdentifier::new(STATUS)?,
    );
    for result in upstream_client.read(&[item]).await? {
        println!("upstream {DEVICE}/{STATUS}: {result:?}");
    }
    downstream_client.close().await?;
    upstream_client.close().await?;
    downstream.shutdown().await;
    upstream.shutdown().await;
    Ok(())
}
