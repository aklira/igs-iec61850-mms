// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! A model built in code, served over MMS and read by the MMS client (tasks A3 and A4): the GetNameList and the
//! GetVariableAccessAttributes answers, and the one value behind a node and its flattened names.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

mod common;

use common::{all_names, domain_item, one, start};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, GetVariableAccessAttributesRequest, ReadRequest, TypeDescription,
};

#[tokio::test]
async fn get_name_list_lists_the_node_and_its_flattened_names() {
    let (server, client) = start().await;
    let names = all_names(&client).await;
    assert!(names.contains(&"GGIO1".to_string()));
    assert!(names.contains(&"GGIO1$ST$Ind1$stVal".to_string()));
    assert!(names.contains(&"GGIO1$MX$AnIn1$mag$f".to_string()));
    // Thirty-four characters: beyond the ISO limit, served by the IEC limit of the VMD.
    assert!(names.contains(&"MMXU1234567890123456$MX$TotW$mag$f".to_string()));
    assert!(names.iter().all(|name| name.chars().count() <= 64));
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        names, sorted,
        "the names are listed in the order of the model"
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn get_variable_access_attributes_gives_the_type_of_each_level() {
    let (server, client) = start().await;
    let leaf = client
        .get_variable_access_attributes(GetVariableAccessAttributesRequest::Name(domain_item(
            "GGIO1$ST$Ind1$stVal",
        )))
        .await
        .expect("the attributes are given");
    assert_eq!(leaf.type_description, TypeDescription::Boolean);

    let node = client
        .get_variable_access_attributes(GetVariableAccessAttributesRequest::Name(domain_item(
            "GGIO1",
        )))
        .await
        .expect("the attributes are given");
    let TypeDescription::Structure(structure) = node.type_description else {
        panic!("the node is a structure");
    };
    let functional_constraints: Vec<String> = structure
        .components
        .iter()
        .map(|component| component.component_name.as_ref().unwrap().0.clone())
        .collect();
    // Only the constraints that the node instantiates, in the order of Figure 4 of 8-1.
    assert_eq!(functional_constraints, ["MX", "ST", "SV"]);
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_node_and_its_flattened_name_show_the_same_value() {
    let (server, client) = start().await;
    let flattened = client
        .read(ReadRequest {
            specification_with_result: None,
            variable_access_specification: one(domain_item("GGIO1$ST$Ind1$stVal")),
        })
        .await
        .expect("the value is read");
    assert_eq!(
        flattened.list_of_access_result,
        vec![AccessResult::Success(Data::Boolean(false))]
    );
    // The same value, inside the node: its status, then its first object.
    let node = client
        .read(ReadRequest {
            specification_with_result: None,
            variable_access_specification: one(domain_item("GGIO1")),
        })
        .await
        .expect("the node is read");
    let AccessResult::Success(Data::Structure(functional)) = &node.list_of_access_result[0] else {
        panic!("the node is a structure of its functional constraints");
    };
    let Data::Structure(status) = &functional[1] else {
        panic!("the status is a structure");
    };
    let Data::Structure(ind1) = &status[1] else {
        panic!("Ind1 is a structure");
    };
    assert_eq!(ind1[0], Data::Boolean(false));
    client.close().await.unwrap();
    server.shutdown().await;
}
