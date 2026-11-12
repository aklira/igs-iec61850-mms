// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The write rules of the model and the values behind the variables (task A4, MODEL-005): which attributes an MMS
//! Write may change, which enumeration values it holds, and how a value is read and replaced by its path.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::model::layout::{
    defines_enumerations, layout, set_at, value_at, writable_constraint, DefinedValues,
    IDENTIFIER_LIMIT,
};
use igs_iec61850_mms_core::model::{is_controllable, is_required, DataObjectSpec, LogicalNode};
use igs_iso9506_mms_codec::mms::Data;

/// The mandatory data objects of a class, except `skip`, followed by `named`.
fn with(class: &str, skip: &str, named: Vec<DataObjectSpec>) -> Vec<DataObjectSpec> {
    let mut specs: Vec<DataObjectSpec> = catalogue::ln_class(class)
        .expect("the class is in the catalogue")
        .all_data_objects()
        .into_iter()
        .filter(|object| is_required(&object.presence) && object.name != skip)
        .map(|object| DataObjectSpec::new(object.name))
        .collect();
    specs.extend(named);
    specs
}

/// The variable of a name, from the layout of a node.
fn variable<'a>(
    layout: &'a igs_iec61850_mms_core::model::layout::LnLayout,
    name: &str,
) -> &'a igs_iec61850_mms_core::model::layout::VariableLayout {
    layout
        .variables
        .iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("the variable {name} is in the layout"))
}

#[test]
fn only_the_constraints_that_set_data_values_writes_are_writable() {
    for fc in ["SP", "SV", "CF", "DC", "BL"] {
        assert!(writable_constraint(fc), "{fc} takes SetDataValues");
    }
    for fc in ["ST", "MX", "CO", "SE", "RP", "BR", "OR", "SR", "EX"] {
        assert!(!writable_constraint(fc), "{fc} does not take SetDataValues");
    }
}

#[test]
fn a_substitution_is_writable_and_a_status_is_read_only() {
    let specs = with(
        "GGIO",
        "",
        vec![DataObjectSpec::new("Ind1").with_attributes(["subVal", "subEna"])],
    );
    let node = LogicalNode::new("GGIO1", "GGIO", &specs).unwrap();
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    assert!(variable(&layout, "GGIO1$SV$Ind1$subVal").writable);
    assert!(variable(&layout, "GGIO1$SV$Ind1").writable);
    assert!(!variable(&layout, "GGIO1$ST$Ind1$stVal").writable);
    assert!(!variable(&layout, "GGIO1$ST$Ind1").writable);
    assert!(
        !variable(&layout, "GGIO1").writable,
        "the node is never written as a whole"
    );
}

#[test]
fn a_controllable_object_is_read_only_and_a_status_object_is_not() {
    // Mod is an enumerated controllable object (ENC); Health is an enumerated status (ENS).
    assert!(is_controllable("ENC"));
    assert!(!is_controllable("ENS"));
    let specs = with(
        "LLN0",
        "Mod",
        vec![
            DataObjectSpec::new("Mod").with_attributes(["subVal"]),
            DataObjectSpec::new("Health").with_attributes(["subVal"]),
        ],
    );
    let node = LogicalNode::new("LLN01", "LLN0", &specs).unwrap();
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    assert!(
        !variable(&layout, "LLN01$SV$Mod$subVal").writable,
        "a controllable object goes through its state machine"
    );
    assert!(variable(&layout, "LLN01$SV$Health$subVal").writable);
}

#[test]
fn an_enumeration_holds_the_values_its_namespace_defines() {
    let specs = with(
        "GGIO",
        "",
        vec![DataObjectSpec::new("Health").with_attributes(["subVal"])],
    );
    let node = LogicalNode::new("GGIO1", "GGIO", &specs).unwrap();
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    let defined = variable(&layout, "GGIO1$SV$Health$subVal");
    let literals: Vec<i64> = catalogue::enumeration("HealthKind")
        .expect("HealthKind is in the catalogue")
        .literals
        .iter()
        .map(|literal| i64::from(literal.value))
        .collect();
    assert_eq!(
        defined.enumerations,
        vec![DefinedValues {
            path: Vec::new(),
            values: literals.clone()
        }]
    );
    let first = *literals.first().expect("the enumeration has values");
    assert!(defines_enumerations(
        &Data::Integer(i128::from(first)),
        &defined.enumerations
    ));
    // A value the enumeration does not define, though the MMS integer holds it: refused by the rule of 8-1.
    let undefined = (-4i64..=3).find(|value| !literals.contains(value));
    if let Some(undefined) = undefined {
        assert!(!defines_enumerations(
            &Data::Integer(i128::from(undefined)),
            &defined.enumerations
        ));
    }
}

#[test]
fn an_enumeration_inside_a_structure_is_found_by_its_path() {
    let specs = with(
        "GGIO",
        "",
        vec![DataObjectSpec::new("Health").with_attributes(["subVal", "subEna"])],
    );
    let node = LogicalNode::new("GGIO1", "GGIO", &specs).unwrap();
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    let object = variable(&layout, "GGIO1$SV$Health");
    assert_eq!(
        object.enumerations.len(),
        1,
        "subVal is the only enumeration of the object"
    );
    let defined = &object.enumerations[0];
    // The path of the enumeration is its component in the object: one index, the position of subVal.
    assert_eq!(defined.path.len(), 1);
    let first = *defined.values.first().expect("the enumeration has values");

    // The object's value, with subVal set to a defined value: the write holds; with a value outside the set: refused.
    let mut value = value_at(&layout.root_value, &object.path)
        .expect("the object has a value")
        .clone();
    let Data::Structure(parts) = &mut value else {
        panic!("the object is a structure");
    };
    parts[defined.path[0]] = Data::Integer(i128::from(first));
    assert!(defines_enumerations(&value, &object.enumerations));
    let literals = &defined.values;
    if let Some(undefined) = (-4i64..=3).find(|candidate| !literals.contains(candidate)) {
        let Data::Structure(parts) = &mut value else {
            panic!("the object is a structure");
        };
        parts[defined.path[0]] = Data::Integer(i128::from(undefined));
        assert!(!defines_enumerations(&value, &object.enumerations));
    }
}

#[test]
fn a_value_is_read_and_replaced_by_its_path() {
    let specs = with(
        "GGIO",
        "",
        vec![DataObjectSpec::new("Ind1").with_attributes(["subVal"])],
    );
    let node = LogicalNode::new("GGIO1", "GGIO", &specs).unwrap();
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    let path = variable(&layout, "GGIO1$SV$Ind1$subVal").path.clone();
    let mut root = layout.root_value.clone();
    assert_eq!(value_at(&root, &path), Some(&Data::Boolean(false)));
    assert!(set_at(&mut root, &path, Data::Boolean(true)));
    assert_eq!(value_at(&root, &path), Some(&Data::Boolean(true)));
    // The same value is seen by the name of the data object that holds it.
    let object = variable(&layout, "GGIO1$SV$Ind1").path.clone();
    assert!(matches!(value_at(&root, &object), Some(Data::Structure(_))));
    assert_eq!(
        value_at(&root, &object).and_then(|value| match value {
            Data::Structure(parts) => parts.get(path[path.len() - 1]),
            _ => None,
        }),
        Some(&Data::Boolean(true))
    );
    // A path that leaves the structure is not written.
    assert!(!set_at(&mut root, &[99, 0], Data::Boolean(false)));
}
