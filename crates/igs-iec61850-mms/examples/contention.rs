// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Measures the contention of the shared model (RUN-004; SPECIFICATIONS §12.4): the time an application update takes while a number of
//! associations read the same model. The updates and the reads go through the one model task, so the update time shows how the reads
//! delay it. The workload and the hardware are printed with the result.
//!
//! Run `cargo run --release --example contention -- <readers> <updates>`. The numbers are for this machine and this workload only:
//! loopback, the demonstration model, reads of one item, no reports (the reports of the model are not in this workload), no secured
//! transport. They are not an application performance claim.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::demo::{demo_model, DEVICE};
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iec61850_mms::updates::Change;
use igs_iec61850_mms_core::names::{MmsIdentifier, MmsName, Name, Scope};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::{ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

const STATUS: &str = "GGIO1$ST$Ind1$stVal";

/// A reader: connects, then reads the status until `stop` is set; returns the duration of each read.
async fn reader(address: String, stop: Arc<AtomicBool>) -> Result<Vec<Duration>, String> {
    let config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, _events) = IecClient::connect(&address, config)
        .await
        .map_err(|error| error.to_string())?;
    let item = MmsName::new(
        Scope::Domain(Name::new(DEVICE).map_err(|error| error.to_string())?),
        MmsIdentifier::new(STATUS).map_err(|error| error.to_string())?,
    );
    let mut durations = Vec::new();
    while !stop.load(Ordering::Relaxed) {
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

fn print_distribution(label: &str, sorted: &[Duration]) {
    for (name, p) in [("p50", 0.50), ("p90", 0.90), ("p99", 0.99), ("max", 1.0)] {
        println!(
            "{label} {name}: {:.3} ms",
            percentile(sorted, p).as_secs_f64() * 1000.0
        );
    }
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
    let readers: usize = arguments.next().map_or(Ok(4), |text| text.parse())?;
    let updates: usize = arguments.next().map_or(Ok(1000), |text| text.parse())?;

    let served = ServedModel::new(&demo_model()?, server_identity())?;
    let updater = served.updater();
    let server =
        Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served).await?;
    let address = server.local_address().to_string();

    let stop = Arc::new(AtomicBool::new(false));
    let mut tasks = Vec::with_capacity(readers);
    for _ in 0..readers {
        tasks.push(tokio::spawn(reader(address.clone(), Arc::clone(&stop))));
    }
    // Let the readers reach their steady state before the first update.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let attribute = format!("{DEVICE}/{STATUS}");
    let mut update_times = Vec::with_capacity(updates);
    let started = Instant::now();
    for step in 0..updates {
        let change = Change::new(attribute.as_str(), Data::Boolean(step % 2 == 0));
        let call = Instant::now();
        updater.update(&[change])?;
        update_times.push(call.elapsed());
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let elapsed = started.elapsed();

    stop.store(true, Ordering::Relaxed);
    let mut read_times = Vec::new();
    let mut failed = 0usize;
    for task in tasks {
        match task.await {
            Ok(Ok(mut durations)) => read_times.append(&mut durations),
            Ok(Err(error)) => {
                failed = failed.saturating_add(1);
                eprintln!("a reader failed: {error}");
            }
            Err(error) => {
                failed = failed.saturating_add(1);
                eprintln!("a reader task failed: {error}");
            }
        }
    }
    server.shutdown().await;

    update_times.sort();
    read_times.sort();
    println!("hardware: {}", hardware());
    println!(
        "workload: {readers} readers reading {DEVICE}/{STATUS} in a loop, {updates} updates of the same item at about 1 ms apart, loopback, no reports, no secured transport"
    );
    println!("readers failed: {failed}");
    println!(
        "updates: {updates} in {:.3} s; reads completed: {}",
        elapsed.as_secs_f64(),
        read_times.len()
    );
    print_distribution("update call", &update_times);
    print_distribution("read", &read_times);
    Ok(())
}
