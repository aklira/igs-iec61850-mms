// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The data access of a served model (task A4; ACSI-002, MODEL-005): the writes that the namespace allows, the
//! refusals with the error of 8-1 Table 27, the enumerations that hold defined values only, the results per item,
//! and the data-object directory of 8-1 §13.4.3.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

mod common;

use common::{all_names, domain_item, one, start, write_of};
use igs_iec61850_mms_core::catalogue;
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, DataAccessError, GetVariableAccessAttributesRequest, ReadRequest,
    TypeDescription, WriteResponseItemItem,
};

fn results(
    response: igs_iso9506_mms_codec::generated::types::WriteResponse,
) -> Vec<WriteResponseItemItem> {
    response.0
}

#[tokio::test]
async fn a_substitution_is_written_and_read_back() {
    let (server, client) = start().await;
    let response = client
        .write(write_of(
            vec!["GGIO1$SV$Ind1$subVal"],
            vec![Data::Boolean(true)],
        ))
        .await
        .expect("the write is answered");
    assert_eq!(results(response), vec![WriteResponseItemItem::Success]);
    let read = client
        .read(ReadRequest {
            specification_with_result: None,
            variable_access_specification: one(domain_item("GGIO1$SV$Ind1$subVal")),
        })
        .await
        .expect("the value is read");
    assert_eq!(
        read.list_of_access_result,
        vec![AccessResult::Success(Data::Boolean(true))]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_status_is_read_only_and_refused_with_access_denied() {
    let (server, client) = start().await;
    let response = client
        .write(write_of(
            vec!["GGIO1$ST$Ind1$stVal"],
            vec![Data::Boolean(true)],
        ))
        .await
        .expect("the write is answered");
    assert_eq!(
        results(response),
        vec![WriteResponseItemItem::Failure(
            DataAccessError::OBJECT_ACCESS_DENIED
        )]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_node_is_never_written_as_a_whole() {
    let (server, client) = start().await;
    let node = client
        .read(ReadRequest {
            specification_with_result: None,
            variable_access_specification: one(domain_item("GGIO1")),
        })
        .await
        .expect("the node is read");
    let value = match node.list_of_access_result.into_iter().next() {
        Some(AccessResult::Success(value)) => value,
        other => panic!("the node is read: {other:?}"),
    };
    let response = client
        .write(write_of(vec!["GGIO1"], vec![value]))
        .await
        .expect("the write is answered");
    assert_eq!(
        results(response),
        vec![WriteResponseItemItem::Failure(
            DataAccessError::OBJECT_ACCESS_DENIED
        )]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_value_of_another_type_is_type_inconsistent() {
    let (server, client) = start().await;
    let response = client
        .write(write_of(
            vec!["GGIO1$SV$Ind1$subVal"],
            vec![Data::Integer(1)],
        ))
        .await
        .expect("the write is answered");
    assert_eq!(
        results(response),
        vec![WriteResponseItemItem::Failure(
            DataAccessError::TYPE_INCONSISTENT
        )]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn an_enumeration_holds_its_defined_values_only() {
    let (server, client) = start().await;
    let defined: Vec<i64> = catalogue::enumeration("HealthKind")
        .expect("HealthKind is in the catalogue")
        .literals
        .iter()
        .map(|literal| i64::from(literal.value))
        .collect();
    // A value the MMS integer holds (the range of the type, -4 to 3 for these literals), but not a defined one.
    let undefined = (-4i64..=3)
        .find(|value| !defined.contains(value))
        .expect("the range holds an undefined value");
    let response = client
        .write(write_of(
            vec!["GGIO1$SV$Health$subVal"],
            vec![Data::Integer(i128::from(undefined))],
        ))
        .await
        .expect("the write is answered");
    assert_eq!(
        results(response),
        vec![WriteResponseItemItem::Failure(
            DataAccessError::OBJECT_VALUE_INVALID
        )]
    );

    let first = *defined.first().expect("HealthKind has values");
    let response = client
        .write(write_of(
            vec!["GGIO1$SV$Health$subVal"],
            vec![Data::Integer(i128::from(first))],
        ))
        .await
        .expect("the write is answered");
    assert_eq!(results(response), vec![WriteResponseItemItem::Success]);
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn each_item_of_a_write_has_its_own_result() {
    let (server, client) = start().await;
    let response = client
        .write(write_of(
            vec!["GGIO1$ST$Ind1$stVal", "GGIO1$SV$Ind1$subVal"],
            vec![Data::Boolean(true), Data::Boolean(true)],
        ))
        .await
        .expect("the write is answered");
    assert_eq!(
        results(response),
        vec![
            WriteResponseItemItem::Failure(DataAccessError::OBJECT_ACCESS_DENIED),
            WriteResponseItemItem::Success,
        ]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn the_directory_of_a_data_object_lists_its_attributes() {
    let (server, client) = start().await;
    let object = client
        .get_variable_access_attributes(GetVariableAccessAttributesRequest::Name(domain_item(
            "GGIO1$ST$Ind1",
        )))
        .await
        .expect("the directory is given");
    let TypeDescription::Structure(structure) = object.type_description else {
        panic!("the data object is a structure");
    };
    let attributes: Vec<String> = structure
        .components
        .iter()
        .map(|component| component.component_name.as_ref().unwrap().0.clone())
        .collect();
    assert!(attributes.contains(&"stVal".to_string()));
    assert!(attributes.contains(&"q".to_string()));
    assert!(attributes.contains(&"t".to_string()));
    let names = all_names(&client).await;
    assert!(names.contains(&"GGIO1$SV$Health$subVal".to_string()));
    client.close().await.unwrap();
    server.shutdown().await;
}
