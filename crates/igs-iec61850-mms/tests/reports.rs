// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The unbuffered reports of a configuration served on MMS (task D4; RPT-002, RPT-003): the owner receives the report of a change,
//! no other association does, a change that changes nothing or is outside the data set reports nothing, a write of another
//! association is refused, and the close of the owner releases the block.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::reports::{ReportError, Reports};
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::report::urcb::{Refusal, Write};
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::client::{Event, Events};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, Identifier, InformationReport, ObjectName, UnconfirmedService,
    VariableAccessSpecification,
};
use igs_iso9506_mms_codec::schema::BitString;
use igs_iso9506_mms_link::config::LinkConfig;

const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");

/// The sample, with an unbuffered report RPT1 of the data set DS1: data-change and quality-change, and the sequence number,
/// the reason for inclusion, the data set name and the configuration revision reported.
fn sample_with_report() -> String {
    let anchor = "            </DataSet>\n          </LN0>";
    assert!(SAMPLE.contains(anchor));
    SAMPLE.replacen(
        anchor,
        "            </DataSet>\n            <ReportControl name=\"RPT1\" datSet=\"DS1\" rptID=\"RPT1\" confRev=\"1\" bufTime=\"0\" intgPd=\"0\" buffered=\"false\">\n              <TrgOps dchg=\"true\" qchg=\"true\"/>\n              <OptFields seqNum=\"true\" reasonCode=\"true\" dataSet=\"true\" configRef=\"true\"/>\n            </ReportControl>\n          </LN0>",
        1,
    )
}

fn configuration() -> Configuration {
    let text = sample_with_report();
    let document = parse(text.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "sample.cid").expect("the document is valid");
    build(&document, "sample.cid", "IED1", &Capabilities::current()).expect("the document builds")
}

/// A served server, its handle to the reports and to the values, and a connected client per association. The clients are in
/// the order of their admission.
struct Setup {
    server: Server,
    reports: Reports,
    updater: Updater,
    clients: Vec<(Client, Events)>,
}

async fn start(clients: usize) -> Setup {
    let configuration = configuration();
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "reports".to_string(),
        revision: "1".to_string(),
    };
    let served =
        ServedModel::from_configuration(&configuration, identity).expect("the model is served");
    let reports = served.reports();
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
        let pair = tokio::time::timeout(Duration::from_secs(10), Client::connect(&address, config))
            .await
            .expect("the association is established in time")
            .expect("the association is accepted");
        connected.push(pair);
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    Setup {
        server,
        reports,
        updater,
        clients: connected,
    }
}

/// The next report of an association within the time, if one comes.
async fn report_within(events: &mut Events, time: Duration) -> Option<UnconfirmedService> {
    tokio::time::timeout(time, async {
        loop {
            match events.next().await {
                Some(Event::Report(service)) => return Some(service),
                Some(_) => continue,
                None => return None,
            }
        }
    })
    .await
    .unwrap_or(None)
}

fn results_of(service: &UnconfirmedService) -> &InformationReport {
    match service {
        UnconfirmedService::InformationReport(report) => report,
        other => panic!("expected an information report, got {other:?}"),
    }
}

#[tokio::test]
async fn the_owner_receives_the_report_of_a_change_and_no_other_association() {
    let mut setup = start(2).await;
    let peers = setup.reports.peers().expect("the peers");
    assert_eq!(peers.len(), 2, "both associations are admitted");
    let (owner, other) = (peers[0], peers[1]);
    setup
        .reports
        .request(owner, "RPT1", Write::Reserve(true))
        .expect("reserved");
    setup
        .reports
        .request(owner, "RPT1", Write::RptEna(true))
        .expect("enabled");
    assert_eq!(setup.reports.owner("RPT1").expect("a block"), Some(owner));

    setup
        .updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("the value is updated");

    let (_, owner_events) = &mut setup.clients[0];
    let report = report_within(owner_events, Duration::from_secs(10))
        .await
        .expect("the owner receives the report");
    let info = results_of(&report);
    assert!(matches!(
        &info.variable_access_specification,
        VariableAccessSpecification::VariableListName(ObjectName::VmdSpecific(Identifier(name))) if name == "RPT"
    ));
    let results = &info.list_of_access_result;
    assert!(matches!(&results[0], AccessResult::Success(Data::VisibleString(id)) if id == "RPT1"));
    // RptID, options, SeqNum, DatSet, ConfRev, inclusion, the value of the one included variable, its reason.
    assert_eq!(results.len(), 8, "{results:?}");
    assert!(
        matches!(&results[5], AccessResult::Success(Data::BitString(bits)) if bits.bytes == [0x40] && bits.unused == 6),
        "only the second member (GGIO1 Ind1) is included: {results:?}"
    );
    assert!(matches!(
        &results[6],
        AccessResult::Success(Data::Boolean(true))
    ));

    let (_, other_events) = &mut setup.clients[1];
    assert!(
        report_within(other_events, Duration::from_millis(300))
            .await
            .is_none(),
        "{other:?} gets nothing"
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn an_update_that_changes_nothing_reports_nothing() {
    let mut setup = start(1).await;
    let owner = setup.reports.peers().expect("the peers")[0];
    setup
        .reports
        .request(owner, "RPT1", Write::Reserve(true))
        .expect("reserved");
    setup
        .reports
        .request(owner, "RPT1", Write::RptEna(true))
        .expect("enabled");
    setup
        .updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("updated");
    let (_, events) = &mut setup.clients[0];
    assert!(
        report_within(events, Duration::from_secs(10))
            .await
            .is_some(),
        "the first change is reported"
    );
    setup
        .updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("updated");
    assert!(
        report_within(events, Duration::from_millis(400))
            .await
            .is_none(),
        "no change, no data-change report"
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_change_outside_the_data_set_reports_nothing() {
    let mut setup = start(1).await;
    let owner = setup.reports.peers().expect("the peers")[0];
    setup
        .reports
        .request(owner, "RPT1", Write::Reserve(true))
        .expect("reserved");
    setup
        .reports
        .request(owner, "RPT1", Write::RptEna(true))
        .expect("enabled");
    // The quality of Ind1 is not a member of DS1: its change is not a notice of the block.
    setup
        .updater
        .update(&[Change::new(
            "LD1/GGIO1$ST$Ind1$q",
            Data::BitString(quality_bits()),
        )])
        .expect("updated");
    let (_, events) = &mut setup.clients[0];
    assert!(report_within(events, Duration::from_millis(400))
        .await
        .is_none());
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_disabled_block_reports_nothing_and_its_refusals_are_explicit() {
    let mut setup = start(2).await;
    let peers = setup.reports.peers().expect("the peers");
    let (owner, other) = (peers[0], peers[1]);
    setup
        .updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("updated");
    let (_, owner_events) = &mut setup.clients[0];
    assert!(
        report_within(owner_events, Duration::from_millis(300))
            .await
            .is_none(),
        "disabled: nothing"
    );

    setup
        .reports
        .request(owner, "RPT1", Write::Reserve(true))
        .expect("reserved");
    assert_eq!(
        setup.reports.request(other, "RPT1", Write::Reserve(true)),
        Err(ReportError::Refused(Refusal::Reserved))
    );
    assert_eq!(
        setup.reports.request(other, "RPT1", Write::RptEna(true)),
        Err(ReportError::Refused(Refusal::NotReserved))
    );
    setup
        .reports
        .request(owner, "RPT1", Write::RptEna(true))
        .expect("enabled");
    assert_eq!(
        setup
            .reports
            .request(owner, "RPT1", Write::BufTm(Duration::from_millis(5))),
        Err(ReportError::Refused(Refusal::Enabled)),
        "configuration is refused while enabled"
    );
    assert_eq!(
        setup.reports.request(owner, "NOPE", Write::RptEna(false)),
        Err(ReportError::UnknownBlock("NOPE".to_string()))
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn the_close_of_the_owner_releases_and_disables_the_block() {
    let mut setup = start(2).await;
    let peers = setup.reports.peers().expect("the peers");
    let (owner, other) = (peers[0], peers[1]);
    setup
        .reports
        .request(owner, "RPT1", Write::Reserve(true))
        .expect("reserved");
    setup
        .reports
        .request(owner, "RPT1", Write::RptEna(true))
        .expect("enabled");
    let (owner_client, _) = setup.clients.remove(0);
    owner_client.close().await.expect("closed");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(setup.reports.owner("RPT1").expect("a block"), None);
    setup
        .reports
        .request(other, "RPT1", Write::Reserve(true))
        .expect("the block is released");
    setup.server.shutdown().await;
}

/// A quality value: thirteen bits, as the common data class of the status gives them (the unused bits are the last three).
fn quality_bits() -> BitString {
    BitString {
        unused: 3,
        bytes: vec![0, 0],
    }
}
