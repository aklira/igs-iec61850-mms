// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The setting group control block over MMS (task O3; IEC 61850-7-2 §16.3; 8-1 §16.2): the SGCB is read as a structure, its
//! services are writes of its attributes and of the values with FC SE, and the editing is held by one association at a time.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms::model::{client_config, server_config, ServedModel};
use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use igs_iso9506_mms::server::Server;
use igs_iso9506_mms::vmd::Identity;
use igs_iso9506_mms::Client;
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, DataAccessError, Identifier, ObjectName, ObjectNameDomainSpecific,
    ReadRequest, VariableAccessSpecification, VariableAccessSpecificationListOfVariableItem,
    VariableSpecification, WriteRequest, WriteResponseItemItem,
};
use igs_iso9506_mms_link::config::LinkConfig;

const SETTINGS: &str = include_str!("data/settings.cid");
const TIMEOUT: Duration = Duration::from_secs(10);
const SGCB: &str = "LLN0$SP$SGCB";
const SE_SETTING: &str = "LLN0$SE$MltLev$setVal";
const SG_SETTING: &str = "LLN0$SG$MltLev$setVal";

fn configuration() -> Configuration {
    let document = parse(SETTINGS.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "settings.cid").expect("the document is valid");
    build(&document, "settings.cid", "IED1", &Capabilities::current()).expect("the document builds")
}

/// The MMS name of an item of the device LD1.
fn item(name: &str) -> VariableAccessSpecification {
    VariableAccessSpecification::ListOfVariable(vec![
        VariableAccessSpecificationListOfVariableItem {
            variable_specification: VariableSpecification::Name(ObjectName::DomainSpecific(
                ObjectNameDomainSpecific {
                    domain_id: Identifier("LD1".to_string()),
                    item_id: Identifier(name.to_string()),
                },
            )),
            alternate_access: None,
        },
    ])
}

async fn connect(address: &str) -> Client {
    let config = client_config(LinkConfig::default());
    tokio::time::timeout(TIMEOUT, Client::connect(address, config))
        .await
        .expect("the association is in time")
        .expect("the association is accepted")
        .0
}

async fn write(client: &Client, name: &str, value: Data) -> WriteResponseItemItem {
    let response = tokio::time::timeout(
        TIMEOUT,
        client.write(WriteRequest {
            variable_access_specification: item(name),
            list_of_data: vec![value],
        }),
    )
    .await
    .unwrap()
    .expect("the write is answered");
    response.0.into_iter().next().expect("one answer")
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

fn value(result: AccessResult) -> Data {
    match result {
        AccessResult::Success(data) => data,
        other => panic!("a read that succeeds, found {other:?}"),
    }
}

fn failure(answer: WriteResponseItemItem) -> DataAccessError {
    match answer {
        WriteResponseItemItem::Failure(error) => error,
        other => panic!("a write that fails, found {other:?}"),
    }
}

async fn start() -> (Server, String) {
    let identity = Identity {
        vendor: "igs".to_string(),
        model: "settings".to_string(),
        revision: "1".to_string(),
    };
    let served =
        ServedModel::from_configuration(&configuration(), identity).expect("the model is served");
    let server = Server::bind_model("127.0.0.1:0", server_config(LinkConfig::default()), served)
        .await
        .expect("the server binds");
    let address = server.local_address().to_string();
    (server, address)
}

#[tokio::test]
async fn the_sgcb_is_read_as_a_structure_of_its_attributes() {
    let (_server, address) = start().await;
    let client = connect(&address).await;
    let Data::Structure(parts) = value(read(&client, SGCB).await) else {
        panic!("the SGCB is a structure");
    };
    assert_eq!(parts.len(), 6);
    assert_eq!(parts[0], Data::Unsigned(2), "NumOfSG");
    assert_eq!(parts[1], Data::Unsigned(1), "ActSG");
    assert_eq!(parts[2], Data::Unsigned(0), "EditSG at power-up");
    assert_eq!(parts[3], Data::Boolean(false), "CnfEdit");
}

#[tokio::test]
async fn the_editing_is_held_by_one_association_and_its_values_are_confirmed_into_the_group() {
    let (_server, address) = start().await;
    let holder = connect(&address).await;
    let other = connect(&address).await;

    // SelectEditSG reserves the editing for the holder; the other association is refused.
    assert_eq!(
        write(&holder, "LLN0$SP$SGCB$EditSG", Data::Unsigned(2)).await,
        WriteResponseItemItem::Success
    );
    assert_eq!(
        failure(write(&other, "LLN0$SP$SGCB$EditSG", Data::Unsigned(1)).await),
        DataAccessError::TEMPORARILY_UNAVAILABLE
    );

    // SetEditSGValue writes the edited group: the holder's value is seen with FC SE, and the active one (FC SG) is unchanged.
    assert_eq!(
        write(&holder, SE_SETTING, Data::Boolean(true)).await,
        WriteResponseItemItem::Success
    );
    assert_eq!(value(read(&holder, SE_SETTING).await), Data::Boolean(true));
    assert_eq!(value(read(&holder, SG_SETTING).await), Data::Boolean(false));
    assert_eq!(
        failure(write(&other, SE_SETTING, Data::Boolean(false)).await),
        DataAccessError::OBJECT_ACCESS_DENIED,
        "the other association does not hold the editing"
    );

    // ConfirmEditSGValues overwrites group 2 and releases the editing.
    assert_eq!(
        write(&holder, "LLN0$SP$SGCB$CnfEdit", Data::Boolean(true)).await,
        WriteResponseItemItem::Success
    );
    let Data::Structure(parts) = value(read(&holder, SGCB).await) else {
        panic!("the SGCB is a structure");
    };
    assert_eq!(
        parts[2],
        Data::Unsigned(0),
        "EditSG is zero after the confirmation"
    );
    assert_eq!(
        parts[3],
        Data::Boolean(false),
        "CnfEdit is back to FALSE after the storage"
    );

    // SelectActiveSG takes group 2: its values are the active ones.
    assert_eq!(
        write(&holder, "LLN0$SP$SGCB$ActSG", Data::Unsigned(2)).await,
        WriteResponseItemItem::Success
    );
    assert_eq!(value(read(&holder, SG_SETTING).await), Data::Boolean(true));

    // A group that does not exist is refused.
    assert_eq!(
        failure(write(&holder, "LLN0$SP$SGCB$ActSG", Data::Unsigned(3)).await),
        DataAccessError::OBJECT_VALUE_INVALID
    );
}

#[tokio::test]
async fn the_editing_of_an_association_that_is_lost_is_released() {
    let (_server, address) = start().await;
    let holder = connect(&address).await;
    let other = connect(&address).await;
    assert_eq!(
        write(&holder, "LLN0$SP$SGCB$EditSG", Data::Unsigned(1)).await,
        WriteResponseItemItem::Success
    );
    assert_eq!(
        failure(write(&other, "LLN0$SP$SGCB$EditSG", Data::Unsigned(2)).await),
        DataAccessError::TEMPORARILY_UNAVAILABLE
    );
    holder.close().await.expect("the association closes");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        write(&other, "LLN0$SP$SGCB$EditSG", Data::Unsigned(2)).await,
        WriteResponseItemItem::Success
    );
}
