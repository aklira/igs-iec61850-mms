// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! AT-09 between our client and server (task D5; RPT-001, RPT-002, RPT-003): two clients with distinct subscriptions configure, enable and
//! disable their unbuffered blocks through the typed API, and receive the decoded reports of their own blocks: the changes, the general
//! interrogation, the integrity period, and the quality trigger.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::client_report::{decode_report, DecodedReport, UrcbWrite};
use igs_iec61850_mms::control::AcceptAll;
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::report::urcb::{OptionalFields, Reason, TriggerOptions};
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::client::{Event, Events};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_codec::schema::BitString;
use igs_iso9506_mms_link::config::LinkConfig;

const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");

/// The sample with unbuffered blocks in LLN0: RPT1 and RPT2 on the data set DS1 (the status of Ind1), and RPT3 on DS2 (the
/// quality of Ind1). The clients configure the blocks themselves.
fn fixture() -> String {
    let anchor = "            </DataSet>\n          </LN0>";
    assert!(SAMPLE.contains(anchor));
    let block = |name: &str, data_set: &str| {
        format!(
            "            <ReportControl name=\"{name}\" datSet=\"{data_set}\" rptID=\"{name}\" confRev=\"1\" bufTime=\"0\" intgPd=\"0\" buffered=\"false\">\n              <OptFields seqNum=\"true\" reasonCode=\"true\" dataSet=\"true\" configRef=\"true\"/>\n            </ReportControl>\n"
        )
    };
    let quality_set = "            <DataSet name=\"DS2\">\n              <FCDA ldInst=\"LD1\" prefix=\"\" lnClass=\"GGIO\" lnInst=\"1\" doName=\"Ind1\" daName=\"q\" fc=\"ST\"/>\n            </DataSet>\n";
    SAMPLE.replacen(
        anchor,
        &format!(
            "            </DataSet>\n{quality_set}{}{}{}          </LN0>",
            block("RPT1", "DS1"),
            block("RPT2", "DS1"),
            block("RPT3", "DS2")
        ),
        1,
    )
}

fn configuration() -> Configuration {
    let text = fixture();
    let document = parse(text.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "reporting.cid").expect("the document is valid");
    build(&document, "reporting.cid", "IED1", &Capabilities::current())
        .expect("the document builds")
}

struct Setup {
    server: Server,
    updater: Updater,
    clients: Vec<(IecClient, Events)>,
}

async fn start(clients: usize) -> Setup {
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "reporting".to_string(),
        revision: "1".to_string(),
    };
    let served = ServedModel::from_configuration(&configuration(), identity)
        .expect("the model is served")
        .with_control_handler(AcceptAll);
    let updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    let mut connected = Vec::new();
    for _ in 0..clients {
        let config = ClientConfig {
            reconnect: Reconnect::Never,
            ..client_config(LinkConfig::default())
        };
        let (client, events) =
            tokio::time::timeout(Duration::from_secs(10), Client::connect(&address, config))
                .await
                .expect("the association is established in time")
                .expect("the association is accepted");
        connected.push((IecClient::new(client), events));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    Setup {
        server,
        updater,
        clients: connected,
    }
}

/// The options that the tests report: the sequence number, the reasons, the data set and the configuration revision.
fn options() -> OptionalFields {
    OptionalFields {
        seq_num: true,
        reason_code: true,
        data_set_name: true,
        conf_rev: true,
        ..Default::default()
    }
}

/// Reserves the block, sets its data set, options and triggers, and enables it.
async fn configure(
    client: &IecClient,
    block: &str,
    data_set: &str,
    triggers: TriggerOptions,
    intg_ms: u32,
) {
    let steps = [
        UrcbWrite::Reserve(true),
        UrcbWrite::DataSet(Some(data_set.to_string())),
        UrcbWrite::OptFields(options()),
        UrcbWrite::TriggerOptions(triggers),
        UrcbWrite::BufTm(0),
        UrcbWrite::IntgPd(intg_ms),
        UrcbWrite::Enable(true),
    ];
    for step in steps {
        client
            .urcb_write("LD1", "LLN0", block, step)
            .await
            .expect("the write is answered")
            .expect("the write is accepted");
    }
}

fn data_change() -> TriggerOptions {
    TriggerOptions {
        data_change: true,
        ..Default::default()
    }
}

/// The next report of an association within the time, decoded, and which block it is for.
async fn report_within(events: &mut Events, time: Duration) -> Option<DecodedReport> {
    tokio::time::timeout(time, async {
        loop {
            match events.next().await {
                Some(Event::Report(service)) => {
                    if let Some(report) = decode_report(&service) {
                        return Some(report);
                    }
                }
                Some(_) => continue,
                None => return None,
            }
        }
    })
    .await
    .unwrap_or(None)
}

#[tokio::test]
async fn a_configured_block_reports_the_change_to_its_owner_in_the_layout_of_the_standard() {
    let mut setup = start(1).await;
    let (client, events) = setup.clients.remove(0);
    configure(
        &client,
        "RPT1",
        "LLN0$DS1",
        TriggerOptions {
            data_change: true,
            quality_change: true,
            ..Default::default()
        },
        0,
    )
    .await;
    setup
        .updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("the value is updated");
    let mut events = events;
    let report = report_within(&mut events, Duration::from_secs(10))
        .await
        .expect("the report");
    assert_eq!(report.rpt_id, "RPT1");
    assert_eq!(report.data_set.as_deref(), Some("LLN0$DS1"));
    assert_eq!(report.sequence, Some(0), "the first report of an enabling");
    assert_eq!(report.conf_rev, Some(1));
    assert_eq!(report.entries.len(), 1);
    let entry = &report.entries[0];
    assert_eq!(
        entry.index, 1,
        "the second variable of the data set (GGIO1 Ind1 stVal)"
    );
    assert_eq!(entry.value, Data::Boolean(true));
    assert_eq!(entry.reasons, [Reason::DataChange]);
    client.close().await.expect("closed");
    setup.server.shutdown().await;
}

#[tokio::test]
async fn two_clients_with_distinct_subscriptions_receive_only_their_own_reports() {
    let mut setup = start(2).await;
    let (first, mut first_events) = setup.clients.remove(0);
    let (second, mut second_events) = setup.clients.remove(0);
    configure(&first, "RPT1", "LLN0$DS1", data_change(), 0).await;
    configure(&second, "RPT2", "LLN0$DS1", data_change(), 0).await;
    setup
        .updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("updated");
    let for_first = report_within(&mut first_events, Duration::from_secs(10))
        .await
        .expect("the first report");
    let for_second = report_within(&mut second_events, Duration::from_secs(10))
        .await
        .expect("the second report");
    assert_eq!(for_first.rpt_id, "RPT1");
    assert_eq!(for_second.rpt_id, "RPT2");
    assert!(
        report_within(&mut first_events, Duration::from_millis(300))
            .await
            .is_none(),
        "nothing more for the first client"
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_disabled_block_reports_nothing_and_its_values_are_read() {
    let mut setup = start(1).await;
    let (client, mut events) = setup.clients.remove(0);
    configure(&client, "RPT1", "LLN0$DS1", data_change(), 0).await;
    client
        .urcb_write("LD1", "LLN0", "RPT1", UrcbWrite::Enable(false))
        .await
        .expect("answered")
        .expect("disabled");
    setup
        .updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("updated");
    assert!(report_within(&mut events, Duration::from_millis(400))
        .await
        .is_none());
    let values = client
        .urcb_values("LD1", "LLN0", "RPT1")
        .await
        .expect("the values are read");
    assert_eq!(values.rpt_id, "RPT1");
    assert!(!values.enabled);
    assert!(
        values.reserved,
        "the reservation stays with the client that holds it"
    );
    assert_eq!(values.data_set.as_deref(), Some("LLN0$DS1"));
    assert!(values.triggers.data_change && !values.triggers.quality_change);
    client.close().await.expect("closed");
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_general_interrogation_reports_every_member_with_its_reason() {
    let mut setup = start(1).await;
    let (client, mut events) = setup.clients.remove(0);
    configure(
        &client,
        "RPT1",
        "LLN0$DS1",
        TriggerOptions {
            general_interrogation: true,
            ..Default::default()
        },
        0,
    )
    .await;
    client
        .urcb_write("LD1", "LLN0", "RPT1", UrcbWrite::GeneralInterrogation)
        .await
        .expect("answered")
        .expect("accepted");
    let report = report_within(&mut events, Duration::from_secs(10))
        .await
        .expect("the report");
    assert_eq!(report.entries.len(), 2, "every member of the data set");
    for entry in &report.entries {
        assert_eq!(entry.reasons, [Reason::GeneralInterrogation]);
    }
    setup.server.shutdown().await;
}

#[tokio::test]
async fn the_integrity_period_repeats_a_report_of_every_member() {
    let mut setup = start(1).await;
    let (client, mut events) = setup.clients.remove(0);
    configure(
        &client,
        "RPT1",
        "LLN0$DS1",
        TriggerOptions {
            integrity: true,
            ..Default::default()
        },
        200,
    )
    .await;
    let mut integrity = Vec::new();
    while let Some(report) = report_within(&mut events, Duration::from_secs(2)).await {
        integrity.push(report);
        if integrity.len() == 2 {
            break;
        }
    }
    assert!(
        integrity.len() >= 2,
        "two integrity reports: {}",
        integrity.len()
    );
    for report in &integrity {
        assert_eq!(report.entries.len(), 2);
        assert!(report
            .entries
            .iter()
            .all(|entry| entry.reasons == [Reason::Integrity]));
    }
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_quality_trigger_alone_does_not_report_a_data_change() {
    let mut setup = start(1).await;
    let (client, mut events) = setup.clients.remove(0);
    configure(
        &client,
        "RPT3",
        "LLN0$DS2",
        TriggerOptions {
            quality_change: true,
            ..Default::default()
        },
        0,
    )
    .await;
    setup
        .updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("updated");
    assert!(
        report_within(&mut events, Duration::from_millis(400))
            .await
            .is_none(),
        "a data change does not report"
    );
    setup
        .updater
        .update(&[Change::new(
            "LD1/GGIO1$ST$Ind1$q",
            Data::BitString(BitString {
                unused: 3,
                bytes: vec![0x80, 0],
            }),
        )])
        .expect("updated");
    let report = report_within(&mut events, Duration::from_secs(10))
        .await
        .expect("the quality report");
    assert_eq!(report.rpt_id, "RPT3");
    assert_eq!(report.entries[0].reasons, [Reason::QualityChange]);
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_write_by_a_client_that_does_not_hold_the_block_is_refused() {
    let mut setup = start(2).await;
    let (holder, _) = setup.clients.remove(0);
    let (other, _) = setup.clients.remove(0);
    holder
        .urcb_write("LD1", "LLN0", "RPT1", UrcbWrite::Reserve(true))
        .await
        .expect("answered")
        .expect("reserved");
    assert!(other
        .urcb_write(
            "LD1",
            "LLN0",
            "RPT1",
            UrcbWrite::DataSet(Some("LLN0$DS1".to_string()))
        )
        .await
        .expect("answered")
        .is_err());
    assert!(other
        .urcb_write("LD1", "LLN0", "RPT1", UrcbWrite::Enable(true))
        .await
        .expect("answered")
        .is_err());
    setup.server.shutdown().await;
}

#[test]
fn a_report_that_is_not_on_the_list_of_reports_is_not_decoded() {
    use igs_iso9506_mms_codec::generated::types::{
        Identifier, InformationReport, ObjectName, UnconfirmedService, VariableAccessSpecification,
    };
    let report = UnconfirmedService::InformationReport(InformationReport {
        variable_access_specification: VariableAccessSpecification::VariableListName(
            ObjectName::VmdSpecific(Identifier("LastApplError".to_string())),
        ),
        list_of_access_result: vec![],
    });
    assert!(decode_report(&report).is_none());
    let _ = OptionalFields::default();
}
