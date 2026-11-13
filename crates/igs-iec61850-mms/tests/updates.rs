// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The application's updates of a served model (task A5; MODEL-006, AT-17): an update is seen by the reads as one
//! state of the node, a refused update changes nothing, and a reader never sees a value without the timestamp of the
//! same update.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

mod common;

use common::{domain_item, one, start_with_updater};
use igs_iec61850_mms::updates::{Change, UpdateError};
use igs_iec61850_mms_core::acsi::{
    float32_to_data, timestamp_from_data, timestamp_to_data, Quality,
};
use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::model::layout::value_at;
use igs_iso9506_mms::Client;
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, GetVariableAccessAttributesRequest, ReadRequest, TypeDescription,
    TypeSpecification,
};
use igs_iso9506_mms_codec::mms::{TimeAccuracy, TimeQuality, UtcTimestamp};

const STATUS: &str = "IED1LD1/GGIO1$ST$Ind1$stVal";
const MEASURE: &str = "IED1LD1/MMXU1234567890123456$MX$TotW$mag$f";
const TOTAL_TIME: &str = "IED1LD1/MMXU1234567890123456$MX$TotW$t";
const NODE: &str = "MMXU1234567890123456";

/// A timestamp of `seconds`, with the quality of a synchronised clock.
fn stamp(seconds: u32) -> Data {
    let quality = TimeQuality {
        leap_second_known: true,
        clock_failure: false,
        clock_not_synchronized: false,
        accuracy: TimeAccuracy::Bits(20),
    };
    timestamp_to_data(UtcTimestamp::new(seconds, 0, quality).expect("the timestamp is valid"))
}

/// The value of an attribute of an object, read in one request: the object is one item, so the value is one state.
async fn attribute_of(client: &Client, object: &str, path: &[&str]) -> Data {
    let object_type = client
        .get_variable_access_attributes(GetVariableAccessAttributesRequest::Name(domain_item(
            object,
        )))
        .await
        .expect("the type is given")
        .type_description;
    let indexes = index_path(&object_type, path);
    let value = read_object(client, object).await;
    value_at(&value, &indexes)
        .expect("the attribute is in the object")
        .clone()
}

async fn read_object(client: &Client, object: &str) -> Data {
    let response = client
        .read(ReadRequest {
            specification_with_result: None,
            variable_access_specification: one(domain_item(object)),
        })
        .await
        .expect("the object is read");
    match response.list_of_access_result.into_iter().next() {
        Some(AccessResult::Success(value)) => value,
        other => panic!("the object is read: {other:?}"),
    }
}

/// The indexes of the components named by `names`, from the type of an object.
fn index_path(ty: &TypeDescription, names: &[&str]) -> Vec<usize> {
    let mut indexes = Vec::new();
    let mut current = ty;
    for name in names {
        let TypeDescription::Structure(structure) = current else {
            panic!("{name} is looked up in a structure");
        };
        let index = structure
            .components
            .iter()
            .position(|component| {
                component.component_name.as_ref().map(|n| n.0.as_str()) == Some(*name)
            })
            .unwrap_or_else(|| panic!("the component {name} is in the type"));
        indexes.push(index);
        current = match &structure.components[index].component_type {
            TypeSpecification::TypeDescription(inner) => inner,
            TypeSpecification::TypeName(_) => panic!("the type is named, not described"),
        };
    }
    indexes
}

#[tokio::test]
async fn an_update_of_value_quality_and_time_is_read_as_one_state() {
    let (server, client, updater) = start_with_updater().await;
    let revision = updater
        .update(&[
            Change::new(STATUS, Data::Boolean(true)),
            Change::new("IED1LD1/GGIO1$ST$Ind1$q", Quality::good().to_data()),
            Change::new("IED1LD1/GGIO1$ST$Ind1$t", stamp(1_700_000_000)),
        ])
        .expect("the update is applied");
    assert_eq!(revision.get(), 1);

    assert_eq!(
        attribute_of(&client, "GGIO1$ST$Ind1", &["stVal"]).await,
        Data::Boolean(true)
    );
    assert_eq!(
        attribute_of(&client, "GGIO1$ST$Ind1", &["q"]).await,
        Quality::good().to_data()
    );
    let time = attribute_of(&client, "GGIO1$ST$Ind1", &["t"]).await;
    assert_eq!(
        timestamp_from_data(&time).expect("a timestamp").seconds,
        1_700_000_000
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_refused_update_changes_nothing_and_takes_no_revision() {
    let (server, client, updater) = start_with_updater().await;
    updater
        .update(&[Change::new(STATUS, Data::Boolean(true))])
        .expect("the first update is applied");

    // The second change names no attribute: the first one is not applied either.
    let refused = updater.update(&[
        Change::new(STATUS, Data::Boolean(false)),
        Change::new("IED1LD1/GGIO1$ST$Ind1$nope", Data::Boolean(false)),
    ]);
    assert_eq!(
        refused,
        Err(UpdateError::UnknownName(
            "IED1LD1/GGIO1$ST$Ind1$nope".to_string()
        ))
    );
    assert_eq!(
        attribute_of(&client, "GGIO1$ST$Ind1", &["stVal"]).await,
        Data::Boolean(true)
    );
    // The next update takes the revision that follows the last applied one.
    let next = updater
        .update(&[Change::new(STATUS, Data::Boolean(false))])
        .expect("the next update is applied");
    assert_eq!(next.get(), 2);
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn an_update_holds_the_types_the_namespace_defines_and_the_changes_of_one_node() {
    let (server, client, updater) = start_with_updater().await;
    // A status is a boolean: an integer is refused.
    assert_eq!(
        updater.update(&[Change::new(STATUS, Data::Integer(1))]),
        Err(UpdateError::TypeMismatch(STATUS.to_string()))
    );
    // The enumeration of the health status: a value the MMS integer holds, but the namespace does not define.
    let defined: Vec<i64> = catalogue::enumeration("HealthKind")
        .expect("HealthKind is in the catalogue")
        .literals
        .iter()
        .map(|literal| i64::from(literal.value))
        .collect();
    let undefined = (-4i64..=3)
        .find(|value| !defined.contains(value))
        .expect("the range holds an undefined value");
    let health = "IED1LD1/GGIO1$SV$Health$subVal";
    assert_eq!(
        updater.update(&[Change::new(health, Data::Integer(i128::from(undefined)))]),
        Err(UpdateError::UndefinedValue(health.to_string()))
    );
    // Changes of two nodes in one update are refused.
    assert_eq!(
        updater.update(&[
            Change::new(STATUS, Data::Boolean(true)),
            Change::new(MEASURE, float32_to_data(1.0)),
        ]),
        Err(UpdateError::MixedNodes)
    );
    assert_eq!(updater.update(&[]), Err(UpdateError::Empty));
    assert_eq!(
        attribute_of(&client, "GGIO1$ST$Ind1", &["stVal"]).await,
        Data::Boolean(false),
        "none of the refused updates was applied"
    );
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn a_reader_never_sees_a_value_without_the_time_of_its_update() {
    let (server, client, updater) = start_with_updater().await;
    let writer = tokio::spawn(async move {
        for seconds in 1..=300u32 {
            // The measurement and its time change in one update: a reader sees both of one update, or neither.
            updater
                .update(&[
                    Change::new(MEASURE, float32_to_data(seconds as f32)),
                    Change::new(TOTAL_TIME, stamp(seconds)),
                ])
                .expect("the update is applied");
            tokio::task::yield_now().await;
        }
    });

    let object_type = client
        .get_variable_access_attributes(GetVariableAccessAttributesRequest::Name(domain_item(NODE)))
        .await
        .expect("the type is given")
        .type_description;
    let measure_path = index_path(&object_type, &["MX", "TotW", "mag", "f"]);
    let time_path = index_path(&object_type, &["MX", "TotW", "t"]);
    for _ in 0..300 {
        // The whole node is one item: its measurement and its time come from one state.
        let value = read_object(&client, NODE).await;
        let Some(Data::FloatingPoint(measure)) = value_at(&value, &measure_path) else {
            panic!("the measurement is a floating-point value");
        };
        let measure = measure.to_f32().expect("a single-precision value");
        let seconds =
            timestamp_from_data(value_at(&value, &time_path).expect("the time is in the node"))
                .expect("a timestamp")
                .seconds;
        assert_eq!(
            measure, seconds as f32,
            "the measurement and its time are of one update"
        );
    }
    writer.await.expect("the writer finishes");
    let last = read_object(&client, NODE).await;
    let Some(Data::FloatingPoint(measure)) = value_at(&last, &measure_path) else {
        panic!("the measurement is a floating-point value");
    };
    assert_eq!(measure.to_f32().unwrap(), 300.0);
    client.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn an_array_is_updated_with_its_number_of_elements_and_a_wrong_number_is_refused() {
    // The points of a curve are an array of three elements (the size given by the instance). An update of three points is applied;
    // an update of two is refused, and changes nothing (the conformance of the type, 8-1 §8.1.2.7).
    let (_server, _client, updater) = start_with_updater().await;
    let points = "IED1LD1/PTTR1$SP$TmTmpChr$crvPts";
    let ty = {
        let layout = igs_iec61850_mms_core::model::layout::layout(
            &igs_iec61850_mms_core::model::LogicalNode::new(
                "PTTR1",
                "PTTR",
                &common::with(
                    "PTTR",
                    vec![
                        igs_iec61850_mms_core::model::DataObjectSpec::new("TmTmpChr")
                            .with_attributes(["crvPts"])
                            .with_integers(std::collections::BTreeMap::from([(
                                "maxPts".to_string(),
                                3,
                            )])),
                    ],
                ),
            )
            .expect("the node is valid"),
            igs_iec61850_mms_core::model::layout::IDENTIFIER_LIMIT,
        )
        .expect("the layout is built");
        layout
            .variables
            .iter()
            .find(|v| v.name == "PTTR1$SP$TmTmpChr$crvPts")
            .expect("the points are a variable")
            .ty
            .clone()
    };
    let three = igs_iec61850_mms_core::model::layout::default_value(&ty).expect("a zero value");
    updater
        .update(&[Change::new(points, three)])
        .expect("three points are applied");
    let two = match igs_iec61850_mms_core::model::layout::default_value(&ty) {
        Some(igs_iso9506_mms_codec::mms::Data::Array(mut items)) => {
            items.pop();
            igs_iso9506_mms_codec::mms::Data::Array(items)
        }
        other => panic!("the points are an array, found {other:?}"),
    };
    assert!(updater.update(&[Change::new(points, two)]).is_err());
}
