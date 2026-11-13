// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The server model and its MMS layout (task A3): the checks against the namespace (MODEL-004), and the
//! flattened names and the type of a logical node (8-1 §7.3).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use igs_iec61850_mms_core::catalogue::{self, generated};
use igs_iec61850_mms_core::model::layout::{layout, IDENTIFIER_LIMIT};
use igs_iec61850_mms_core::model::{
    is_required, DataObjectSpec, LogicalDevice, LogicalNode, ModelError, Node, ServerModel,
};
use igs_iso9506_mms_codec::mms::{Data, Integer32, TypeDescription, TypeSpecification};
use igs_iso9506_mms_codec::schema::BitString;

/// The mandatory data objects of a class, with no attribute: the base of every test node.
fn required(class: &str) -> Vec<DataObjectSpec> {
    catalogue::ln_class(class)
        .expect("the class is in the catalogue")
        .all_data_objects()
        .into_iter()
        .filter(|object| is_required(&object.presence))
        .map(|object| DataObjectSpec::new(object.name))
        .collect()
}

/// The mandatory data objects of a class, except the one named `skip`, followed by `extra`.
fn with(class: &str, skip: &str, extra: Vec<DataObjectSpec>) -> Vec<DataObjectSpec> {
    let mut specs: Vec<DataObjectSpec> = required(class)
        .into_iter()
        .filter(|spec| spec.name() != skip)
        .collect();
    specs.extend(extra);
    specs
}

/// A node of the class GGIO with the status `Ind1` (ST) and the measurement `AnIn1` (MX), both instantiated.
fn ggio(name: &str) -> LogicalNode {
    let specs = with(
        "GGIO",
        "",
        vec![DataObjectSpec::new("Ind1"), DataObjectSpec::new("AnIn1")],
    );
    LogicalNode::new(name, "GGIO", &specs).expect("the node is valid")
}

/// The value at the path of a variable, from the root of the logical node.
fn value_at<'a>(root: &'a Data, path: &[usize]) -> &'a Data {
    let mut current = root;
    for index in path {
        match current {
            Data::Structure(members) => current = &members[*index],
            other => panic!("the path leaves the structure at {other:?}"),
        }
    }
    current
}

/// The path of a named variable of the layout.
fn path_of(layout: &igs_iec61850_mms_core::model::layout::LnLayout, name: &str) -> Vec<usize> {
    layout
        .variables
        .iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("the variable {name} is in the layout"))
        .path
        .clone()
}

fn names(node: &LogicalNode, limit: usize) -> Vec<String> {
    layout(node, limit)
        .unwrap()
        .variables
        .into_iter()
        .map(|variable| variable.name)
        .collect()
}

#[test]
fn a_node_without_its_mandatory_objects_is_refused() {
    let refused = LogicalNode::new("LLN01", "LLN0", &[]);
    assert!(matches!(
        refused,
        Err(ModelError::MissingDataObject { class: "LLN0", .. })
    ));
}

#[test]
fn an_object_the_class_does_not_define_is_refused() {
    let specs = with("GGIO", "", vec![DataObjectSpec::new("Nope")]);
    assert!(matches!(
        LogicalNode::new("GGIO1", "GGIO", &specs),
        Err(ModelError::UnknownDataObject { class: "GGIO", .. })
    ));
}

#[test]
fn an_abstract_or_unknown_class_is_refused() {
    let abstract_class = generated::ns_7_4::LN_CLASSES
        .iter()
        .find(|class| class.is_abstract)
        .expect("the namespace has abstract classes");
    assert!(matches!(
        LogicalNode::new("X1", abstract_class.name, &[]),
        Err(ModelError::AbstractLnClass(_))
    ));
    assert!(matches!(
        LogicalNode::new("X1", "NOPE", &[]),
        Err(ModelError::UnknownLnClass(_))
    ));
}

#[test]
fn an_attribute_the_common_data_class_does_not_define_is_refused() {
    let specs = with(
        "GGIO",
        "",
        vec![DataObjectSpec::new("Ind1").with_attributes(["nope"])],
    );
    assert!(matches!(
        LogicalNode::new("GGIO1", "GGIO", &specs),
        Err(ModelError::UnknownAttribute { .. })
    ));
}

#[test]
fn a_repeated_object_or_node_or_device_is_refused() {
    let mut specs = with("GGIO", "", vec![DataObjectSpec::new("Ind1")]);
    specs.push(DataObjectSpec::new("Ind1"));
    assert!(matches!(
        LogicalNode::new("GGIO1", "GGIO", &specs),
        Err(ModelError::Duplicate {
            what: "data object",
            ..
        })
    ));
    let device = LogicalDevice::new("IED1LD1", vec![ggio("GGIO1"), ggio("GGIO1")]);
    assert!(matches!(
        device,
        Err(ModelError::Duplicate {
            what: "logical node",
            ..
        })
    ));
    let mut server = ServerModel::new();
    server
        .add_device(LogicalDevice::new("IED1LD1", vec![ggio("GGIO1")]).unwrap())
        .unwrap();
    assert!(matches!(
        server.add_device(LogicalDevice::new("IED1LD1", vec![]).unwrap()),
        Err(ModelError::Duplicate {
            what: "logical device",
            ..
        })
    ));
}

#[test]
fn an_optional_attribute_is_instantiated_only_when_named() {
    // subEna is conditional (MFsubst) in SPS: it is not instantiated unless the application names it.
    let plain = names(&ggio("GGIO1"), IDENTIFIER_LIMIT);
    assert!(plain.contains(&"GGIO1$ST$Ind1$stVal".to_string()));
    assert!(!plain.contains(&"GGIO1$SV$Ind1$subEna".to_string()));
    let specs = with(
        "GGIO",
        "",
        vec![DataObjectSpec::new("Ind1").with_attributes(["subEna"])],
    );
    let named = names(
        &LogicalNode::new("GGIO1", "GGIO", &specs).unwrap(),
        IDENTIFIER_LIMIT,
    );
    assert!(named.contains(&"GGIO1$SV$Ind1$subEna".to_string()));
}

#[test]
fn the_functional_constraints_follow_the_order_of_figure_4() {
    let node = ggio("GGIO1");
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    let TypeDescription::Structure(root) = &layout.root else {
        panic!("the logical node is a structure");
    };
    let order: Vec<String> = root
        .components
        .iter()
        .map(|c| c.component_name.as_ref().unwrap().0.clone())
        .collect();
    // MX comes before ST in Figure 4, and only the constraints that are instantiated appear.
    assert_eq!(order, ["MX", "ST"]);
}

#[test]
fn the_flattened_names_follow_the_paths_of_the_tree() {
    let layout = layout(&ggio("GGIO1"), IDENTIFIER_LIMIT).unwrap();
    let stval = layout
        .variables
        .iter()
        .find(|v| v.name == "GGIO1$ST$Ind1$stVal")
        .expect("the status value is a flattened name");
    // ST is the second component of the root. In ST, the mandatory Beh comes first, as the object list gives it
    // (the mandatory objects are added before the named ones), then Ind1. stVal is the first of Ind1.
    assert_eq!(stval.path, vec![1, 1, 0]);
    assert_eq!(stval.ty, TypeDescription::Boolean);
    let node_variable = &layout.variables[0];
    assert_eq!(node_variable.name, "GGIO1");
    assert!(node_variable.path.is_empty());
    // A functional constraint alone is not a variable (D-006).
    assert!(!layout.variables.iter().any(|v| v.name == "GGIO1$ST"));
}

#[test]
fn a_constructed_attribute_keeps_its_components_under_their_names() {
    // The magnitude of a measurement is an analogue value, a constructed attribute with two components.
    let specs = with("MMXU", "", vec![DataObjectSpec::new("TotW")]);
    let node = LogicalNode::new("MMXU1", "MMXU", &specs).expect("the node is valid");
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    let names: Vec<&str> = layout.variables.iter().map(|v| v.name.as_str()).collect();
    assert!(names.contains(&"MMXU1$MX$TotW$mag$f"));
    assert!(names.contains(&"MMXU1$MX$TotW$mag$i"));
    let float = layout
        .variables
        .iter()
        .find(|v| v.name == "MMXU1$MX$TotW$mag$f")
        .unwrap();
    assert!(matches!(float.ty, TypeDescription::FloatingPoint(_)));
}

#[test]
fn an_attribute_the_class_does_not_define_is_not_taken_from_another_class() {
    // The status of SPS has no originator: the originator of a controllable object is not added to it.
    let specs = with(
        "GGIO",
        "",
        vec![DataObjectSpec::new("Ind1").with_attributes(["origin"])],
    );
    assert!(matches!(
        LogicalNode::new("GGIO1", "GGIO", &specs),
        Err(ModelError::UnknownAttribute { .. })
    ));
}

#[test]
fn a_flattened_name_longer_than_the_limit_is_counted_not_listed() {
    let layout = layout(&ggio("GGIO1"), 16).unwrap();
    assert!(layout.long_names > 0);
    assert!(layout
        .variables
        .iter()
        .all(|v| v.name.chars().count() <= 16));
}

#[test]
fn the_initial_value_conforms_to_the_layout() {
    let layout = layout(&ggio("GGIO1"), IDENTIFIER_LIMIT).unwrap();
    let stval = path_of(&layout, "GGIO1$ST$Ind1$stVal");
    assert_eq!(value_at(&layout.root_value, &stval), &Data::Boolean(false));
    // The quality is thirteen zero bits.
    let quality = path_of(&layout, "GGIO1$ST$Ind1$q");
    match value_at(&layout.root_value, &quality) {
        Data::BitString(bits) => {
            assert_eq!(bits.unused, 3);
            assert_eq!(bits.bytes, vec![0, 0]);
        }
        other => panic!("the quality is a bit string, found {other:?}"),
    }
    // Every variable has a value at its path.
    for variable in &layout.variables {
        let _ = value_at(&layout.root_value, &variable.path);
    }
}

#[test]
fn the_position_of_a_breaker_is_a_two_bit_double_point() {
    // The position of a breaker is a DPC whose status is a Dbpos (7-2 Table 24): a bit string of two bits, zero
    // at the start.
    let specs = with("XCBR", "", vec![]);
    let node = LogicalNode::new("XCBR1", "XCBR", &specs).expect("the node is valid");
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    let stval = layout
        .variables
        .iter()
        .find(|v| v.name == "XCBR1$ST$Pos$stVal")
        .expect("the status of the position is a variable");
    assert_eq!(stval.ty, TypeDescription::BitString(Integer32(2)));
    assert_eq!(
        value_at(&layout.root_value, &stval.path),
        &Data::BitString(BitString {
            unused: 6,
            bytes: vec![0]
        })
    );
}

#[test]
fn a_type_without_a_mapping_is_refused_with_its_path() {
    // The tracking object of a sampled value block holds the destination address, a PhyComAddr whose form
    // depends on the protocol of the traffic, and the mapping does not resolve it. The object is given first, so
    // that its error is the first one found.
    let mut specs = vec![DataObjectSpec::new("UsvcbTrk")];
    specs.extend(with("LTRK", "UsvcbTrk", vec![]));
    let refused = LogicalNode::new("LTRK1", "LTRK", &specs);
    assert!(
        matches!(
            &refused,
            Err(ModelError::UnmappedType { path, type_name }) if path == "LTRK1.UsvcbTrk.dstAddress" && type_name == "PhyComAddr"
        ),
        "found {refused:?}"
    );
}

#[test]
fn the_tracking_objects_of_the_reports_and_the_logs_are_served_and_the_goose_one_is_not() {
    // The tracking objects of the reports, the logs and the setting groups hold trigger, option and entry fields,
    // which are mapped, and each one builds on its own.
    for object in ["BrcbTrk", "UrcbTrk", "LocbTrk", "SgcbTrk"] {
        let mut specs = vec![DataObjectSpec::new(object)];
        specs.extend(with("LTRK", object, vec![]));
        let served = LogicalNode::new("LTRK1", "LTRK", &specs);
        assert!(served.is_ok(), "{object} is served: {served:?}");
    }
    // The GOOSE tracking holds the destination address, a PhyComAddr whose type the SCSM gives: it is refused,
    // and so is the destination of the sampled values above (UsvcbTrk).
    let mut specs = vec![DataObjectSpec::new("GocbTrk")];
    specs.extend(with("LTRK", "GocbTrk", vec![]));
    let refused = LogicalNode::new("LTRK1", "LTRK", &specs);
    assert!(
        matches!(
            &refused,
            Err(ModelError::Unsupported { path, .. }) if path == "LTRK1.GocbTrk.dstAddress"
        ),
        "found {refused:?}"
    );
}

#[test]
fn the_tracking_of_a_buffered_block_is_served_with_its_entry_fields_at_zero() {
    // The entry identifier is eight zero octets and the entry time the six-octet zero time of day (8-1 §8.1.3.5,
    // §8.1.3.8); the trigger and option fields are bit strings of their minimum size.
    let mut specs = vec![DataObjectSpec::new("BrcbTrk")];
    specs.extend(with("LTRK", "BrcbTrk", vec![]));
    let node = LogicalNode::new("LTRK1", "LTRK", &specs).expect("the tracking node is served");
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    let entry = path_of(&layout, "LTRK1$SR$BrcbTrk$entryID");
    assert_eq!(
        value_at(&layout.root_value, &entry),
        &Data::OctetString(vec![0; 8])
    );
    let time = path_of(&layout, "LTRK1$SR$BrcbTrk$timeOfEntry");
    assert!(matches!(
        value_at(&layout.root_value, &time),
        Data::BinaryTime(_)
    ));
    let trigger = path_of(&layout, "LTRK1$SR$BrcbTrk$trgOps");
    assert_eq!(
        value_at(&layout.root_value, &trigger),
        &Data::BitString(BitString {
            unused: 2,
            bytes: vec![0]
        })
    );
}

#[test]
fn the_structure_of_a_data_object_holds_its_attributes_in_their_order() {
    let node = ggio("GGIO1");
    let layout = layout(&node, IDENTIFIER_LIMIT).unwrap();
    let TypeDescription::Structure(root) = &layout.root else {
        panic!("the logical node is a structure");
    };
    let TypeSpecification::TypeDescription(st) = &root.components[1].component_type else {
        panic!("the status is a structure");
    };
    let TypeDescription::Structure(st) = st.as_ref() else {
        panic!("the status is a structure");
    };
    let TypeSpecification::TypeDescription(ind1) = &st.components[0].component_type else {
        panic!("Ind1 is a structure");
    };
    let TypeDescription::Structure(ind1) = ind1.as_ref() else {
        panic!("Ind1 is a structure");
    };
    let attributes: Vec<String> = ind1
        .components
        .iter()
        .map(|c| c.component_name.as_ref().unwrap().0.clone())
        .collect();
    // The attributes follow the order of the common data class: stVal first, then the quality and the timestamp.
    assert_eq!(attributes[0], "stVal");
    assert_eq!(attributes[1], "q");
    assert_eq!(attributes[2], "t");
    assert!(matches!(node.objects()[0].node(), Node::Branch(_)));
}

#[test]
fn a_mandatory_object_check_uses_the_conditions_named_m_only() {
    assert!(is_required(&catalogue::Presence {
        condition: "M",
        arguments: None
    }));
    assert!(!is_required(&catalogue::Presence {
        condition: "MOcond",
        arguments: None
    }));
}
