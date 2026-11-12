// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The generated catalogue (task F3): counts against the NSD files, namespace identities, and a sample
//! of well-known definitions checked against the standards (IEC 61850-7-2, 7-3, 7-4, 8-1).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use igs_iec61850_mms_core::catalogue::{self, generated, TypeKind};

#[test]
fn namespaces_are_the_latest_releases() {
    for (ns, id, version, revision, release) in [
        (&generated::ns_7_2::NAMESPACE, "IEC 61850-7-2", 2007, 'B', 6),
        (&generated::ns_7_3::NAMESPACE, "IEC 61850-7-3", 2007, 'B', 6),
        (&generated::ns_7_4::NAMESPACE, "IEC 61850-7-4", 2007, 'B', 6),
        (&generated::ns_8_1::NAMESPACE, "IEC 61850-8-1", 2003, 'A', 4),
    ] {
        assert_eq!(
            (ns.id, ns.version, ns.revision, ns.release),
            (id, version, revision, release)
        );
    }
}

#[test]
fn counts_match_the_nsd_files() {
    // Counts printed by `cargo xtask catalogue`, which checks them against the XML elements.
    assert_eq!(generated::ns_7_2::BASIC_TYPES.len(), 27);
    assert_eq!(generated::ns_7_2::FUNCTIONAL_CONSTRAINTS.len(), 12);
    assert_eq!(generated::ns_7_2::PRESENCE_CONDITIONS.len(), 39);
    assert_eq!(generated::ns_7_2::ENUMERATIONS.len(), 5);
    assert_eq!(generated::ns_7_2::CONSTRUCTED_ATTRIBUTES.len(), 1);
    assert_eq!(generated::ns_7_3::CDCS.len(), 70);
    assert_eq!(generated::ns_7_3::CONSTRUCTED_ATTRIBUTES.len(), 13);
    assert_eq!(generated::ns_7_3::ENUMERATIONS.len(), 19);
    let attributes: usize = generated::ns_7_3::CDCS
        .iter()
        .map(|c| c.data_attributes.len())
        .sum();
    assert_eq!(attributes, 903);
    assert_eq!(generated::ns_7_4::LN_CLASSES.len(), 204);
    assert_eq!(
        generated::ns_7_4::LN_CLASSES
            .iter()
            .filter(|c| c.is_abstract)
            .count(),
        39
    );
    let objects: usize = generated::ns_7_4::LN_CLASSES
        .iter()
        .map(|c| c.data_objects.len())
        .sum();
    assert_eq!(objects, 1431);
    assert_eq!(generated::ns_7_4::ENUMERATIONS.len(), 62);
    assert_eq!(generated::ns_8_1::SERVICE_CDCS.len(), 8);
    assert_eq!(generated::ns_8_1::APPLIES_TO.len(), 6);
}

#[test]
fn single_point_status_has_its_status_attributes_in_order() {
    // IEC 61850-7-3, CDC SPS: stVal, q, t, with FC ST and their trigger options.
    let sps = catalogue::cdc("SPS", None).unwrap();
    let first: Vec<_> = sps.data_attributes.iter().take(3).map(|a| a.name).collect();
    assert_eq!(first, ["stVal", "q", "t"]);
    let st_val = &sps.data_attributes[0];
    assert_eq!(
        (st_val.fc, st_val.type_name, st_val.type_kind),
        ("ST", Some("BOOLEAN"), TypeKind::Basic)
    );
    assert!(st_val.dchg && !st_val.qchg);
    assert!(sps.data_attributes[1].qchg);
}

#[test]
fn behaviour_enumeration_has_the_values_of_7_4() {
    let beh = catalogue::enumeration("BehaviourModeKind").unwrap();
    let values: Vec<_> = beh.literals.iter().map(|l| (l.name, l.value)).collect();
    assert_eq!(
        values,
        [
            ("on", 1),
            ("blocked", 2),
            ("test", 3),
            ("test/blocked", 4),
            ("off", 5)
        ]
    );
}

#[test]
fn logical_nodes_carry_their_data_objects() {
    let lphd = catalogue::ln_class("LPHD").unwrap();
    let proxy = lphd
        .data_objects
        .iter()
        .find(|d| d.name == "Proxy")
        .unwrap();
    assert_eq!(proxy.cdc, "SPS");
    let xcbr = catalogue::ln_class("XCBR").unwrap();
    assert!(!xcbr.is_abstract);
    // XCBR.Pos is declared by its abstract base (switching equipment), not by XCBR itself.
    assert!(!xcbr.data_objects.iter().any(|d| d.name == "Pos"));
    let all = xcbr.all_data_objects();
    assert!(all.iter().any(|d| d.name == "Pos" && d.cdc == "DPC"));
    assert!(
        all.iter().any(|d| d.name == "Beh"),
        "inherited from the common LN classes"
    );
}

#[test]
fn every_reference_resolves_within_the_catalogue() {
    // CDCs named by data objects and sub-data objects exist; FCs named by attributes exist.
    for ln in generated::ns_7_4::LN_CLASSES {
        if let Some(base) = ln.base {
            assert!(
                catalogue::ln_class(base).is_some(),
                "{}: base {base}",
                ln.name
            );
        }
        for d in ln.data_objects {
            assert!(
                catalogue::cdc(d.cdc, None).is_some(),
                "{}.{}: CDC {}",
                ln.name,
                d.name,
                d.cdc
            );
        }
    }
    for cdc in generated::ns_7_3::CDCS {
        for a in cdc.data_attributes {
            assert!(
                catalogue::functional_constraint(a.fc).is_some(),
                "{}.{}: FC {}",
                cdc.name,
                a.name,
                a.fc
            );
        }
        for s in cdc.sub_data_objects {
            assert!(
                catalogue::cdc(s.cdc, None).is_some(),
                "{}.{}: CDC {}",
                cdc.name,
                s.name,
                s.cdc
            );
        }
    }
}
