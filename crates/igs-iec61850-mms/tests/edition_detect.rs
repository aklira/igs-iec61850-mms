// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The edition of a served device, detected from its namespace string (task E2): the first and the second edition, and no edition when the
//! string is not one of the two (no check is relaxed for it).

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::client_control::{edition_of_namespace, Edition};
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms_core::scl::build::{build, Capabilities};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_link::config::LinkConfig;

const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");

/// The sample with the device namespace of the logical node LLN0: its NamPlt carries `ldNs`, an extension attribute of FC EX (the type LPL
/// gets it here), or the sample without it.
fn sample(ld_ns: Option<&str>) -> String {
    let text = SAMPLE.replacen(
        "<DOType id=\"LPL\" cdc=\"LPL\">\n      <DA name=\"vendor\" fc=\"DC\" bType=\"VisString255\"/>",
        "<DOType id=\"LPL\" cdc=\"LPL\">\n      <DA name=\"vendor\" fc=\"DC\" bType=\"VisString255\"/>\n      <DA name=\"ldNs\" fc=\"EX\" bType=\"VisString255\"/>",
        1,
    );
    match ld_ns {
        Some(value) => text.replacen(
            "<LN0 lnClass=\"LLN0\" inst=\"\" lnType=\"LLN0T\">",
            &format!(
                "<LN0 lnClass=\"LLN0\" inst=\"\" lnType=\"LLN0T\">\n            <DOI name=\"NamPlt\">\n              <DAI name=\"ldNs\"><Val>{value}</Val></DAI>\n            </DOI>"
            ),
            1,
        ),
        None => text,
    }
}

async fn detect(text: &str) -> Option<Edition> {
    let document = parse(text.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "edition.cid").expect("the document is valid");
    let configuration =
        build(&document, "edition.cid", "IED1", &Capabilities::current()).expect("builds");
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "edition".to_string(),
        revision: "1".to_string(),
    };
    let served = ServedModel::from_configuration(&configuration, identity).expect("served");
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("binds");
    let address = server.local_address().to_string();
    let config = ClientConfig {
        reconnect: Reconnect::Never,
        ..client_config(LinkConfig::default())
    };
    let (client, _events) =
        tokio::time::timeout(Duration::from_secs(10), Client::connect(&address, config))
            .await
            .expect("in time")
            .expect("accepted");
    let client = IecClient::new(client);
    let edition = client.detect_edition("LD1").await.expect("read");
    client.close().await.expect("closed");
    server.shutdown().await;
    edition
}

#[test]
fn the_namespace_of_the_first_and_of_the_second_edition_are_recognised() {
    assert_eq!(
        edition_of_namespace("IEC 61850-7-4:2003"),
        Some(Edition::One)
    );
    assert_eq!(
        edition_of_namespace("IEC 61850-7-4:2007"),
        Some(Edition::Two)
    );
    assert_eq!(
        edition_of_namespace("IEC 61850-7-4:2010"),
        None,
        "an unknown namespace is not assumed"
    );
    assert_eq!(edition_of_namespace(""), None);
}

#[tokio::test]
async fn a_device_that_declares_the_first_edition_is_detected_as_such() {
    assert_eq!(
        detect(&sample(Some("IEC 61850-7-4:2003"))).await,
        Some(Edition::One)
    );
}

#[tokio::test]
async fn a_device_that_declares_the_second_edition_is_detected_as_such() {
    assert_eq!(
        detect(&sample(Some("IEC 61850-7-4:2007"))).await,
        Some(Edition::Two)
    );
}

#[tokio::test]
async fn a_device_without_a_namespace_has_no_detected_edition() {
    assert_eq!(detect(&sample(None)).await, None);
}
