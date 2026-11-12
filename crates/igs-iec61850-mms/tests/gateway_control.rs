// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The gateway forwards the controls of its upstream model to a downstream server (task G2; GW-004): an operate of the upstream client reaches the
//! downstream control object, and the upstream termination is the downstream outcome, positive or negative with its cause.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::sync::Arc;
use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::client_control::{termination, Command, Termination};
use igs_iec61850_mms::gateway::{forward_outcomes, ForwardHandler};
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::updates::Change;
use igs_iec61850_mms_core::acsi::{CheckConditions, Originator};
use igs_iec61850_mms_core::scl::build::{build, Capabilities};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::client::{Event, Events};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::Data;
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

fn configured(text: &str) -> igs_iec61850_mms_core::scl::build::Configuration {
    let document = parse(text.as_bytes(), &Limits::default()).expect("parses");
    validate(&document, "control.cid").expect("valid");
    build(&document, "control.cid", "IED1", &Capabilities::current()).expect("builds")
}

fn identity(model: &str) -> Identity {
    Identity {
        vendor: "igs".to_string(),
        model: model.to_string(),
        revision: "1".to_string(),
    }
}

async fn connect(address: &str) -> (IecClient, Events) {
    let config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, events) =
        tokio::time::timeout(Duration::from_secs(10), Client::connect(address, config))
            .await
            .expect("in time")
            .expect("accepted");
    (IecClient::new(client), events)
}

fn command(ctl_num: u8) -> Command {
    Command {
        ctl_val: Data::Integer(1),
        origin: Originator {
            category: 1,
            identity: vec![7],
        },
        ctl_num,
        test: false,
        check: CheckConditions {
            synchrocheck: false,
            interlock_check: false,
        },
    }
}

async fn next_termination(events: &mut Events, within: Duration) -> Option<Termination> {
    tokio::time::timeout(within, async {
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
async fn an_upstream_operate_is_forwarded_and_its_termination_is_the_downstream_outcome() {
    // The downstream: an enhanced direct control whose operate times out at 300 ms (no change of its status).
    let down_config = configured(&controlled_sample(3, 300));
    let down = ServedModel::from_configuration(&down_config, identity("down")).expect("served");
    let down_updater = down.updater();
    let down_server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), down)
        .await
        .expect("binds");
    let (downstream_client, downstream_events) =
        connect(&down_server.local_address().to_string()).await;
    let downstream_client = Arc::new(downstream_client);

    // The upstream: the same control, with a long operate time, whose commands the gateway forwards.
    let up_config = configured(&controlled_sample(3, 5_000));
    let served = ServedModel::from_configuration(&up_config, identity("up")).expect("served");
    let handle = served.control_handle();
    let served = served.with_control_handler(ForwardHandler {
        downstream: Arc::clone(&downstream_client),
        device: "LD1".to_string(),
        component: "LLN0$CO$Mod".to_string(),
        upstream: handle.clone(),
        reference: "LD1/LLN0$CO$Mod".to_string(),
        origin: Originator {
            category: 3,
            identity: b"gateway".to_vec(),
        },
    });
    let up_server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("binds");
    let (upstream_client, mut upstream_events) =
        connect(&up_server.local_address().to_string()).await;

    let pump = forward_outcomes(
        downstream_events,
        handle.clone(),
        "LD1/LLN0$CO$Mod".to_string(),
    );
    tokio::time::sleep(Duration::from_millis(100)).await;

    // A command that the downstream does not complete: the upstream termination is the downstream timeout.
    assert!(upstream_client
        .operate("LD1", "LLN0$CO$Mod", &command(1))
        .await
        .expect("answered")
        .is_ok());
    match next_termination(&mut upstream_events, Duration::from_secs(10))
        .await
        .expect("the upstream termination")
    {
        Termination::Negative {
            cause_name,
            ctl_num,
            ..
        } => {
            assert_eq!(
                cause_name,
                Some("Time-limit-over"),
                "the downstream cause is forwarded"
            );
            assert_eq!(ctl_num, 1);
        }
        other => panic!("a negative termination is expected: {other:?}"),
    }

    // A command that the downstream completes: its status changes, and the upstream termination is positive.
    assert!(upstream_client
        .operate("LD1", "LLN0$CO$Mod", &command(2))
        .await
        .expect("answered")
        .is_ok());
    tokio::time::sleep(Duration::from_millis(100)).await;
    down_updater
        .update(&[Change::new("LD1/LLN0$ST$Mod$stVal", Data::Integer(1))])
        .expect("the downstream status changes");
    match next_termination(&mut upstream_events, Duration::from_secs(10))
        .await
        .expect("the upstream termination")
    {
        Termination::Positive { object, ctl_num } => {
            assert_eq!(object, "LD1/LLN0$CO$Mod");
            assert_eq!(ctl_num, 2);
        }
        other => panic!("a positive termination is expected: {other:?}"),
    }

    pump.abort();
    upstream_client.close().await.expect("closed");
    up_server.shutdown().await;
    down_server.shutdown().await;
}
