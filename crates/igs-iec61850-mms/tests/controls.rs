// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The MMS mapping of the controls (task K2; CTL-001, CTL-002, CTL-004): each control model is driven through its services on a
//! served configuration, with the answers of the writes and reads, the CommandTermination and the LastApplError.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::control::{AcceptAll, ControlHandler};
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::acsi::{CheckConditions, Originator};
use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::control::{AddCause, CheckKind};
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::client::{Event, Events};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, DataAccessError, Identifier, ObjectName, ObjectNameDomainSpecific,
    ReadRequest, UnconfirmedService, VariableAccessSpecification,
    VariableAccessSpecificationListOfVariableItem, VariableSpecification, WriteRequest,
    WriteResponseItemItem,
};
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

/// A started server with its updater and the clients, in the order of their connection.
struct Setup {
    server: Server,
    updater: Updater,
    clients: Vec<(Client, Events)>,
}

async fn start(text: &str, handler: impl ControlHandler, clients: usize) -> Setup {
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "controls".to_string(),
        revision: "1".to_string(),
    };
    let served = ServedModel::from_configuration(&configuration(text), identity)
        .expect("the model is served")
        .with_control_handler(handler);
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
        connected.push(
            tokio::time::timeout(Duration::from_secs(10), Client::connect(&address, config))
                .await
                .expect("the association is established in time")
                .expect("the association is accepted"),
        );
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    Setup {
        server,
        updater,
        clients: connected,
    }
}

/// The item name of a component of the control object Mod of LLN0.
fn component(name: &str) -> VariableAccessSpecification {
    VariableAccessSpecification::ListOfVariable(vec![
        VariableAccessSpecificationListOfVariableItem {
            variable_specification: VariableSpecification::Name(ObjectName::DomainSpecific(
                ObjectNameDomainSpecific {
                    domain_id: Identifier("LD1".to_string()),
                    item_id: Identifier(format!("LLN0$CO$Mod${name}")),
                },
            )),
            alternate_access: None,
        },
    ])
}

/// A control structure: the control value, the originator, the control number, a time, the test flag, and the checks.
fn operation(client: u8, ctl_num: u8, value: i128) -> Data {
    Data::Structure(vec![
        Data::Integer(value),
        Originator {
            category: 1,
            identity: vec![client],
        }
        .to_data()
        .expect("the originator"),
        Data::Unsigned(u128::from(ctl_num)),
        Data::OctetString(vec![0; 8]),
        Data::Boolean(false),
        CheckConditions {
            synchrocheck: false,
            interlock_check: false,
        }
        .to_data(),
    ])
}

async fn write(client: &Client, name: &str, value: Data) -> WriteResponseItemItem {
    let response = client
        .write(WriteRequest {
            variable_access_specification: component(name),
            list_of_data: vec![value],
        })
        .await
        .expect("the write is answered");
    response.0.into_iter().next().expect("one answer")
}

async fn select(client: &Client) -> AccessResult {
    let response = client
        .read(ReadRequest {
            specification_with_result: None,
            variable_access_specification: component("SBO"),
        })
        .await
        .expect("the read is answered");
    response
        .list_of_access_result
        .into_iter()
        .next()
        .expect("one answer")
}

/// The next report of an association within the time.
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

/// The result values of an information report.
fn results(service: &UnconfirmedService) -> Vec<Data> {
    match service {
        UnconfirmedService::InformationReport(report) => report
            .list_of_access_result
            .iter()
            .filter_map(|result| match result {
                AccessResult::Success(data) => Some(data.clone()),
                AccessResult::Failure(_) => None,
            })
            .collect(),
        other => panic!("expected an information report, got {other:?}"),
    }
}

fn last_appl_error_list(service: &UnconfirmedService) -> bool {
    matches!(
        service,
        UnconfirmedService::InformationReport(report)
            if matches!(&report.variable_access_specification,
                VariableAccessSpecification::VariableListName(ObjectName::VmdSpecific(Identifier(name)))
                    if name == "LastApplError")
    )
}

fn add_cause_of(value: &Data) -> i128 {
    match value {
        Data::Structure(components) => match components.get(4) {
            Some(Data::Integer(cause)) => *cause,
            other => panic!("the additional cause is an integer: {other:?}"),
        },
        other => panic!("a LastApplError is a structure: {other:?}"),
    }
}

fn cause_value(name: &str) -> i128 {
    i128::from(
        catalogue::enumeration("ControlServiceStatusKind")
            .and_then(|enumeration| {
                enumeration
                    .literals
                    .iter()
                    .find(|literal| literal.name == name)
            })
            .expect("the cause is in the catalogue")
            .value,
    )
}

#[tokio::test]
async fn a_direct_normal_operate_is_answered_without_a_selection() {
    let setup = start(&controlled_sample(1, 500), AcceptAll, 1).await;
    let (client, mut events) = setup.clients.into_iter().next().expect("a client");
    assert_eq!(
        write(&client, "Oper", operation(1, 1, 1)).await,
        WriteResponseItemItem::Success
    );
    assert!(
        report_within(&mut events, Duration::from_millis(300))
            .await
            .is_none(),
        "no termination for a direct normal control"
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_select_before_operate_refuses_an_operate_without_selection_and_reports_its_cause() {
    let setup = start(&controlled_sample(2, 500), AcceptAll, 1).await;
    let (client, mut events) = setup.clients.into_iter().next().expect("a client");
    let refused = write(&client, "Oper", operation(1, 7, 1)).await;
    assert_eq!(
        refused,
        WriteResponseItemItem::Failure(DataAccessError::OBJECT_ACCESS_DENIED)
    );
    let report = report_within(&mut events, Duration::from_secs(5))
        .await
        .expect("the LastApplError");
    assert!(last_appl_error_list(&report));
    let values = results(&report);
    assert_eq!(add_cause_of(&values[0]), cause_value("Object-not-selected"));
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_selection_belongs_to_one_client_and_a_select_of_another_is_null() {
    let setup = start(&controlled_sample(2, 500), AcceptAll, 2).await;
    let mut clients = setup.clients.into_iter();
    let (first, _) = clients.next().expect("the first client");
    let (second, _) = clients.next().expect("the second client");
    let selected = select(&first).await;
    assert!(
        matches!(&selected, AccessResult::Success(Data::VisibleString(reference)) if reference == "LD1/LLN0$CO$Mod"),
        "{selected:?}"
    );
    let refused = select(&second).await;
    assert!(
        matches!(&refused, AccessResult::Success(Data::VisibleString(reference)) if reference.is_empty()),
        "a select of another client is the NULL value: {refused:?}"
    );
    assert_eq!(
        write(&second, "Oper", operation(2, 1, 1)).await,
        WriteResponseItemItem::Failure(DataAccessError::TEMPORARILY_UNAVAILABLE)
    );
    assert_eq!(
        write(&first, "Oper", operation(1, 2, 1)).await,
        WriteResponseItemItem::Success
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn an_enhanced_select_with_value_then_operate_terminates_when_the_status_changes() {
    let setup = start(&controlled_sample(4, 5_000), AcceptAll, 1).await;
    let (client, mut events) = setup.clients.into_iter().next().expect("a client");
    assert_eq!(
        write(&client, "SBOw", operation(1, 3, 1)).await,
        WriteResponseItemItem::Success
    );
    assert_eq!(
        write(&client, "Oper", operation(1, 3, 1)).await,
        WriteResponseItemItem::Success
    );
    assert!(
        report_within(&mut events, Duration::from_millis(200))
            .await
            .is_none(),
        "the termination waits for the change"
    );
    setup
        .updater
        .update(&[Change::new("LD1/LLN0$ST$Mod$stVal", Data::Integer(1))])
        .expect("the status changes");
    let termination = report_within(&mut events, Duration::from_secs(10))
        .await
        .expect("the CommandTermination");
    assert!(
        !last_appl_error_list(&termination),
        "a positive termination is on Oper"
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn an_enhanced_operate_without_a_change_terminates_negatively_at_the_operate_timeout() {
    let setup = start(&controlled_sample(3, 300), AcceptAll, 1).await;
    let (client, mut events) = setup.clients.into_iter().next().expect("a client");
    assert_eq!(
        write(&client, "Oper", operation(1, 4, 1)).await,
        WriteResponseItemItem::Success
    );
    let termination = report_within(&mut events, Duration::from_secs(10))
        .await
        .expect("the negative termination");
    assert!(last_appl_error_list(&termination));
    assert_eq!(
        add_cause_of(&results(&termination)[0]),
        cause_value("Time-limit-over")
    );
    setup.server.shutdown().await;
}

/// A handler that refuses every check with an interlock.
struct Interlocked;

impl ControlHandler for Interlocked {
    fn check(
        &mut self,
        _object: &str,
        kind: CheckKind,
        _ctl_val: Option<&Data>,
    ) -> Result<(), AddCause> {
        match kind {
            CheckKind::Operative => Err(AddCause::BLOCKED_BY_INTERLOCKING),
            _ => Ok(()),
        }
    }
}

#[tokio::test]
async fn an_interlock_of_the_application_refuses_the_operate_with_its_cause() {
    let setup = start(&controlled_sample(1, 500), Interlocked, 1).await;
    let (client, mut events) = setup.clients.into_iter().next().expect("a client");
    assert_eq!(
        write(&client, "Oper", operation(1, 5, 1)).await,
        WriteResponseItemItem::Failure(DataAccessError::OBJECT_ACCESS_DENIED)
    );
    let report = report_within(&mut events, Duration::from_secs(5))
        .await
        .expect("the LastApplError");
    assert_eq!(
        add_cause_of(&results(&report)[0]),
        cause_value("Blocked-by-interlocking")
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn an_object_without_a_control_model_is_not_a_control_object() {
    let setup = start(&controlled_sample(0, 500), AcceptAll, 1).await;
    let (client, _events) = setup.clients.into_iter().next().expect("a client");
    assert!(matches!(
        write(&client, "Oper", operation(1, 1, 1)).await,
        WriteResponseItemItem::Failure(_)
    ));
    setup.server.shutdown().await;
}

#[tokio::test]
async fn a_cancel_releases_the_selection_for_another_client() {
    let setup = start(&controlled_sample(2, 500), AcceptAll, 2).await;
    let mut clients = setup.clients.into_iter();
    let (first, _) = clients.next().expect("the first client");
    let (second, _) = clients.next().expect("the second client");
    assert!(
        matches!(select(&first).await, AccessResult::Success(Data::VisibleString(r)) if !r.is_empty())
    );
    assert_eq!(
        write(&first, "Cancel", operation(1, 1, 1)).await,
        WriteResponseItemItem::Success
    );
    assert!(
        matches!(select(&second).await, AccessResult::Success(Data::VisibleString(r)) if !r.is_empty()),
        "the selection is free after the cancel"
    );
    setup.server.shutdown().await;
}

#[tokio::test]
async fn the_selection_is_released_when_its_client_closes() {
    let setup = start(&controlled_sample(2, 500), AcceptAll, 2).await;
    let mut clients = setup.clients.into_iter();
    let (first, _) = clients.next().expect("the first client");
    let (second, _) = clients.next().expect("the second client");
    assert!(
        matches!(select(&first).await, AccessResult::Success(Data::VisibleString(r)) if !r.is_empty())
    );
    first.close().await.expect("closed");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        matches!(select(&second).await, AccessResult::Success(Data::VisibleString(r)) if !r.is_empty()),
        "the selection of a closed association is released"
    );
    setup.server.shutdown().await;
}
