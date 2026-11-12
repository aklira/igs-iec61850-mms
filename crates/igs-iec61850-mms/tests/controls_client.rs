// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! AT-11 through the typed control API (task K3; ACSI-001, CTL-003, CTL-004): the four control models driven by the typed client, the
//! deferred outcomes of the application decoded by the client, the competing clients, the cancel, and an unknown object.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::client_control::{cause_name, termination, Command, Termination};
use igs_iec61850_mms::control::{AcceptAll, ControlError};
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms_core::acsi::{CheckConditions, Originator};
use igs_iec61850_mms_core::control::AddCause;
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::client::{Event, Events};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::{Data, UnconfirmedService};
use igs_iso9506_mms_link::config::LinkConfig;

const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");

/// The sample with the controllable data object Mod of LLN0 (its type gets the control attributes), configured with a control model,
/// the times (milliseconds) and the behaviour On.
fn controlled_sample(ctl_model: u8, oper_timeout_ms: u32) -> String {
    let with_types = SAMPLE.replacen(
        "      <DA name=\"stVal\" fc=\"ST\" bType=\"Enum\" type=\"ModKind\"/>",
        "      <DA name=\"stVal\" fc=\"ST\" bType=\"Enum\" type=\"ModKind\"/>\n      <DA name=\"ctlModel\" fc=\"CF\" bType=\"INT8\"/>\n      <DA name=\"sboTimeout\" fc=\"CF\" bType=\"INT32U\"/>\n      <DA name=\"operTimeout\" fc=\"CF\" bType=\"INT32U\"/>",
        1,
    );
    let anchor = "<LN0 lnClass=\"LLN0\" inst=\"\" lnType=\"LLN0T\">";
    assert!(with_types.contains(anchor));
    with_types.replacen(
        anchor,
        &format!(
            "{anchor}\n            <DOI name=\"Mod\">\n              <DAI name=\"ctlModel\"><Val>{ctl_model}</Val></DAI>\n              <DAI name=\"sboTimeout\"><Val>10000</Val></DAI>\n              <DAI name=\"operTimeout\"><Val>{oper_timeout_ms}</Val></DAI>\n            </DOI>\n            <DOI name=\"Beh\">\n              <DAI name=\"stVal\"><Val>1</Val></DAI>\n            </DOI>"
        ),
        1,
    )
}

fn configuration(text: &str) -> Configuration {
    let document = parse(text.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "control.cid").expect("the document is valid");
    build(&document, "control.cid", "IED1", &Capabilities::current()).expect("the document builds")
}

/// The reference of the controlled object of the sample.
const OBJECT: &str = "LLN0$CO$Mod";
const REFERENCE: &str = "LD1/LLN0$CO$Mod";

/// Starts the served sample, and connects the clients (in order), each with its events.
async fn start(
    text: &str,
    clients: usize,
) -> (
    Server,
    Vec<(IecClient, Events)>,
    igs_iec61850_mms::control::ControlHandle,
) {
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "controls".to_string(),
        revision: "1".to_string(),
    };
    let served = ServedModel::from_configuration(&configuration(text), identity)
        .expect("the model is served")
        .with_control_handler(AcceptAll);
    let handle = served.control_handle();
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
    (server, connected, handle)
}

fn command(client: u8, ctl_num: u8, value: bool) -> Command {
    Command {
        ctl_val: Data::Boolean(value),
        origin: Originator {
            category: 1,
            identity: vec![client],
        },
        ctl_num,
        test: false,
        check: CheckConditions {
            synchrocheck: false,
            interlock_check: false,
        },
    }
}

/// The next outcome of the association, within the time.
async fn outcome_within(events: &mut Events, time: Duration) -> Option<Termination> {
    tokio::time::timeout(time, async {
        loop {
            match events.next().await {
                Some(Event::Report(service)) => {
                    if let Some(outcome) = termination(&service) {
                        return Some(outcome);
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
async fn a_select_before_operate_object_is_selected_and_operated_by_the_typed_api() {
    let (server, mut clients, _handle) = start(&controlled_sample(2, 500), 1).await;
    let (client, _events) = clients.remove(0);
    let reference = client
        .select("LD1", OBJECT)
        .await
        .expect("the select is answered");
    assert_eq!(reference.as_deref(), Some(REFERENCE));
    assert!(client
        .operate("LD1", OBJECT, &command(1, 1, true))
        .await
        .expect("answered")
        .is_ok());
    client.close().await.expect("closed");
    server.shutdown().await;
}

#[tokio::test]
async fn competing_clients_are_refused_by_the_typed_api() {
    let (server, mut clients, _handle) = start(&controlled_sample(2, 500), 2).await;
    let (first, _) = clients.remove(0);
    let (second, _) = clients.remove(0);
    assert_eq!(
        first
            .select("LD1", OBJECT)
            .await
            .expect("answered")
            .as_deref(),
        Some(REFERENCE)
    );
    assert_eq!(
        second.select("LD1", OBJECT).await.expect("answered"),
        None,
        "the selection is held by the first client"
    );
    assert!(second
        .operate("LD1", OBJECT, &command(2, 1, true))
        .await
        .expect("answered")
        .is_err());
    assert!(first
        .operate("LD1", OBJECT, &command(1, 2, true))
        .await
        .expect("answered")
        .is_ok());
    server.shutdown().await;
}

#[tokio::test]
async fn a_cancel_frees_the_selection_for_another_client() {
    let (server, mut clients, _handle) = start(&controlled_sample(2, 500), 2).await;
    let (first, _) = clients.remove(0);
    let (second, _) = clients.remove(0);
    first.select("LD1", OBJECT).await.expect("answered");
    assert!(first
        .cancel("LD1", OBJECT, &command(1, 3, false))
        .await
        .expect("answered")
        .is_ok());
    assert_eq!(
        second
            .select("LD1", OBJECT)
            .await
            .expect("answered")
            .as_deref(),
        Some(REFERENCE)
    );
    server.shutdown().await;
}

#[tokio::test]
async fn a_deferred_positive_outcome_is_decoded_as_a_positive_termination() {
    let (server, mut clients, handle) = start(&controlled_sample(4, 5_000), 1).await;
    let (client, mut events) = clients.remove(0);
    assert!(client
        .select_with_value("LD1", OBJECT, &command(1, 7, true))
        .await
        .expect("answered")
        .is_ok());
    assert!(client
        .operate("LD1", OBJECT, &command(1, 7, true))
        .await
        .expect("answered")
        .is_ok());
    handle
        .complete(REFERENCE, Ok(()))
        .expect("the object is known");
    let outcome = outcome_within(&mut events, Duration::from_secs(10))
        .await
        .expect("the termination");
    assert_eq!(
        outcome,
        Termination::Positive {
            object: REFERENCE.to_string(),
            ctl_num: 7
        }
    );
    server.shutdown().await;
}

#[tokio::test]
async fn a_deferred_negative_outcome_is_decoded_with_its_cause() {
    let (server, mut clients, handle) = start(&controlled_sample(4, 5_000), 1).await;
    let (client, mut events) = clients.remove(0);
    client
        .select_with_value("LD1", OBJECT, &command(1, 8, true))
        .await
        .expect("answered")
        .expect("selected");
    client
        .operate("LD1", OBJECT, &command(1, 8, true))
        .await
        .expect("answered")
        .expect("operated");
    handle
        .complete(REFERENCE, Err(AddCause::BLOCKED_BY_INTERLOCKING))
        .expect("the object is known");
    match outcome_within(&mut events, Duration::from_secs(10))
        .await
        .expect("the termination")
    {
        Termination::Negative {
            object,
            ctl_num,
            cause,
            cause_name: name,
            ..
        } => {
            assert_eq!(object, REFERENCE);
            assert_eq!(ctl_num, 8);
            assert_eq!(name, Some("Blocked-by-interlocking"));
            assert_eq!(cause_name(cause), Some("Blocked-by-interlocking"));
        }
        other => panic!("a negative termination is expected: {other:?}"),
    }
    server.shutdown().await;
}

#[tokio::test]
async fn an_enhanced_operate_without_an_outcome_times_out_negatively() {
    let (server, mut clients, _handle) = start(&controlled_sample(3, 300), 1).await;
    let (client, mut events) = clients.remove(0);
    client
        .operate("LD1", OBJECT, &command(1, 9, true))
        .await
        .expect("answered")
        .expect("operated");
    match outcome_within(&mut events, Duration::from_secs(10))
        .await
        .expect("the termination")
    {
        Termination::Negative {
            cause_name: name, ..
        } => assert_eq!(name, Some("Time-limit-over")),
        other => panic!("a negative termination is expected: {other:?}"),
    }
    server.shutdown().await;
}

#[tokio::test]
async fn an_outcome_for_an_unknown_object_is_refused_by_the_handle() {
    let (server, _clients, handle) = start(&controlled_sample(4, 5_000), 0).await;
    assert_eq!(
        handle.complete("LD1/GGIO1$CO$Ind1", Ok(())),
        Err(ControlError::UnknownObject("LD1/GGIO1$CO$Ind1".to_string()))
    );
    server.shutdown().await;
}

#[test]
fn a_report_that_is_not_a_termination_is_not_decoded() {
    let report = UnconfirmedService::InformationReport(
        igs_iso9506_mms_codec::generated::types::InformationReport {
            variable_access_specification:
                igs_iso9506_mms_codec::generated::types::VariableAccessSpecification::VariableListName(
                    igs_iso9506_mms_codec::generated::types::ObjectName::VmdSpecific(
                        igs_iso9506_mms_codec::generated::types::Identifier("RPT".to_string()),
                    ),
                ),
            list_of_access_result: vec![],
        },
    );
    assert!(termination(&report).is_none());
}
