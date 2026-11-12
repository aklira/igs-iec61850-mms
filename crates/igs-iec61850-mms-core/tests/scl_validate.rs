// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The checks of an SCL document against the frozen 2007B schema and its references (task S2; AT-12): a valid
//! document with a `Private` element is accepted; each invalid case gives its diagnostic, with the file, the path
//! and the line; several findings are all reported, and a refused document builds nothing.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use igs_iec61850_mms_core::scl::validate::{validate, Diagnostic};
use igs_iec61850_mms_core::scl::{parse, Limits, SclError};

/// A document that the checks accept: an IED with two data-set members, a report control, a `Private` element, and
/// the templates of its nodes.
const VALID: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SCL xmlns="http://www.iec.ch/61850/2003/SCL" version="2007" revision="B">
  <Header id="HDR1" version="1" revision="1" toolID="test"/>
  <Communication>
    <SubNetwork name="SN1" type="8-MMS">
      <ConnectedAP iedName="IED1" apName="AP1"/>
    </SubNetwork>
  </Communication>
  <IED name="IED1" type="T" manufacturer="igs" configVersion="1">
    <AccessPoint name="AP1">
      <Server>
        <Authentication none="true"/>
        <LDevice inst="LD1">
          <LN0 lnClass="LLN0" inst="" lnType="LN0T">
            <DataSet name="DS1">
              <FCDA ldInst="LD1" prefix="" lnClass="LLN0" lnInst="" doName="Mod" daName="stVal" fc="ST"/>
              <FCDA ldInst="LD1" lnClass="GGIO" lnInst="1" doName="Ind1.stVal" fc="ST"/>
            </DataSet>
            <ReportControl name="RC1" datSet="DS1" rptID="r" confRev="1" buffered="false">
              <OptFields seqNum="true" timeStamp="true" reasonCode="true"/>
            </ReportControl>
          </LN0>
          <LN lnClass="GGIO" inst="1" lnType="GGIOT"/>
        </LDevice>
      </Server>
    </AccessPoint>
    <Private type="vendor"><vendor:Probe xmlns:vendor="urn:example:vendor" level="3"/></Private>
  </IED>
  <DataTypeTemplates>
    <LNodeType id="LN0T" lnClass="LLN0">
      <DO name="Mod" type="ModT"/>
    </LNodeType>
    <LNodeType id="GGIOT" lnClass="GGIO">
      <DO name="Ind1" type="SPST"/>
    </LNodeType>
    <DOType id="ModT" cdc="INC">
      <DA name="stVal" fc="ST" bType="Enum" type="BehEnum"/>
      <DA name="q" fc="ST" bType="Quality"/>
    </DOType>
    <DOType id="SPST" cdc="SPS">
      <DA name="stVal" fc="ST" bType="BOOLEAN"/>
    </DOType>
    <EnumType id="BehEnum">
      <EnumVal ord="1">on</EnumVal>
    </EnumType>
  </DataTypeTemplates>
</SCL>
"#;

/// Parses and checks a document, with the default limits.
fn check(text: &str) -> Result<(), Vec<Diagnostic>> {
    let document = parse(text.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "test.scd")
}

/// The line (from 1) of the first line that contains `needle`.
fn line_of(text: &str, needle: &str) -> u32 {
    let index = text
        .lines()
        .position(|line| line.contains(needle))
        .expect("the needle is in the text");
    u32::try_from(index + 1).unwrap()
}

/// The document with one replacement, which must apply.
fn mutate(from: &str, to: &str) -> String {
    assert!(VALID.contains(from), "the fixture has {from:?}");
    VALID.replacen(from, to, 1)
}

/// The document is refused with a diagnostic of this code, whose message has this text, on the line of `needle`.
fn refused_with(text: &str, code: &str, message: &str, needle: &str) {
    let diagnostics = check(text).expect_err("the document is refused");
    let expected_line = line_of(text, needle);
    assert!(
        diagnostics.iter().any(|d| d.code == code
            && d.message.contains(message)
            && d.line == expected_line
            && d.file == "test.scd"),
        "expected {code} {message:?} on line {expected_line}; got {diagnostics:#?}"
    );
}

#[test]
fn a_valid_document_with_a_private_element_is_accepted() {
    assert_eq!(check(VALID), Ok(()));
}

#[test]
fn an_element_the_type_does_not_admit_is_refused() {
    let text = mutate(
        "          <LN lnClass=\"GGIO\" inst=\"1\" lnType=\"GGIOT\"/>",
        "          <LN lnClass=\"GGIO\" inst=\"1\" lnType=\"GGIOT\"/>\n          <Bogus/>",
    );
    refused_with(
        &text,
        "SCL-002",
        "element Bogus is not admitted in LDevice",
        "<Bogus/>",
    );
}

#[test]
fn a_missing_required_element_is_refused() {
    let text = mutate(
        "  <Header id=\"HDR1\" version=\"1\" revision=\"1\" toolID=\"test\"/>\n",
        "",
    );
    refused_with(&text, "SCL-002", "Header occurs 0 time(s)", "<SCL xmlns");
}

#[test]
fn a_missing_required_attribute_is_refused() {
    let text = mutate(" name=\"IED1\" type=\"T\"", " type=\"T\"");
    refused_with(
        &text,
        "SCL-002",
        "missing required attribute name",
        "<IED type",
    );
}

#[test]
fn an_unknown_attribute_is_refused() {
    let text = mutate(
        "configVersion=\"1\">",
        "configVersion=\"1\" colour=\"red\">",
    );
    refused_with(&text, "SCL-002", "unknown attribute colour", "<IED name");
}

#[test]
fn an_enumeration_value_outside_its_list_is_refused() {
    let text = mutate(
        "<IED name=\"IED1\"",
        "<IED engRight=\"maybe\" name=\"IED1\"",
    );
    refused_with(&text, "SCL-002", "engRight is not one of", "<IED engRight");
}

#[test]
fn a_root_outside_the_2007b_namespace_is_refused() {
    let text = mutate(
        "xmlns=\"http://www.iec.ch/61850/2003/SCL\" version",
        "xmlns=\"http://www.iec.ch/61850/2003/SCL-ED1\" version",
    );
    refused_with(
        &text,
        "SCL-004",
        "root is not in the namespace",
        "<SCL xmlns",
    );
}

#[test]
fn an_element_of_another_namespace_where_none_is_admitted_is_refused() {
    let text = mutate(
        "toolID=\"test\"/>",
        "toolID=\"test\"><probe:Note xmlns:probe=\"urn:example:probe\"/></Header>",
    );
    refused_with(
        &text,
        "SCL-004",
        "element Note is in namespace",
        "probe:Note",
    );
}

#[test]
fn an_unresolved_node_type_is_refused() {
    let text = mutate("lnType=\"GGIOT\"/>", "lnType=\"NOPE\"/>");
    refused_with(
        &text,
        "SCL-003",
        "lnType NOPE is not an LNodeType",
        "<LN lnClass",
    );
}

#[test]
fn an_unresolved_data_object_type_is_refused() {
    let text = mutate(
        "<DO name=\"Ind1\" type=\"SPST\"/>",
        "<DO name=\"Ind1\" type=\"NOPE\"/>",
    );
    refused_with(
        &text,
        "SCL-003",
        "type NOPE is not defined",
        "<DO name=\"Ind1\"",
    );
}

#[test]
fn an_enumeration_attribute_with_an_unknown_enum_type_is_refused() {
    let text = mutate("type=\"BehEnum\"", "type=\"NoEnum\"");
    refused_with(
        &text,
        "SCL-003",
        "type NoEnum is not defined",
        "<DA name=\"stVal\" fc=\"ST\" bType=\"Enum\"",
    );
}

#[test]
fn an_enumeration_attribute_without_a_type_is_refused() {
    let text = mutate("bType=\"Enum\" type=\"BehEnum\"", "bType=\"Enum\"");
    refused_with(
        &text,
        "SCL-003",
        "bType Enum requires a type",
        "<DA name=\"stVal\" fc=\"ST\" bType=\"Enum\"",
    );
}

#[test]
fn a_data_set_member_on_an_unknown_device_is_refused() {
    let text = mutate(
        "ldInst=\"LD1\" lnClass=\"GGIO\"",
        "ldInst=\"LD9\" lnClass=\"GGIO\"",
    );
    refused_with(
        &text,
        "SCL-003",
        "ldInst LD9 is not an LDevice",
        "<FCDA ldInst=\"LD9\"",
    );
}

#[test]
fn a_data_set_member_on_an_unknown_logical_node_is_refused() {
    let text = mutate(
        "lnClass=\"GGIO\" lnInst=\"1\" doName=\"Ind1.stVal\"",
        "lnClass=\"MMXU\" lnInst=\"1\" doName=\"Ind1.stVal\"",
    );
    refused_with(
        &text,
        "SCL-003",
        "no logical node MMXU1 in LDevice LD1",
        "lnClass=\"MMXU\"",
    );
}

#[test]
fn a_data_set_member_with_an_unknown_data_object_is_refused() {
    let text = mutate("doName=\"Ind1.stVal\"", "doName=\"Ind2.stVal\"");
    refused_with(
        &text,
        "SCL-003",
        "Ind2 is not a member of",
        "doName=\"Ind2.stVal\"",
    );
}

#[test]
fn a_data_set_member_with_an_unknown_data_attribute_is_refused() {
    let text = mutate("doName=\"Ind1.stVal\"", "doName=\"Ind1.nope\"");
    refused_with(
        &text,
        "SCL-003",
        "nope is not a member of",
        "doName=\"Ind1.nope\"",
    );
}

#[test]
fn a_report_control_on_an_unknown_data_set_is_refused() {
    let text = mutate("datSet=\"DS1\" rptID", "datSet=\"DS9\" rptID");
    refused_with(
        &text,
        "SCL-003",
        "datSet DS9 is not a DataSet",
        "<ReportControl",
    );
}

#[test]
fn a_connection_to_an_unknown_ied_is_refused() {
    let text = mutate("iedName=\"IED1\" apName", "iedName=\"IED2\" apName");
    refused_with(
        &text,
        "SCL-003",
        "iedName IED2 is not an IED of this file",
        "<ConnectedAP",
    );
}

#[test]
fn several_findings_are_all_reported_in_line_order() {
    let text = mutate("lnType=\"GGIOT\"/>", "lnType=\"NOPE\"/>");
    let text = text.replacen("datSet=\"DS1\" rptID", "datSet=\"DS9\" rptID", 1);
    let diagnostics = check(&text).expect_err("the document is refused");
    let codes: Vec<(u32, &str)> = diagnostics.iter().map(|d| (d.line, d.code)).collect();
    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    assert!(
        codes.windows(2).all(|pair| pair[0].0 <= pair[1].0),
        "{codes:?}"
    );
}

#[test]
fn a_diagnostic_is_displayed_with_its_file_line_path_and_code() {
    let text = mutate("lnType=\"GGIOT\"/>", "lnType=\"NOPE\"/>");
    let diagnostics = check(&text).expect_err("the document is refused");
    let shown = diagnostics
        .iter()
        .find(|d| d.code == "SCL-003")
        .expect("the reference finding")
        .to_string();
    assert!(shown.starts_with("test.scd:"), "{shown}");
    assert!(shown.contains("SCL-003"), "{shown}");
    assert!(
        shown.contains("SCL/IED[IED1]/AccessPoint[AP1]/Server/LDevice[LD1]/LN[GGIO1]: lnType NOPE"),
        "{shown}"
    );
}

#[test]
fn a_malicious_document_type_is_refused_before_the_checks() {
    let text = b"<?xml version=\"1.0\"?>\n<!DOCTYPE SCL [<!ENTITY e \"x\">]>\n<SCL xmlns=\"http://www.iec.ch/61850/2003/SCL\"/>";
    assert_eq!(
        parse(text, &Limits::default()).unwrap_err(),
        SclError::DocTypeNotAllowed
    );
}
