// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! `igs61850-server`: serves a model over MMS on the IEC 61850 profile (task A7). It serves the demonstration model, or, with `--scl`
//! (built with the `scl` feature), the configuration of an IED of an SCL file (task S3). With `--toggle-ms`, it changes the status
//! `GGIO1.Ind1.stVal` of its model periodically through its updater, so that its reports are sent. It runs until it is interrupted
//! (Ctrl-C), then closes its associations.

use std::error::Error;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use igs_iec61850_mms::demo::{demo_model, DEVICE};
use igs_iec61850_mms::model::{server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iec61850_mms::updates::Change;
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

#[derive(Parser)]
#[command(
    name = "igs61850-server",
    version,
    about = "Serves an IEC 61850 model over MMS: the demonstration model, or an IED of an SCL file"
)]
struct Cli {
    /// The address to listen on, as host:port.
    #[arg(long, default_value = "127.0.0.1:11102")]
    listen: String,
    /// The SCL file whose IED is served (needs the `scl` feature).
    #[cfg(feature = "scl")]
    #[arg(long)]
    scl: Option<std::path::PathBuf>,
    /// The IED of the SCL file to serve.
    #[cfg(feature = "scl")]
    #[arg(long, default_value = "IED1")]
    ied: String,
    /// Changes the status `GGIO1.Ind1.stVal` every this many milliseconds (zero: it does not change).
    #[arg(long, default_value_t = 0)]
    toggle_ms: u64,
    /// A directory whose regular files the server serves through the file services, each under its own name (X3).
    #[arg(long, value_name = "DIR")]
    files_dir: Option<std::path::PathBuf>,
}

/// The largest file that the server loads from its directory: a file above it is refused, and the server does not start.
const FILE_LIMIT: u64 = 16 * 1024 * 1024;

/// Loads the regular files of a directory into the file store of the served model. Subdirectories and links are not followed.
/// Returns how many files were loaded.
fn load_files(
    served: &mut ServedModel,
    directory: &std::path::Path,
) -> Result<usize, Box<dyn Error>> {
    let mut loaded: usize = 0;
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() {
            continue;
        }
        if metadata.len() > FILE_LIMIT {
            return Err(format!("{}: larger than 16 MiB, not loaded", path.display()).into());
        }
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| format!("{}: the name is not UTF-8", path.display()))?;
        served.add_file(&[name], std::fs::read(&path)?);
        loaded = loaded.saturating_add(1);
    }
    Ok(loaded)
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("igs61850-server: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The served model, and the attribute that `--toggle-ms` changes, when the model has the status.
#[cfg_attr(not(feature = "scl"), allow(unused_variables))]
fn served(cli: &Cli) -> Result<(ServedModel, Option<String>), Box<dyn Error>> {
    #[cfg(feature = "scl")]
    if let Some(path) = &cli.scl {
        use igs_iec61850_mms_core::scl::build::{build, Capabilities};
        use igs_iec61850_mms_core::scl::validate::validate;
        use igs_iec61850_mms_core::scl::{parse, Limits};

        let text = std::fs::read(path)?;
        let name = path.display().to_string();
        let document =
            parse(&text, &Limits::default()).map_err(|error| format!("{name}: {error:?}"))?;
        validate(&document, &name).map_err(|found| format!("{name}: {found:?}"))?;
        let configuration = build(&document, &name, &cli.ied, &Capabilities::current())
            .map_err(|found| format!("{name}: {found:?}"))?;
        let status = configuration
            .data_sets
            .iter()
            .flat_map(|set| set.variables.iter())
            .find(|variable| variable.name == "GGIO1$ST$Ind1$stVal")
            .map(|variable| format!("{}/{}", variable.device, variable.name));
        return Ok((
            ServedModel::from_configuration(&configuration, server_identity())?,
            status,
        ));
    }
    let status = format!("{DEVICE}/GGIO1$ST$Ind1$stVal");
    Ok((
        ServedModel::new(&demo_model()?, server_identity())?,
        Some(status),
    ))
}

async fn run(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let (mut served, status) = served(cli)?;
    if let Some(directory) = &cli.files_dir {
        let loaded = load_files(&mut served, directory)?;
        println!(
            "igs61850-server: {loaded} file(s) served from {}",
            directory.display()
        );
    }
    let updater = served.updater();
    let server =
        Server::bind_model(&cli.listen, server_config(LinkConfig::default()), served).await?;
    println!("igs61850-server: listening on {}", server.local_address());
    let toggling = status.filter(|_| cli.toggle_ms > 0);
    let mut ticker = tokio::time::interval(Duration::from_millis(cli.toggle_ms.max(1)));
    let mut state = false;
    loop {
        tokio::select! {
            _ = ticker.tick(), if toggling.is_some() => {
                if let Some(attribute) = &toggling {
                    state = !state;
                    updater.update(&[Change::new(attribute.as_str(), Data::Boolean(state))])?;
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    server.shutdown().await;
    Ok(())
}
