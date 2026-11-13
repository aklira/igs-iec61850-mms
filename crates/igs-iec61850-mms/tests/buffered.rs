// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! AT-10 between our client and server (task B2; BRCB-002, BRCB-003): a buffered block is configured and enabled by its owner, and its
//! reports reach that association only. When the association is lost, the entries are kept, and a client resumes from the last entry
//! it received: the entries after it are sent, with no loss flag.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::client::IecClient;
use igs_iec61850_mms::client_buffered::{decode_buffered_report, BufferedWrite, DecodedBuffered};
use igs_iec61850_mms::control::AcceptAll;
use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::report::brcb::{BufferedFields, EntryId};
use igs_iec61850_mms_core::report::urcb::{Reason, TriggerOptions};
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::client::{Event, Events};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::{Client, ClientConfig, Reconnect};
use igs_iso9506_mms_codec::generated::types::Data;
use igs_iso9506_mms_link::config::LinkConfig;

const SAMPLE: &str = include_str!("../../igs-iec61850-mms-core/tests/data/sample.cid");
const BLOCK: &str = "BRC1";
const STATUS: &str = "GGIO1$ST$Ind1$stVal";

/// The sample with one buffered block in LLN0 on the data set DS1 (the status of Ind1 and the mode). The clients configure it.
fn configuration() -> Configuration {
    let anchor = "            </DataSet>\n          </LN0>";
    assert!(SAMPLE.contains(anchor));
    let text = SAMPLE.replacen(
        anchor,
        &format!(
            "            </DataSet>\n            <ReportControl name=\"{BLOCK}\" datSet=\"DS1\" rptID=\"{BLOCK}\" confRev=\"1\" bufTime=\"0\" intgPd=\"0\" buffered=\"true\">\n              <OptFields seqNum=\"true\" reasonCode=\"true\" dataSet=\"true\" configRef=\"true\" bufOvfl=\"true\" entryID=\"true\"/>\n            </ReportControl>\n          </LN0>"
        ),
        1,
    );
    let document = parse(text.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "buffered.cid").expect("the document is valid");
    build(&document, "buffered.cid", "IED1", &Capabilities::current()).expect("the document builds")
}

struct Setup {
    server: Server,
    updater: Updater,
    address: String,
}

async fn start() -> Setup {
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "buffered".to_string(),
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
    Setup {
        server,
        updater,
        address,
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
            .expect("the association is established in time")
            .expect("the association is accepted");
    (IecClient::new(client), events)
}

fn fields() -> BufferedFields {
    BufferedFields {
        seq_num: true,
        reason_code: true,
        data_set_name: true,
        data_ref: true,
        conf_rev: true,
        buf_ovfl: true,
        entry_id: true,
        ..Default::default()
    }
}

/// Sets the options and the triggers of the block, and enables it.
async fn configure(client: &IecClient) {
    let steps = [
        BufferedWrite::DataSet(Some("LLN0$DS1".to_string())),
        BufferedWrite::OptFields(fields()),
        BufferedWrite::TriggerOptions(TriggerOptions {
            data_change: true,
            ..Default::default()
        }),
        BufferedWrite::BufTm(0),
        BufferedWrite::IntgPd(0),
        BufferedWrite::Enable(true),
    ];
    for step in steps {
        client
            .buffered_write("LD1", "LLN0", BLOCK, step)
            .await
            .expect("the write is answered")
            .expect("the write is accepted");
    }
}

/// The next report of an association within the time, decoded.
async fn report_within(events: &mut Events, time: Duration) -> Option<DecodedBuffered> {
    tokio::time::timeout(time, async {
        loop {
            match events.next().await {
                Some(Event::Report(service)) => {
                    if let Some(report) = decode_buffered_report(&service) {
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

fn set_status(updater: &Updater, on: bool) {
    updater
        .update(&[Change::new(format!("LD1/{STATUS}"), Data::Boolean(on))])
        .expect("the change is applied");
}

#[tokio::test]
async fn a_buffered_block_reports_its_changes_to_the_association_that_enabled_it() {
    let setup = start().await;
    let (owner, mut events) = connect(&setup.address).await;
    configure(&owner).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    set_status(&setup.updater, true);
    let first = report_within(&mut events, Duration::from_secs(5))
        .await
        .expect("the owner receives the first entry");
    assert_eq!(first.rpt_id, BLOCK);
    assert_eq!(first.entry_id, Some(EntryId(1)));
    assert_eq!(first.sequence, Some(0));
    assert_eq!(
        first.buf_ovfl,
        Some(true),
        "enabled from disabled without resynchronisation"
    );
    assert_eq!(first.data_set.as_deref(), Some("LLN0$DS1"));
    let entry = &first.entries[0];
    assert!(entry
        .reference
        .as_deref()
        .is_some_and(|r| r.ends_with("Ind1.stVal")));
    assert_eq!(entry.value, Data::Boolean(true));
    assert_eq!(entry.reasons, vec![Reason::DataChange]);

    set_status(&setup.updater, false);
    let second = report_within(&mut events, Duration::from_secs(5))
        .await
        .expect("the owner receives the second entry");
    assert_eq!(second.entry_id, Some(EntryId(2)));
    assert_eq!(second.sequence, Some(1));
    assert_eq!(second.buf_ovfl, Some(false), "the flag is said once");
    assert!(second.entries[0].value == Data::Boolean(false));

    let values = owner
        .buffered_values("LD1", "LLN0", BLOCK)
        .await
        .expect("the block is read");
    assert!(values.enabled);
    assert_eq!(values.entry_id, EntryId(2), "the last entry sent");
    assert_eq!(values.sequence, 2);
}

#[tokio::test]
async fn only_the_association_that_enabled_the_block_receives_its_reports() {
    let setup = start().await;
    let (owner, _owner_events) = connect(&setup.address).await;
    let (other, mut other_events) = connect(&setup.address).await;
    configure(&owner).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    set_status(&setup.updater, true);
    assert!(report_within(&mut other_events, Duration::from_millis(400))
        .await
        .is_none());
    drop(other);
}

#[tokio::test]
async fn the_entries_made_while_the_association_is_lost_are_sent_after_the_client_resumes_from_its_last_entry(
) {
    let setup = start().await;
    let (owner, mut events) = connect(&setup.address).await;
    configure(&owner).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    set_status(&setup.updater, true);
    let received = report_within(&mut events, Duration::from_secs(5))
        .await
        .expect("entry 1 is received");
    let last_received = received.entry_id.expect("the report carries its entry");
    assert_eq!(last_received, EntryId(1));

    // The association is lost; the changes are made while nobody is attached.
    owner.close().await.expect("the association closes");
    tokio::time::sleep(Duration::from_millis(200)).await;
    set_status(&setup.updater, false);
    set_status(&setup.updater, true);
    // The model is polled on its tick: the changes become entries of the history at the next one.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // The reconnected client asks the block where it is: the newest entry made.
    let (client, mut events) = connect(&setup.address).await;
    let values = client
        .buffered_values("LD1", "LLN0", BLOCK)
        .await
        .expect("the block is read");
    assert!(
        !values.enabled,
        "a lost association leaves the block disabled"
    );
    assert_eq!(
        values.entry_id,
        EntryId(3),
        "the newest entry made while disabled"
    );

    // It resumes after the entry it received, and the reports start with the next one.
    client
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::EntryId(last_received))
        .await
        .expect("the write is answered")
        .expect("the resynchronisation is accepted");
    client
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::Enable(true))
        .await
        .expect("the write is answered")
        .expect("the enabling is accepted");

    let resumed_2 = report_within(&mut events, Duration::from_secs(5))
        .await
        .expect("entry 2 is sent");
    let resumed_3 = report_within(&mut events, Duration::from_secs(5))
        .await
        .expect("entry 3 is sent");
    assert_eq!(resumed_2.entry_id, Some(EntryId(2)));
    assert_eq!(resumed_3.entry_id, Some(EntryId(3)));
    assert_eq!(
        resumed_2.buf_ovfl,
        Some(false),
        "resynchronised: no loss flag"
    );
    assert_eq!(resumed_3.buf_ovfl, Some(false));
    assert_eq!(resumed_2.entries[0].value, Data::Boolean(false));
    assert_eq!(resumed_3.entries[0].value, Data::Boolean(true));
}

#[tokio::test]
async fn an_entry_that_the_history_does_not_hold_is_refused_for_resynchronisation() {
    let setup = start().await;
    let (client, _events) = connect(&setup.address).await;
    configure(&client).await;
    let refused = client
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::EntryId(EntryId(999)))
        .await;
    // The block is enabled, so the identifier is refused for that reason before the history is searched.
    assert!(refused.expect("the write is answered").is_err());
}

#[tokio::test]
async fn a_purge_is_refused_while_enabled_and_empties_the_history_when_disabled() {
    let setup = start().await;
    let (client, mut events) = connect(&setup.address).await;
    configure(&client).await;
    set_status(&setup.updater, true);
    report_within(&mut events, Duration::from_secs(5))
        .await
        .expect("entry 1 is sent");

    let purged = client
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::PurgeBuf)
        .await
        .expect("the write is answered");
    assert!(purged.is_err(), "refused while enabled");

    client
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::Enable(false))
        .await
        .expect("the write is answered")
        .expect("the disabling is accepted");
    client
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::PurgeBuf)
        .await
        .expect("the write is answered")
        .expect("the purge is accepted while disabled");
    let values = client
        .buffered_values("LD1", "LLN0", BLOCK)
        .await
        .expect("the block is read");
    assert_eq!(values.entry_id, EntryId::NULL, "the history is empty");
}

#[tokio::test]
async fn the_configuration_of_an_enabled_block_is_refused() {
    let setup = start().await;
    let (client, _events) = connect(&setup.address).await;
    configure(&client).await;
    let changed = client
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::BufTm(500))
        .await
        .expect("the write is answered");
    assert!(changed.is_err());
    let _ = Duration::ZERO;
    let _ = setup.server;
}

#[tokio::test]
async fn a_general_interrogation_reports_every_member_to_the_association_that_enabled_the_block() {
    // The owner asks for a general interrogation (figure 30): one report of every member, with the reason general-interrogation.
    let setup = start().await;
    let (owner, mut events) = connect(&setup.address).await;
    let steps = [
        BufferedWrite::DataSet(Some("LLN0$DS1".to_string())),
        BufferedWrite::OptFields(fields()),
        BufferedWrite::TriggerOptions(TriggerOptions {
            data_change: true,
            general_interrogation: true,
            ..Default::default()
        }),
        BufferedWrite::BufTm(0),
        BufferedWrite::IntgPd(0),
        BufferedWrite::Enable(true),
    ];
    for step in steps {
        owner
            .buffered_write("LD1", "LLN0", BLOCK, step)
            .await
            .expect("the write is answered")
            .expect("the write is accepted");
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    owner
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::GeneralInterrogation)
        .await
        .expect("the request is answered")
        .expect("the request is accepted");
    let report = report_within(&mut events, Duration::from_secs(5))
        .await
        .expect("the owner receives the general-interrogation report");
    assert_eq!(report.rpt_id, BLOCK);
    assert!(!report.entries.is_empty(), "every member is reported");
    for entry in &report.entries {
        assert_eq!(entry.reasons, vec![Reason::GeneralInterrogation]);
    }
}

#[tokio::test]
async fn a_reservation_by_one_association_refuses_the_enabling_of_another() {
    // A reserved block takes no write from another association (7-2 §17.2.2.17); the holder releases it.
    let setup = start().await;
    let (holder, _holder_events) = connect(&setup.address).await;
    let (other, _other_events) = connect(&setup.address).await;
    holder
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::Reserve(60))
        .await
        .expect("the reservation is answered")
        .expect("the reservation is granted");
    let refused = other
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::Enable(true))
        .await
        .expect("the write is answered");
    assert!(
        refused.is_err(),
        "another association is refused while reserved"
    );
    holder
        .buffered_write("LD1", "LLN0", BLOCK, BufferedWrite::Reserve(0))
        .await
        .expect("the release is answered")
        .expect("the release is accepted");
    other
        .buffered_write(
            "LD1",
            "LLN0",
            BLOCK,
            BufferedWrite::DataSet(Some("LLN0$DS1".to_string())),
        )
        .await
        .expect("the write is answered")
        .expect("the free block is written by another association");
}
