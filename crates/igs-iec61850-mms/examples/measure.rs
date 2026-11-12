// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Measures the reads of the demonstration model over loopback (SPECIFICATIONS §12.4): the throughput and the latency distribution of
//! one read, with a number of concurrent associations. The workload is printed with the result, and the hardware the machine reports.
//!
//! Run `cargo run --release --example measure -- <associations> <reads per association>`. The numbers are for this machine and this
//! workload only: loopback, one item per read, no secured transport, no reports. They are not an application performance claim.

use std::time::{Duration, Instant};

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::demo::{demo_model, DEVICE};
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iec61850_mms_core::names::{MmsIdentifier, MmsName, Name, Scope};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::{ClientConfig, Reconnect};
use igs_iso9506_mms_link::config::LinkConfig;

const STATUS: &str = "GGIO1$ST$Ind1$stVal";

/// One association: connects, then reads the status `reads` times, one request at a time; returns the duration of each read.
async fn one_association(address: &str, reads: usize) -> Result<Vec<Duration>, String> {
    let config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, _events) = IecClient::connect(address, config)
        .await
        .map_err(|error| error.to_string())?;
    let item = MmsName::new(
        Scope::Domain(Name::new(DEVICE).map_err(|error| error.to_string())?),
        MmsIdentifier::new(STATUS).map_err(|error| error.to_string())?,
    );
    let mut durations = Vec::with_capacity(reads);
    for _ in 0..reads {
        let started = Instant::now();
        let results = client
            .read(std::slice::from_ref(&item))
            .await
            .map_err(|error| error.to_string())?;
        if !matches!(results.first(), Some(Ok(_))) {
            return Err("a read was refused".to_string());
        }
        durations.push(started.elapsed());
    }
    client.close().await.map_err(|error| error.to_string())?;
    Ok(durations)
}

/// The value at the fraction `p` of the sorted durations (nearest rank).
fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let last = sorted.len().saturating_sub(1);
    let rank = ((last as f64) * p).round() as usize;
    sorted.get(rank.min(last)).copied().unwrap_or_default()
}

/// The processor model and the number of threads the machine offers, as far as the system tells them.
fn hardware() -> String {
    let model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split(':').nth(1))
                .map(|name| name.trim().to_string())
        })
        .unwrap_or_else(|| "unknown processor".to_string());
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(0);
    format!(
        "{model}, {threads} hardware threads, {}",
        std::env::consts::OS
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let associations: usize = arguments.next().map_or(Ok(8), |text| text.parse())?;
    let reads: usize = arguments.next().map_or(Ok(500), |text| text.parse())?;

    let served = ServedModel::new(&demo_model()?, server_identity())?;
    let server =
        Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served).await?;
    let address = server.local_address().to_string();

    let started = Instant::now();
    let mut tasks = Vec::with_capacity(associations);
    for _ in 0..associations {
        let address = address.clone();
        tasks.push(tokio::spawn(async move {
            one_association(&address, reads).await
        }));
    }
    let mut latencies = Vec::with_capacity(associations.saturating_mul(reads));
    let mut failed = 0usize;
    for task in tasks {
        match task.await {
            Ok(Ok(mut durations)) => latencies.append(&mut durations),
            Ok(Err(error)) => {
                failed = failed.saturating_add(1);
                eprintln!("an association failed: {error}");
            }
            Err(error) => {
                failed = failed.saturating_add(1);
                eprintln!("an association task failed: {error}");
            }
        }
    }
    let elapsed = started.elapsed();
    server.shutdown().await;

    latencies.sort();
    let completed = latencies.len();
    let per_second = (completed as f64) / elapsed.as_secs_f64();
    println!("hardware: {}", hardware());
    println!("workload: {associations} associations x {reads} reads of {DEVICE}/{STATUS}, loopback, no reports, no secured transport");
    println!("associations failed: {failed}");
    println!(
        "reads completed: {completed} in {:.3} s",
        elapsed.as_secs_f64()
    );
    println!("throughput: {per_second:.0} reads/s");
    for (label, p) in [("p50", 0.50), ("p90", 0.90), ("p99", 0.99), ("max", 1.0)] {
        println!(
            "latency {label}: {:.3} ms",
            percentile(&latencies, p).as_secs_f64() * 1000.0
        );
    }
    Ok(())
}
