// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The log control block over MMS (task X2; IEC 61850-8-1 §17.3.3 and §17.3.4): its structure is read (GetLCBValues), its components
//! are written (SetLCBValues: LogEna stops and starts the recording; a log reference is refused while the block records), and the
//! journal is read from an entry (QueryLogAfter), so that a client that lost its association recovers the entries after the last one it had.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms::server_identity;
use igs_iec61850_mms::updates::{Change, Updater};
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::Client;
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, DataAccessError, EntryContentEntryForm, Identifier, JournalEntry,
    ObjectName, ObjectNameDomainSpecific, ReadJournalRequest,
    ReadJournalRequestRangeStartSpecification, ReadJournalRequestRangeStopSpecification,
    ReadRequest, VariableAccessSpecification, VariableAccessSpecificationListOfVariableItem,
    VariableSpecification, WriteRequest, WriteResponseItemItem,
};
use igs_iso9506_mms_link::config::LinkConfig;

const LOGS: &str = include_str!("data/logs.cid");
const TIMEOUT: Duration = Duration::from_secs(10);
const LCB: &str = "LLN0$LG$LCB1";
const STATUS: &str = "IED1LD1/GGIO1$ST$Ind1$stVal";

fn configuration() -> Configuration {
    let document = parse(LOGS.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "logs.cid").expect("the document is valid");
    build(&document, "logs.cid", "IED1", &Capabilities::current()).expect("the document builds")
}

/// The MMS name of an item of the logical device LD1, as a one-item access specification.
fn item(name: &str) -> VariableAccessSpecification {
    VariableAccessSpecification::ListOfVariable(vec![
        VariableAccessSpecificationListOfVariableItem {
            variable_specification: VariableSpecification::Name(ObjectName::DomainSpecific(
                ObjectNameDomainSpecific {
                    domain_id: Identifier("IED1LD1".to_string()),
                    item_id: Identifier(name.to_string()),
                },
            )),
            alternate_access: None,
        },
    ])
}

async fn connect(address: &str) -> Client {
    tokio::time::timeout(
        TIMEOUT,
        Client::connect(address, client_config(LinkConfig::default())),
    )
    .await
    .expect("the association is in time")
    .expect("the association is accepted")
    .0
}

async fn read(client: &Client, name: &str) -> AccessResult {
    let response = tokio::time::timeout(
        TIMEOUT,
        client.read(ReadRequest {
            specification_with_result: None,
            variable_access_specification: item(name),
        }),
    )
    .await
    .unwrap()
    .expect("the read is answered");
    response
        .list_of_access_result
        .into_iter()
        .next()
        .expect("one answer")
}

async fn write(client: &Client, name: &str, data: Data) -> WriteResponseItemItem {
    let response = tokio::time::timeout(
        TIMEOUT,
        client.write(WriteRequest {
            variable_access_specification: item(name),
            list_of_data: vec![data],
        }),
    )
    .await
    .unwrap()
    .expect("the write is answered");
    response.0.into_iter().next().expect("one answer")
}

async fn journal(client: &Client, after: Option<Vec<u8>>) -> Vec<JournalEntry> {
    let request = ReadJournalRequest {
        journal_name: ObjectName::DomainSpecific(ObjectNameDomainSpecific {
            domain_id: Identifier("IED1LD1".to_string()),
            item_id: Identifier("LLN0$C1".to_string()),
        }),
        range_start_specification: after
            .map(ReadJournalRequestRangeStartSpecification::StartingEntry),
        range_stop_specification: None,
        list_of_variables: None,
        entry_to_start_after: None,
    };
    tokio::time::timeout(TIMEOUT, client.read_journal(request))
        .await
        .unwrap()
        .expect("the journal is read")
        .list_of_journal_entry
}

fn identifier(entry: &JournalEntry) -> Vec<u8> {
    entry.entry_identifier.clone()
}

fn is_event(entry: &JournalEntry) -> bool {
    matches!(&entry.entry_content.entry_form, EntryContentEntryForm::Data(form) if form.event.is_some())
}

/// The components of the structure that an answer holds, as (name, value) pairs in their order.
fn structure(result: AccessResult) -> Vec<Data> {
    match result {
        AccessResult::Success(Data::Structure(parts)) => parts,
        other => panic!("the structure of the block, found {other:?}"),
    }
}

#[tokio::test]
async fn the_structure_of_the_block_is_read_with_its_references_and_its_state() {
    let served = ServedModel::from_configuration(&configuration(), server_identity()).unwrap();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let client = connect(&server.local_address().to_string()).await;

    let parts = structure(read(&client, LCB).await);
    assert_eq!(parts.len(), 9, "the nine components of table 75");
    assert_eq!(
        parts[0],
        Data::Boolean(true),
        "LogEna: the file enables the block"
    );
    assert_eq!(
        parts[1],
        Data::VisibleString("IED1LD1/LLN0$C1".to_string()),
        "LogRef: the journal"
    );
    assert_eq!(
        parts[2],
        Data::VisibleString("IED1LD1/LLN0$DS1".to_string()),
        "DatSet: the data set"
    );
    assert_eq!(parts[8], Data::Unsigned(0), "IntgPd: no integrity period");
    // The component names in MMS: OldEnt and NewEnt, as table 75 maps them.
    match read(&client, "LLN0$LG$LCB1$LogEna").await {
        AccessResult::Success(Data::Boolean(true)) => {}
        other => panic!("LogEna is read as a component, found {other:?}"),
    }
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn logena_stops_and_starts_the_recording_and_a_reference_is_refused_while_recording() {
    let served = ServedModel::from_configuration(&configuration(), server_identity()).unwrap();
    let updater: Updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let client = connect(&server.local_address().to_string()).await;

    // While the block records, its log reference cannot change (8-1 Table 75: May not be written if LogEna=TRUE).
    assert_eq!(
        write(
            &client,
            "LLN0$LG$LCB1$LogRef",
            Data::VisibleString("IED1LD1/LLN0$C1".to_string())
        )
        .await,
        WriteResponseItemItem::Failure(DataAccessError::OBJECT_ACCESS_DENIED)
    );

    updater
        .update(&[Change::new(STATUS, Data::Boolean(true))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        write(&client, "LLN0$LG$LCB1$LogEna", Data::Boolean(false)).await,
        WriteResponseItemItem::Success
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    updater
        .update(&[Change::new(STATUS, Data::Boolean(false))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // The journal holds the transition at the start, the change while recording, and the transition that stopped it: not the last change.
    let entries = journal(&client, None).await;
    assert_eq!(entries.len(), 3, "transition, change, transition");
    assert!(is_event(&entries[0]) && is_event(&entries[2]) && !is_event(&entries[1]));
    match read(&client, "LLN0$LG$LCB1$LogEna").await {
        AccessResult::Success(Data::Boolean(false)) => {}
        other => panic!("LogEna is false, found {other:?}"),
    }
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_client_that_lost_its_association_recovers_the_entries_after_its_last_one() {
    let served = ServedModel::from_configuration(&configuration(), server_identity()).unwrap();
    let updater: Updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();

    updater
        .update(&[Change::new(STATUS, Data::Boolean(true))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let first = connect(&address).await;
    let seen = journal(&first, None).await;
    let last = identifier(seen.last().expect("entries were recorded"));
    first.abort().await.expect("the association is aborted");

    // The association is gone; the changes go on being recorded.
    updater
        .update(&[Change::new(STATUS, Data::Boolean(false))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;
    updater
        .update(&[Change::new(STATUS, Data::Boolean(true))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let second = connect(&address).await;
    let recovered = journal(&second, Some(last.clone())).await;
    // The entry the client had is the start of the answer (QueryLogAfter from it): the entries after it are the two changes.
    let after: Vec<&JournalEntry> = recovered
        .iter()
        .filter(|entry| identifier(entry) != last)
        .collect();
    assert_eq!(
        after.len(),
        2,
        "the two changes made while the client was away"
    );
    assert!(
        after.iter().all(|entry| identifier(entry) > last),
        "identifiers increase past the last seen"
    );
    second.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_journal_is_queried_by_time_from_and_until_a_time() {
    // QueryLogByTime (8-1 Table 79): the entries from a time of day, and up to one, both ends included.
    let served = ServedModel::from_configuration(&configuration(), server_identity()).unwrap();
    let updater: Updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let client = connect(&server.local_address().to_string()).await;
    updater
        .update(&[Change::new(STATUS, Data::Boolean(true))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let all = journal(&client, None).await;
    assert!(all.len() >= 2, "the transition and the change are recorded");
    let first_time = all[0].entry_content.occurrence_time.clone();
    let query = |start: Option<igs_iso9506_mms_codec::generated::types::TimeOfDay>,
                 stop: Option<igs_iso9506_mms_codec::generated::types::TimeOfDay>| {
        ReadJournalRequest {
            journal_name: ObjectName::DomainSpecific(ObjectNameDomainSpecific {
                domain_id: Identifier("IED1LD1".to_string()),
                item_id: Identifier("LLN0$C1".to_string()),
            }),
            range_start_specification: start
                .map(ReadJournalRequestRangeStartSpecification::StartingTime),
            range_stop_specification: stop
                .map(ReadJournalRequestRangeStopSpecification::EndingTime),
            list_of_variables: None,
            entry_to_start_after: None,
        }
    };
    let from = client
        .read_journal(query(Some(first_time.clone()), None))
        .await
        .expect("the journal is queried")
        .list_of_journal_entry;
    assert_eq!(
        from.len(),
        all.len(),
        "every entry is at or after the first one's time"
    );
    let until = client
        .read_journal(query(None, Some(first_time)))
        .await
        .expect("the journal is queried")
        .list_of_journal_entry;
    assert!(
        !until.is_empty(),
        "the entries up to the first one's time include it"
    );
    assert!(until.len() <= all.len());
    client.close().await.unwrap();
    server.shutdown().await;
}
