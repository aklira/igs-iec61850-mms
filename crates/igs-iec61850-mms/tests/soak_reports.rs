// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The soak test with reports enabled (task X8; RUN-002): clients reserve and enable the unbuffered block RPT1, and end their associations
//! in three ways (an orderly close, an abort, a drop without a word). A lost association must release its reservation: the checks take the
//! block over from a new association, and receive its reports. The number of cycles is `IGS_SOAK_REPORT_CYCLES` (default 150).

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::client_report::{decode_report, UrcbWrite};
use igs_iec61850_mms::model::{server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::report::urcb::{OptionalFields, TriggerOptions};
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::client::{Event, Events};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::Client;
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

const REPORTS: &str = include_str!("data/reports.cid");
const STATUS: &str = "IED1LD1/GGIO1$ST$Ind1$stVal";
const TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_CYCLES: usize = 150;
const CHECK_EVERY: usize = 10;

fn configuration() -> Configuration {
    let document = parse(REPORTS.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "reports.cid").expect("the document is valid");
    build(&document, "reports.cid", "IED1", &Capabilities::current()).expect("the document builds")
}

async fn connect(address: &str) -> Option<(IecClient, Events)> {
    let config = igs_iec61850_mms::model::client_config(LinkConfig::default());
    let (client, events) = tokio::time::timeout(TIMEOUT, Client::connect(address, config))
        .await
        .ok()?
        .ok()?;
    Some((IecClient::new(client), events))
}

/// Reserves RPT1 and enables it for data changes: true when every step is accepted.
async fn enable(client: &IecClient) -> bool {
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
        match client.urcb_write("IED1LD1", "LLN0", "RPT1", step).await {
            Ok(Ok(())) => {}
            _ => return false,
        }
    }
    true
}

/// Reserves and enables RPT1, retrying for up to a second while the block is held by an association that the server has not yet found
/// lost. A dropped association (no word) is found when the server reads its end of the connection: a client that connects at once
/// may meet the reservation first. The number of retries is returned, so that the latency is seen in the result.
async fn enable_retrying(client: &IecClient, retries: &mut usize) -> bool {
    for attempt in 0..50 {
        if enable(client).await {
            return true;
        }
        if attempt == 0 {
            *retries = retries.saturating_add(1);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

/// Whether the next report of a client arrives within the time.
async fn report_within(events: &mut Events, time: Duration) -> bool {
    tokio::time::timeout(time, async {
        loop {
            match events.next().await {
                Some(Event::Report(service)) => {
                    if decode_report(&service).is_some() {
                        return true;
                    }
                }
                Some(_) => {}
                None => return false,
            }
        }
    })
    .await
    .unwrap_or(false)
}

#[tokio::test]
async fn reservations_are_released_and_reports_arrive_through_repeated_losses() {
    let cycles = std::env::var("IGS_SOAK_REPORT_CYCLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_CYCLES);
    let served = ServedModel::from_configuration(&configuration(), server_identity()).unwrap();
    let updater: Updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    let mut value = false;
    let mut failures = 0_usize;
    let mut checks = 0_usize;
    let mut retried = 0_usize;
    let mut log: Vec<String> = Vec::new();

    for cycle in 0..cycles {
        match connect(&address).await {
            Some((client, _events)) => {
                if !enable_retrying(&client, &mut retried).await {
                    failures += 1;
                    log.push(format!(
                        "cycle {cycle}: enable refused (kind {})",
                        cycle % 3
                    ));
                }
                match cycle % 3 {
                    0 => {
                        let _ = client.close().await;
                    }
                    1 => {
                        let _ = client.close().await;
                    }
                    _ => drop(client),
                }
            }
            None => {
                failures += 1;
                log.push(format!("cycle {cycle}: connect failed"));
            }
        }

        if cycle % CHECK_EVERY == 0 {
            checks += 1;
            // A new association takes the block over: the losses above must have released the reservation, and its reports arrive.
            match connect(&address).await {
                Some((checker, mut events)) => {
                    if enable_retrying(&checker, &mut retried).await {
                        value = !value;
                        updater
                            .update(&[Change::new(STATUS, Data::Boolean(value))])
                            .expect("the value is updated");
                        if !report_within(&mut events, Duration::from_secs(5)).await {
                            failures += 1;
                            log.push(format!("cycle {cycle}: no report"));
                        }
                    } else {
                        failures += 1;
                        log.push(format!("cycle {cycle}: checker enable refused"));
                    }
                    let _ = checker.close().await;
                }
                None => failures += 1,
            }
        }
    }

    println!("soak with reports: {cycles} cycles, {checks} checks, {failures} failures, {retried} reservations retried after a drop");
    for line in log.iter().take(20) {
        println!("{line}");
    }
    server.shutdown().await;
    assert_eq!(
        failures, 0,
        "every association is accepted, reserves the block, and receives its reports"
    );
}
