// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The SCL parser (task S1; SCL-004): the malicious documents are refused with their reason, a valid document is read
//! with its structure, and no input makes the parser panic.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use igs_iec61850_mms_core::scl::{parse, Limits, SclError};
use proptest::prelude::*;

const HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SCL xmlns="http://www.iec.ch/61850/2003/SCL" version="2007" revision="B" release="4">"#;

/// A small but complete ICD-like document: a header, one IED with one access point, one logical device and one node.
fn valid_document() -> String {
    format!(
        r#"{HEADER}
  <Header id="demo" version="1" revision="1" toolID="test" nameStructure="IEDName"/>
  <IED name="IED1" type="T" manufacturer="igs" configVersion="1">
    <AccessPoint name="S1">
      <Server>
        <LDevice inst="LD1">
          <LN0 lnClass="LLN0" inst="" lnType="LLN0_T"/>
          <LN lnClass="GGIO" inst="1" prefix="" lnType="GGIO_T"/>
        </LDevice>
      </Server>
    </AccessPoint>
  </IED>
</SCL>"#
    )
}

fn refused(bytes: &[u8], limits: &Limits) -> SclError {
    parse(bytes, limits).expect_err("the document is refused")
}

#[test]
fn a_valid_document_is_read_with_its_structure() {
    let document =
        parse(valid_document().as_bytes(), &Limits::default()).expect("the document is valid");
    assert_eq!(document.root.name, "SCL");
    assert_eq!(
        document.root.namespace.as_deref(),
        Some("http://www.iec.ch/61850/2003/SCL")
    );
    assert_eq!(document.root.attribute("version"), Some("2007"));
    let ied = document
        .root
        .children
        .iter()
        .find(|e| e.name == "IED")
        .expect("an IED");
    assert_eq!(ied.attribute("name"), Some("IED1"));
    // The instances of the document: IED, AccessPoint, Server, LDevice, LN0 and LN.
    assert_eq!(document.instances, 6);
    // SCL, IED, AccessPoint, Server, LDevice, LN0: the root is at depth 1.
    assert_eq!(document.depth, 6);
}

#[test]
fn an_external_entity_is_refused_and_nothing_is_fetched() {
    let xxe = r#"<?xml version="1.0"?>
<!DOCTYPE SCL [ <!ENTITY secret SYSTEM "file:///etc/passwd"> ]>
<SCL xmlns="http://www.iec.ch/61850/2003/SCL">&secret;</SCL>"#;
    assert_eq!(
        refused(xxe.as_bytes(), &Limits::default()),
        SclError::DocTypeNotAllowed
    );
}

#[test]
fn a_network_entity_is_refused() {
    let remote =
        r#"<!DOCTYPE SCL [ <!ENTITY remote SYSTEM "http://192.0.2.1/x.xml"> ]><SCL>&remote;</SCL>"#;
    assert_eq!(
        refused(remote.as_bytes(), &Limits::default()),
        SclError::DocTypeNotAllowed
    );
}

#[test]
fn an_entity_expansion_bomb_is_refused() {
    // The billion-laughs shape: nested internal entities. A document type declaration is refused before expansion.
    let bomb = r#"<!DOCTYPE SCL [
  <!ENTITY a "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa">
  <!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">
  <!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;">
]><SCL>&c;</SCL>"#;
    assert_eq!(
        refused(bomb.as_bytes(), &Limits::default()),
        SclError::DocTypeNotAllowed
    );
}

#[test]
fn a_document_larger_than_the_limit_is_refused_before_it_is_read() {
    let limits = Limits {
        max_bytes: 100,
        ..Limits::default()
    };
    let document = valid_document();
    assert!(matches!(
        refused(document.as_bytes(), &limits),
        SclError::TooLarge { limit: 100, .. }
    ));
}

#[test]
fn a_document_nested_deeper_than_the_limit_is_refused() {
    let depth = 100;
    let mut document = String::from(HEADER);
    for _ in 0..depth {
        document.push_str("<Private>");
    }
    for _ in 0..depth {
        document.push_str("</Private>");
    }
    document.push_str("</SCL>");
    let limits = Limits {
        max_depth: 32,
        ..Limits::default()
    };
    assert_eq!(
        refused(document.as_bytes(), &limits),
        SclError::TooDeep { limit: 32 }
    );
}

#[test]
fn the_depth_is_capped_whatever_the_limit_asks() {
    // Nested beyond the ceiling of the builder, with a limit that asks for more: refused at the ceiling.
    let depth = 400;
    let mut document = String::from(HEADER);
    for _ in 0..depth {
        document.push_str("<Private>");
    }
    for _ in 0..depth {
        document.push_str("</Private>");
    }
    document.push_str("</SCL>");
    let limits = Limits {
        max_depth: 100_000,
        ..Limits::default()
    };
    assert!(matches!(
        refused(document.as_bytes(), &limits),
        SclError::TooDeep { limit: 256 } | SclError::Xml(_)
    ));
}

#[test]
fn too_many_elements_are_refused() {
    let limits = Limits {
        max_elements: 3,
        ..Limits::default()
    };
    let document = format!("{HEADER}<A/><B/><C/><D/></SCL>");
    assert!(matches!(
        refused(document.as_bytes(), &limits),
        SclError::TooManyElements { limit: 3 }
    ));
}

#[test]
fn too_many_instances_are_refused() {
    let mut document = String::from(HEADER);
    for n in 0..20 {
        document.push_str(&format!(r#"<LN lnClass="GGIO" inst="{n}"/>"#));
    }
    document.push_str("</SCL>");
    let limits = Limits {
        max_instances: 10,
        ..Limits::default()
    };
    assert_eq!(
        refused(document.as_bytes(), &limits),
        SclError::TooManyInstances { limit: 10 }
    );
}

#[test]
fn an_element_with_too_many_attributes_is_refused() {
    let attributes: String = (0..300).map(|n| format!(r#" a{n}="1""#)).collect();
    let document = format!("{HEADER}<Private{attributes}/></SCL>");
    assert!(matches!(
        refused(document.as_bytes(), &Limits::default()),
        SclError::TooManyAttributes { limit: 128, .. }
    ));
}

#[test]
fn a_value_longer_than_the_limit_is_refused() {
    let long = "x".repeat(5000);
    let document = format!("{HEADER}<Private name=\"{long}\"/></SCL>");
    assert!(matches!(
        refused(document.as_bytes(), &Limits::default()),
        SclError::ValueTooLong { limit: 4096, .. }
    ));
}

#[test]
fn a_root_that_is_not_scl_is_refused() {
    let document = r#"<?xml version="1.0"?><Other/>"#;
    assert_eq!(
        refused(document.as_bytes(), &Limits::default()),
        SclError::NotScl {
            root: "Other".to_string()
        }
    );
}

#[test]
fn a_document_that_is_not_xml_or_not_utf8_is_refused() {
    assert!(matches!(
        refused(b"<SCL><unclosed>", &Limits::default()),
        SclError::Xml(_)
    ));
    assert_eq!(
        refused(&[0xff, 0xfe, 0x00], &Limits::default()),
        SclError::NotUtf8
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any byte string gives a document or a refusal: the parser never panics.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = parse(&bytes, &Limits::default());
    }

    /// A valid document with one byte changed gives a document or a refusal, never a panic.
    #[test]
    fn a_mutated_document_never_panics(position in 0usize..1000, value in any::<u8>()) {
        let mut bytes = valid_document().into_bytes();
        let index = position % bytes.len();
        bytes[index] = value;
        let _ = parse(&bytes, &Limits::default());
    }
}
