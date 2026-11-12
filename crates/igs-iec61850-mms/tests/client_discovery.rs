// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! AT-07, client side (task A6): the typed client discovers the served model, rebuilds the definitions, checks values
//! against them, and reads and writes with one result per item (ACSI-001, ACSI-002, MODEL-002).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

mod common;

use common::{all_names, start};
use igs_iec61850_mms::client::{ClientError, IecClient, ItemError};
use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::discovery::{Definition, Leaf, Size};
use igs_iec61850_mms_core::names::{MmsIdentifier, MmsName, Name, Scope};
use igs_iso9506_mms_codec::generated::types::Data;

const DEVICE: &str = "IED1LD1";
const MEASURING: &str = "MMXU1234567890123456";

fn item(text: &str) -> MmsName {
    MmsName::new(
        Scope::Domain(Name::new(DEVICE).expect("a device name")),
        MmsIdentifier::new(text).expect("an item identifier"),
    )
}

#[tokio::test]
async fn the_logical_devices_and_nodes_are_listed() {
    let (server, client) = start().await;
    let client = IecClient::new(client);
    assert_eq!(client.domains().await.unwrap(), vec![DEVICE.to_string()]);
    assert_eq!(
        client.logical_nodes(DEVICE).await.unwrap(),
        vec!["GGIO1".to_string(), MEASURING.to_string()]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn the_named_variables_are_the_listing_of_every_page() {
    let (server, client) = start().await;
    // The raw listing, over the pages of the protocol, with the client of the tests of A4.
    let raw = all_names(&client).await;
    let client = IecClient::new(client);
    let listed = client.named_variables(DEVICE).await.unwrap();
    assert_eq!(
        listed, raw,
        "the typed listing is the raw one, in its order"
    );
    let mut unique = listed.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), listed.len(), "no name is listed twice");
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn the_functional_groups_and_their_data_objects_are_discovered() {
    let (server, client) = start().await;
    let client = IecClient::new(client);
    let groups = client.functional_groups(DEVICE, "GGIO1").await.unwrap();
    let constraints: Vec<&str> = groups
        .iter()
        .map(|g| g.functional_constraint.as_str())
        .collect();
    assert_eq!(constraints, ["MX", "ST", "SV"]);
    let status = groups
        .iter()
        .find(|g| g.functional_constraint == "ST")
        .unwrap();
    assert!(status.objects.contains(&"Ind1".to_string()));
    assert!(status.objects.contains(&"Health".to_string()));
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_data_object_is_defined_by_its_attributes_and_accepts_its_values() {
    let (server, client) = start().await;
    let client = IecClient::new(client);
    let definition = client
        .definition(DEVICE, "GGIO1", "ST", "Ind1")
        .await
        .unwrap();
    let Definition::Structure(members) = definition.definition() else {
        panic!("a data object is a structure");
    };
    let attribute = |name: &str| {
        members
            .iter()
            .find(|member| member.name == name)
            .unwrap_or_else(|| panic!("{name} is an attribute of the object"))
            .definition
            .clone()
    };
    assert_eq!(attribute("stVal"), Definition::Leaf(Leaf::Boolean));
    assert_eq!(
        attribute("q"),
        Definition::Leaf(Leaf::BitString {
            size: Size {
                length: 13,
                variable: false
            }
        })
    );
    assert_eq!(attribute("t"), Definition::Leaf(Leaf::UtcTime));

    // A value of the object, read as one item, is accepted by its definition; a value of another shape is not.
    let value = client
        .read(&[item("GGIO1$ST$Ind1")])
        .await
        .unwrap()
        .remove(0)
        .expect("the object is read");
    assert!(definition.accepts(&value));
    assert!(!definition.accepts(&Data::Integer(1)));
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_measurement_is_defined_down_to_its_floating_point_value() {
    let (server, client) = start().await;
    let definition = IecClient::new(client)
        .definition(DEVICE, MEASURING, "MX", "TotW")
        .await
        .unwrap();
    let Definition::Structure(members) = definition.definition() else {
        panic!("a data object is a structure");
    };
    let mag = members
        .iter()
        .find(|member| member.name == "mag")
        .expect("the magnitude");
    let Definition::Structure(parts) = &mag.definition else {
        panic!("the magnitude is a structure of its components");
    };
    let float = parts
        .iter()
        .find(|part| part.name == "f")
        .expect("the float component");
    assert_eq!(
        float.definition,
        Definition::Leaf(Leaf::FloatingPoint {
            format_width: 32,
            exponent_width: 8
        })
    );
    server.shutdown().await;
}

#[tokio::test]
async fn a_read_answers_one_result_per_item_in_the_order_of_the_items() {
    let (server, client) = start().await;
    let client = IecClient::new(client);
    let results = client
        .read(&[
            item("GGIO1$ST$Ind1$stVal"),
            item("GGIO1$ST$Nope"),
            item("GGIO1$ST$Ind1$stVal"),
        ])
        .await
        .unwrap();
    assert_eq!(
        results,
        vec![
            Ok(Data::Boolean(false)),
            Err(ItemError::InstanceNotAvailable),
            Ok(Data::Boolean(false)),
        ]
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_write_answers_each_item_with_its_own_error() {
    let (server, client) = start().await;
    let client = IecClient::new(client);
    let defined: Vec<i64> = catalogue::enumeration("HealthKind")
        .expect("HealthKind is in the catalogue")
        .literals
        .iter()
        .map(|literal| i64::from(literal.value))
        .collect();
    let undefined = (-4i64..=3)
        .find(|value| !defined.contains(value))
        .expect("the range holds an undefined value");
    let results = client
        .write(&[
            (item("GGIO1$SV$Ind1$subVal"), Data::Boolean(true)),
            (item("GGIO1$ST$Ind1$stVal"), Data::Boolean(true)),
            (item("GGIO1$SV$Ind1$subVal"), Data::Integer(1)),
            (
                item("GGIO1$SV$Health$subVal"),
                Data::Integer(i128::from(undefined)),
            ),
        ])
        .await
        .unwrap();
    assert_eq!(
        results,
        vec![
            Ok(()),
            Err(ItemError::AccessViolation),
            Err(ItemError::TypeConflict),
            Err(ItemError::ParameterValueInconsistent),
        ]
    );
    // The first write took place: the next read sees it.
    let read = client.read(&[item("GGIO1$SV$Ind1$subVal")]).await.unwrap();
    assert_eq!(read, vec![Ok(Data::Boolean(true))]);
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_node_that_does_not_exist_is_an_error_of_the_definition() {
    let (server, client) = start().await;
    let result = IecClient::new(client)
        .definition(DEVICE, "NOPE", "ST", "Ind1")
        .await;
    assert!(matches!(result, Err(ClientError::Mms(_))));
    server.shutdown().await;
}

#[tokio::test]
async fn a_name_that_is_not_an_identifier_is_refused_before_the_request() {
    let (server, client) = start().await;
    let client = IecClient::new(client);
    assert!(matches!(
        client.named_variables("bad name").await,
        Err(ClientError::Name(_))
    ));
    client.close().await.unwrap();
    server.shutdown().await;
}
