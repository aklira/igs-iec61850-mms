// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Property tests of the names and the object references (task A1): round trips between the ACSI reference, the
//! MMS item and the data reference, and the limits of each form.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use igs_iec61850_mms_core::names::{
    FunctionalConstraint, MmsName, Name, Scope, FUNCTIONAL_CONSTRAINTS,
};
use igs_iec61850_mms_core::reference::{ObjectReference, PathItem, RefError};
use proptest::prelude::*;

/// A valid IEC 61850 name: a letter, then letters, digits and underscores, up to 64 characters.
fn a_name() -> impl Strategy<Value = String> {
    "[A-Za-z][A-Za-z0-9_]{0,63}"
}

fn a_constraint() -> impl Strategy<Value = &'static str> {
    prop::sample::select(FUNCTIONAL_CONSTRAINTS.to_vec())
}

/// A reference with a logical node and a path of up to four names, with or without an array element.
fn a_reference(with_indices: bool) -> impl Strategy<Value = ObjectReference> {
    (
        a_name(),
        a_name(),
        prop::collection::vec((a_name(), prop::option::of(0u32..10_000)), 0..4),
    )
        .prop_map(move |(ld, ln, path)| {
            let path = path
                .into_iter()
                .map(|(name, index)| match (with_indices, index) {
                    (true, Some(index)) => PathItem::element(Name::new(&name).unwrap(), index),
                    _ => PathItem::new(Name::new(&name).unwrap()),
                })
                .collect();
            ObjectReference::new(Name::new(&ld).unwrap(), Name::new(&ln).unwrap(), path)
        })
}

proptest! {
    #[test]
    fn an_acsi_reference_reads_back_from_its_text(reference in a_reference(true)) {
        let text = reference.to_string();
        // The reference is at most 129 octets; a longer one is refused, not truncated.
        if text.len() <= 129 {
            prop_assert_eq!(ObjectReference::parse(&text).unwrap(), reference);
        } else {
            match ObjectReference::parse(&text) {
                Err(RefError::TooLong { max, .. }) => prop_assert_eq!(max, 129),
                other => prop_assert!(false, "a reference of {} octets gave {:?}", text.len(), other),
            }
        }
    }

    #[test]
    fn an_mms_item_reads_back_into_its_reference(
        reference in a_reference(false),
        code in a_constraint(),
    ) {
        let fc = FunctionalConstraint::new(code).unwrap();
        match reference.to_mms(fc) {
            Ok(item) => {
                prop_assert!(item.item().as_str().len() <= 64);
                prop_assert_eq!(item.scope(), &Scope::Domain(reference.logical_device_name().clone()));
                let (back, back_fc) = ObjectReference::from_mms(&item).unwrap();
                prop_assert_eq!(back, reference);
                prop_assert_eq!(back_fc, fc);
            }
            Err(error) => {
                // The only refusal of a reference without indices is the length of the item.
                match error {
                    RefError::TooLong { max, .. } => prop_assert_eq!(max, 64),
                    other => prop_assert!(false, "unexpected refusal {:?}", other),
                }
            }
        }
    }

    #[test]
    fn a_data_reference_keeps_its_array_elements(
        reference in a_reference(true),
        code in a_constraint(),
    ) {
        let fc = FunctionalConstraint::new(code).unwrap();
        if let Ok(text) = reference.to_data_ref(fc) {
            let (back, back_fc) = ObjectReference::parse_data_ref(&text).unwrap();
            prop_assert_eq!(back, reference);
            prop_assert_eq!(back_fc, fc);
        }
    }

    #[test]
    fn an_mms_name_reads_back_from_its_text(
        domain in a_name(),
        item in "[A-Za-z][A-Za-z0-9_$]{0,63}",
        scope in 0u8..3,
    ) {
        let text = match scope {
            0 => format!("{domain}/{item}"),
            1 => format!("/{item}"),
            _ => format!("@{item}"),
        };
        let name = MmsName::parse(&text).unwrap();
        prop_assert_eq!(name.to_text(), text);
    }

    #[test]
    fn a_name_longer_than_64_or_with_a_foreign_character_is_refused(
        valid in a_name(),
        extra in "[A-Za-z0-9_]{1,30}",
        bad in prop::sample::select(vec!['.', '/', '$', '(', ')', '@', ' ', '-']),
    ) {
        let long = format!("{valid}{extra}");
        if long.len() > 64 {
            prop_assert!(Name::new(&long).is_err());
        } else {
            prop_assert!(Name::new(&long).is_ok());
        }
        let foreign = format!("{valid}{bad}x");
        prop_assert!(Name::new(&foreign).is_err());
    }
}

#[test]
fn every_functional_constraint_of_the_catalogue_is_accepted() {
    use igs_iec61850_mms_core::catalogue::generated::{ns_7_2, ns_8_1};
    let catalogue = ns_7_2::FUNCTIONAL_CONSTRAINTS
        .iter()
        .chain(ns_8_1::FUNCTIONAL_CONSTRAINTS.iter());
    for constraint in catalogue {
        assert!(
            FunctionalConstraint::new(constraint.abbreviation).is_ok(),
            "{} is missing",
            constraint.abbreviation
        );
    }
}
