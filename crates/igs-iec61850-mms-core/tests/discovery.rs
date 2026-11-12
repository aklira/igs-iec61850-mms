// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Discovery from the MMS answers (task A6; MODEL-002, ACSI-001): the definitions rebuilt from their descriptions, the
//! three states of a definition (known, unknown semantics, malformed), the functional groups of a node, the logical nodes
//! of a device, and the checks of the pages of a listing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use igs_iec61850_mms_core::discovery::{
    define, functional_groups, logical_nodes, Definition, DefinitionError, Leaf, Listing, Member,
    PagingError, Size,
};
use igs_iso9506_mms_codec::mms::{
    Identifier, Integer32, TypeDescription, TypeDescriptionFloatingPoint, TypeDescriptionStructure,
    TypeDescriptionStructureComponentsItem, TypeSpecification, Unsigned8,
};
use igs_iso9506_mms_codec::schema::RawBer;

fn named(name: &str, ty: TypeDescription) -> TypeDescriptionStructureComponentsItem {
    TypeDescriptionStructureComponentsItem {
        component_name: Some(Identifier(name.to_string())),
        component_type: TypeSpecification::TypeDescription(Box::new(ty)),
    }
}

fn structure(components: Vec<TypeDescriptionStructureComponentsItem>) -> TypeDescription {
    TypeDescription::Structure(TypeDescriptionStructure {
        packed: None,
        components,
    })
}

fn leaves(ty: &TypeDescription) -> Definition {
    define(ty, "obj").expect("the definition is known")
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| name.to_string()).collect()
}

#[test]
fn the_basic_types_are_rebuilt_as_their_leaves() {
    assert_eq!(
        leaves(&TypeDescription::Boolean),
        Definition::Leaf(Leaf::Boolean)
    );
    assert_eq!(
        leaves(&TypeDescription::Integer(Unsigned8(8))),
        Definition::Leaf(Leaf::Integer { bits: 8 })
    );
    assert_eq!(
        leaves(&TypeDescription::BitString(Integer32(13))),
        Definition::Leaf(Leaf::BitString {
            size: Size {
                length: 13,
                variable: false
            }
        })
    );
    assert_eq!(
        leaves(&TypeDescription::UtcTime),
        Definition::Leaf(Leaf::UtcTime)
    );
    assert_eq!(
        leaves(&TypeDescription::FloatingPoint(
            TypeDescriptionFloatingPoint {
                format_width: Unsigned8(32),
                exponent_width: Unsigned8(8),
            }
        )),
        Definition::Leaf(Leaf::FloatingPoint {
            format_width: 32,
            exponent_width: 8
        })
    );
}

#[test]
fn a_structure_keeps_its_named_members_in_order() {
    let ty = structure(vec![
        named("stVal", TypeDescription::Boolean),
        named("q", TypeDescription::BitString(Integer32(13))),
    ]);
    assert_eq!(
        leaves(&ty),
        Definition::Structure(vec![
            Member {
                name: "stVal".to_string(),
                definition: Definition::Leaf(Leaf::Boolean),
            },
            Member {
                name: "q".to_string(),
                definition: Definition::Leaf(Leaf::BitString {
                    size: Size {
                        length: 13,
                        variable: false
                    }
                }),
            },
        ])
    );
}

#[test]
fn a_type_the_server_names_is_kept_as_unresolved_not_refused() {
    let ty = TypeDescription::Structure(TypeDescriptionStructure {
        packed: None,
        components: vec![TypeDescriptionStructureComponentsItem {
            component_name: Some(Identifier("dstAddress".to_string())),
            component_type: TypeSpecification::TypeName(
                igs_iso9506_mms_codec::mms::ObjectName::VmdSpecific(Identifier(
                    "PhyComAddr".to_string(),
                )),
            ),
        }],
    });
    let definition = leaves(&ty);
    assert_eq!(
        definition.member("dstAddress"),
        Some(&Definition::Leaf(Leaf::Unresolved {
            path: "obj.dstAddress".to_string()
        }))
    );
}

#[test]
fn a_type_of_mms_this_client_does_not_know_is_kept_as_unknown() {
    let ty = TypeDescription::Unsupported {
        tag: 99,
        raw: RawBer(vec![0x00]),
    };
    assert_eq!(leaves(&ty), Definition::Leaf(Leaf::Unknown));
}

#[test]
fn a_malformed_description_is_an_error_with_its_path() {
    let unnamed = TypeDescription::Structure(TypeDescriptionStructure {
        packed: None,
        components: vec![TypeDescriptionStructureComponentsItem {
            component_name: None,
            component_type: TypeSpecification::TypeDescription(Box::new(TypeDescription::Boolean)),
        }],
    });
    assert!(
        matches!(define(&unnamed, "obj"), Err(DefinitionError::Malformed { reason, .. }) if reason == "a component has no name")
    );

    let twice = structure(vec![
        named("q", TypeDescription::Boolean),
        named("q", TypeDescription::Boolean),
    ]);
    assert!(
        matches!(define(&twice, "obj"), Err(DefinitionError::Malformed { reason, .. }) if reason == "two components have the same name")
    );

    let empty_bits = TypeDescription::BitString(Integer32(0));
    assert!(
        matches!(define(&empty_bits, "obj"), Err(DefinitionError::Malformed { path, .. }) if path == "obj")
    );

    let no_bits = TypeDescription::Integer(Unsigned8(0));
    assert!(define(&no_bits, "obj").is_err());

    let exponent = TypeDescription::FloatingPoint(TypeDescriptionFloatingPoint {
        format_width: Unsigned8(8),
        exponent_width: Unsigned8(8),
    });
    assert!(define(&exponent, "obj").is_err());

    // A negative size is a variable length of at most its absolute value (D-016): not an error.
    assert_eq!(
        define(&TypeDescription::VisibleString(Integer32(-255)), "obj"),
        Ok(Definition::Leaf(Leaf::VisibleString {
            size: Size {
                length: 255,
                variable: true
            }
        }))
    );
    assert_eq!(
        define(&TypeDescription::BitString(Integer32(-13)), "obj"),
        Ok(Definition::Leaf(Leaf::BitString {
            size: Size {
                length: 13,
                variable: true
            }
        }))
    );

    let nested = structure(vec![named(
        "inner",
        structure(vec![named(
            "leaf",
            TypeDescription::BitString(Integer32(0)),
        )]),
    )]);
    assert!(
        matches!(define(&nested, "obj"), Err(DefinitionError::Malformed { path, .. }) if path == "obj.inner.leaf")
    );
}

#[test]
fn a_description_nested_beyond_the_walk_is_refused_not_followed() {
    let mut ty = TypeDescription::Boolean;
    for _ in 0..12 {
        ty = structure(vec![named("n", ty)]);
    }
    assert!(matches!(
        define(&ty, "obj"),
        Err(DefinitionError::TooDeep { .. })
    ));
}

#[test]
fn the_functional_groups_of_a_node_hold_its_objects_in_order() {
    let status = structure(vec![named(
        "Ind1",
        structure(vec![named("stVal", TypeDescription::Boolean)]),
    )]);
    let measure = structure(vec![named(
        "TotW",
        structure(vec![named("mag", TypeDescription::Boolean)]),
    )]);
    let node = structure(vec![named("MX", measure), named("ST", status)]);
    let groups = functional_groups(&node).expect("the groups are known");
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].functional_constraint, "MX");
    assert_eq!(groups[0].objects, names(&["TotW"]));
    assert_eq!(groups[1].functional_constraint, "ST");
    assert_eq!(groups[1].objects, names(&["Ind1"]));
}

#[test]
fn a_functional_constraint_that_is_not_a_structure_is_malformed() {
    let node = structure(vec![named("ST", TypeDescription::Boolean)]);
    assert!(
        matches!(functional_groups(&node), Err(DefinitionError::Malformed { path, .. }) if path == "ST")
    );
    assert!(functional_groups(&TypeDescription::Boolean).is_err());
}

#[test]
fn the_logical_nodes_are_the_named_variables_without_a_dollar() {
    let variables = names(&[
        "GGIO1",
        "GGIO1$ST$Ind1$stVal",
        "MMXU1",
        "MMXU1$MX$TotW$mag$f",
    ]);
    assert_eq!(logical_nodes(&variables), names(&["GGIO1", "MMXU1"]));
}

/// A listing of the pages, with the flag of each page.
fn listed(pages: &[(&[&str], bool)]) -> Result<Vec<String>, PagingError> {
    let mut listing = Listing::new();
    for (page, more) in pages {
        listing.add_page(&names(page), *more)?;
    }
    Ok(listing.into_names())
}

#[test]
fn the_pages_of_a_listing_are_kept_in_the_order_of_the_server() {
    // The server's order is its own: a listing is not sorted, and the pages keep it.
    assert_eq!(
        listed(&[(&["b", "a"], true), (&["c"], false)]),
        Ok(names(&["b", "a", "c"]))
    );
    assert_eq!(
        listed(&[(&[], false)]),
        Ok(Vec::new()),
        "an empty last page ends the listing"
    );
}

#[test]
fn a_name_repeated_over_the_pages_is_refused() {
    assert_eq!(
        listed(&[(&["b", "c"], true), (&["c", "d"], false)]),
        Err(PagingError::Repeated {
            name: "c".to_string()
        })
    );
    assert_eq!(
        listed(&[(&["x", "x"], false)]),
        Err(PagingError::Repeated {
            name: "x".to_string()
        }),
        "a repeat within one page is a repeat too"
    );
}

#[test]
fn a_listing_that_stalls_is_refused() {
    assert_eq!(
        listed(&[(&["b"], true), (&[], true)]),
        Err(PagingError::Stalled)
    );
}
