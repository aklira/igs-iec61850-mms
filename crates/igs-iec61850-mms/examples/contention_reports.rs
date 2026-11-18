// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Measures the report delivery under multi-client load (RUN-004; SPECIFICATIONS §12.4; task X8): N clients each enable their own
//! unbuffered block (RPT1 to RPTn) of a served SCL model, with the data-change trigger and no buffer time; a driver changes the status
//! `Ind1.stVal` at a fixed pace, and every change produces one report at every client. The latency of a report is the time from the
//! update call that made the change to the arrival of the report at its client. The update call is timed too: it is the model task's
//! serial path, which the reports share.
//!
//! The server polls for the changes every `poll` milliseconds, and the driver paces the changes `pace` milliseconds apart. With the
//! pace above the poll, one change gives one report; the report count is printed, so that a coalesced change shows up as a missing report.
//!
//! Run `cargo run --release --example contention_reports --features scl -- <clients> <updates> <poll ms> <pace ms>`. The numbers are for
//! this machine and this workload only: loopback, one block per client, a data set of two members, no secured transport. They are not an
//! application performance claim.

use std::time::{Duration, Instant};

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::client_report::{decode_report, UrcbWrite};
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::updates::Change;
use igs_iec61850_mms_core::report::urcb::{OptionalFields, TriggerOptions};
use igs_iec61850_mms_core::scl::build::{build, Capabilities};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::client::Event;
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::ServerConfig;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");
const STATUS: &str = "LD1/GGIO1$ST$Ind1$stVal";

/// The sample with `blocks` unbuffered blocks in LLN0, RPT1 to RPTn, each on the data set DS1 (the status of Ind1 and the mode).
fn fixture(blocks: usize) -> String {
    let anchor = "            </DataSet>\n          </LN0>";
    let mut text = String::new();
    for number in 1..=blocks {
        text.push_str(&format!(
            "            <ReportControl name=\"RPT{number}\" datSet=\"DS1\" rptID=\"RPT{number}\" confRev=\"1\" bufTime=\"0\" intgPd=\"0\" buffered=\"false\">\n              <OptFields seqNum=\"true\" reasonCode=\"true\" dataSet=\"true\" configRef=\"true\"/>\n            </ReportControl>\n"
        ));
    }
    SAMPLE.replacen(
        anchor,
        &format!("            </DataSet>\n{text}          </LN0>"),
        1,
    )
}

/// The served model of the fixture with `blocks` blocks, and the server configuration with the poll interval given.
fn served(blocks: usize) -> Result<ServedModel, Box<dyn std::error::Error>> {
    let text = fixture(blocks);
    let document =
        parse(text.as_bytes(), &Limits::default()).map_err(|error| format!("{error:?}"))?;
    validate(&document, "contention.cid").map_err(|error| format!("{error:?}"))?;
    let configuration = build(
        &document,
        "contention.cid",
        "IED1",
        &Capabilities::current(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "contention".to_string(),
        revision: "1".to_string(),
    };
    Ok(ServedModel::from_configuration(&configuration, identity)?)
}

/// Enables the block of a client: the data set, the options, the data-change trigger, and no buffer time or integrity period.
async fn enable(client: &IecClient, block: &str) -> Result<(), String> {
    let options = OptionalFields {
        seq_num: true,
        reason_code: true,
        data_set_name: true,
        conf_rev: true,
        ..Default::default()
    };
    let triggers = TriggerOptions {
        data_change: true,
        ..Default::default()
    };
    let steps = [
        UrcbWrite::Reserve(true),
        UrcbWrite::DataSet(Some("LLN0$DS1".to_string())),
        UrcbWrite::OptFields(options),
        UrcbWrite::TriggerOptions(triggers),
        UrcbWrite::BufTm(0),
        UrcbWrite::IntgPd(0),
        UrcbWrite::Enable(true),
    ];
    for step in steps {
        client
            .urcb_write("LD1", "LLN0", block, step)
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| format!("{error:?}"))?;
    }
    Ok(())
}

/// One client: enables its block, then records the arrival and the sequence number of each of its reports, until `count` reports
/// arrived or the time is up.
async fn subscriber(
    address: String,
    block: String,
    count: usize,
    time: Duration,
) -> Result<Vec<(Instant, u8)>, String> {
    let config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, mut events) = Client::connect(&address, config)
        .await
        .map_err(|error| error.to_string())?;
    let client = IecClient::new(client);
    enable(&client, &block).await?;
    let mut arrivals = Vec::with_capacity(count);
    let deadline = tokio::time::Instant::now().checked_add(time).unwrap_or_else(tokio::time::Instant::now);
    while arrivals.len() < count {
        match tokio::time::timeout_at(deadline, events.next()).await {
            Ok(Some(Event::Report(service))) => {
                if let Some(report) = decode_report(&service) {
                    if report.rpt_id == block {
                        arrivals.push((Instant::now(), report.sequence.unwrap_or(0)));
                    }
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    client.close().await.map_err(|error| error.to_string())?;
    Ok(arrivals)
}

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
    let clients: usize = arguments.next().map_or(Ok(4), |text| text.parse())?;
    let updates: usize = arguments.next().map_or(Ok(1000), |text| text.parse())?;
    let poll_ms: u64 = arguments.next().map_or(Ok(1), |text| text.parse())?;
    let pace_ms: u64 = arguments.next().map_or(Ok(5), |text| text.parse())?;

    let model = served(clients)?;
    let updater = model.updater();
    let config = ServerConfig {
        poll_interval: Duration::from_millis(poll_ms.max(1)),
        ..server_config(LinkConfig::default())
    };
    let server = Server::bind_model("127.0.0.1:0", config, model).await?;
    let address = server.local_address().to_string();

    // Every client subscribes before the first change, so that each change produces one report at each client.
    let mut tasks = Vec::with_capacity(clients);
    for number in 1..=clients {
        tasks.push(tokio::spawn(subscriber(
            address.clone(),
            format!("RPT{number}"),
            updates,
            Duration::from_secs(120),
        )));
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut call_times = Vec::with_capacity(updates);
    let mut update_durations = Vec::with_capacity(updates);
    let started = Instant::now();
    for step in 0..updates {
        let change = Change::new(STATUS, Data::Boolean(step % 2 == 0));
        let call = Instant::now();
        updater.update(&[change])?;
        update_durations.push(call.elapsed());
        call_times.push(call);
        tokio::time::sleep(Duration::from_millis(pace_ms)).await;
    }
    let elapsed = started.elapsed();

    let mut latencies = Vec::new();
    let mut received = 0usize;
    let mut out_of_sequence = 0usize;
    let mut failed = 0usize;
    for task in tasks {
        match task.await {
            Ok(Ok(arrivals)) => {
                received = received.saturating_add(arrivals.len());
                for (index, (arrival, sequence)) in arrivals.iter().enumerate() {
                    if let Some(call) = call_times.get(index) {
                        latencies.push(arrival.saturating_duration_since(*call));
                    }
                    if usize::from(*sequence) != index % 256 {
                        out_of_sequence = out_of_sequence.saturating_add(1);
                    }
                }
            }
            Ok(Err(error)) => {
                failed = failed.saturating_add(1);
                eprintln!("a subscriber failed: {error}");
            }
            Err(error) => {
                failed = failed.saturating_add(1);
                eprintln!("a subscriber task failed: {error}");
            }
        }
    }
    server.shutdown().await;

    latencies.sort();
    update_durations.sort();
    let expected = clients.saturating_mul(updates);
    println!("hardware: {}", hardware());
    println!(
        "workload: {clients} clients, one block each (RPT1 to RPT{clients}), data change, no buffer time, no integrity period, data set of two members; {updates} changes of {STATUS} every {pace_ms} ms; the server polls every {poll_ms} ms; loopback; no secured transport"
    );
    println!("subscribers failed: {failed}");
    println!(
        "reports received: {received} of {expected} expected; out of sequence: {out_of_sequence}"
    );
    println!("changes: {updates} in {:.3} s", elapsed.as_secs_f64());
    print_distribution("update call", &update_durations);
    print_distribution("report latency", &latencies);
    Ok(())
}
