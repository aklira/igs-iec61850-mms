// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The logs over MMS (task X2; IEC 61850-7-2 §17.3; IEC 61850-8-1 §17.3): a log control block of the SCL file records the changes of
//! its data set into its journal, which ReadJournal reads. The transition of LogEna at the start is the first entry; each change of a
//! member is an entry with the reference of the member, its value and its reason code.

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
use igs_iec61850_mms::updates::Change;
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::Client;
use igs_iso9506_mms_codec::generated::types::{
    Data, EntryContentEntryForm, GetNameListRequest, GetNameListRequestObjectScope, Identifier,
    JournalEntry, ObjectClass, ObjectClassBasicObjectClass, ObjectName, ObjectNameDomainSpecific,
    ReadJournalRequest,
};
use igs_iso9506_mms_link::config::LinkConfig;

const LOGS: &str = include_str!("data/logs.cid");
const TIMEOUT: Duration = Duration::from_secs(10);

fn configuration() -> Configuration {
    let document = parse(LOGS.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "logs.cid").expect("the document is valid");
    build(&document, "logs.cid", "IED1", &Capabilities::current()).expect("the document builds")
}

/// The journal C1 of LLN0 in the logical device LD1 (the journal of the `Log` of the file).
fn journal() -> ObjectName {
    ObjectName::DomainSpecific(ObjectNameDomainSpecific {
        domain_id: Identifier("LD1".to_string()),
        item_id: Identifier("LLN0$C1".to_string()),
    })
}

/// The variables of an entry of the data form, as (tag, value) pairs; an event entry has none.
fn variables(entry: &JournalEntry) -> Vec<(String, Data)> {
    match &entry.entry_content.entry_form {
        EntryContentEntryForm::Data(form) => form
            .list_of_variables
            .iter()
            .flatten()
            .map(|variable| {
                (
                    variable.variable_tag.0.clone(),
                    variable.value_specification.clone(),
                )
            })
            .collect(),
        EntryContentEntryForm::Annotation(_) => Vec::new(),
    }
}

#[tokio::test]
async fn the_changes_of_the_data_set_are_read_from_the_journal_with_their_reason() {
    let served = ServedModel::from_configuration(&configuration(), server_identity())
        .expect("the model is served");
    let updater = served.updater();
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    let (client, _events) = tokio::time::timeout(
        TIMEOUT,
        Client::connect(&address, client_config(LinkConfig::default())),
    )
    .await
    .expect("the association is in time")
    .expect("the association is accepted");

    updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(true))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;
    updater
        .update(&[Change::new("LD1/GGIO1$ST$Ind1$stVal", Data::Boolean(false))])
        .expect("the value is updated");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let page = tokio::time::timeout(
        TIMEOUT,
        client.read_journal(ReadJournalRequest {
            journal_name: journal(),
            range_start_specification: None,
            range_stop_specification: None,
            list_of_variables: None,
            entry_to_start_after: None,
        }),
    )
    .await
    .unwrap()
    .expect("the journal is read");
    let entries = page.list_of_journal_entry;

    // The first entry is the transition of LogEna at the start (an event), then one entry per change.
    assert_eq!(entries.len(), 3, "the transition and the two changes");
    assert!(
        variables(&entries[0]).is_empty(),
        "the transition is an event, not a list of variables"
    );
    for (entry, expected) in entries[1..].iter().zip([true, false]) {
        let found = variables(entry);
        assert_eq!(
            found.len(),
            2,
            "the value of the member and its reason code"
        );
        assert_eq!(
            found[0].0, "LD1/GGIO1$ST$Ind1$stVal",
            "the reference of the member"
        );
        assert_eq!(
            found[0].1,
            Data::Boolean(expected),
            "the value of the change"
        );
        assert_eq!(found[1].0, "ReasonCode", "the reason code of the entry");
    }
    let identifiers: Vec<u64> = entries
        .iter()
        .map(|entry| u64::from_be_bytes(entry.entry_identifier.clone().try_into().unwrap()))
        .collect();
    assert!(
        identifiers.windows(2).all(|pair| pair[0] < pair[1]),
        "identifiers increase"
    );
    assert!(
        identifiers.iter().all(|id| *id != 0),
        "no entry has the NULL identifier"
    );

    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn the_journals_of_a_domain_are_listed_by_get_name_list() {
    // G14 of the plan: the journal class of GetNameList, in the scope of a domain (8-1 §17.3.3.3.2: the journal of the node is
    // named LN$name in its logical device).
    let served = ServedModel::from_configuration(&configuration(), server_identity())
        .expect("the model is served");
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    let (client, _events) = tokio::time::timeout(
        TIMEOUT,
        Client::connect(&address, client_config(LinkConfig::default())),
    )
    .await
    .expect("the association is in time")
    .expect("the association is accepted");
    let page = tokio::time::timeout(
        TIMEOUT,
        client.get_name_list(GetNameListRequest {
            object_class: ObjectClass::BasicObjectClass(ObjectClassBasicObjectClass::JOURNAL),
            object_scope: GetNameListRequestObjectScope::DomainSpecific(Identifier(
                "LD1".to_string(),
            )),
            continue_after: None,
        }),
    )
    .await
    .unwrap()
    .expect("the journals are listed");
    let names: Vec<String> = page
        .list_of_identifier
        .iter()
        .map(|name| name.0.clone())
        .collect();
    assert_eq!(names, vec!["LLN0$C1".to_string()]);
    client.close().await.unwrap();
    server.shutdown().await;
}
