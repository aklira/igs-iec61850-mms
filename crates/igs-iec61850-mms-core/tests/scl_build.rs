// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The build of an IED from a validated SCL document (task S3; SCL-001, SCL-003, SCOPE-002): the sample configuration builds
//! its model, data sets and address parameters; each declared service or element that would be lost, and each invalid model,
//! gives its diagnostic; a refused document builds nothing.

#![cfg(feature = "scl")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use igs_iec61850_mms_core::scl::build::{build, Capabilities, Configuration};
use igs_iec61850_mms_core::scl::validate::{validate, Diagnostic};
use igs_iec61850_mms_core::scl::{parse, Limits};

const SAMPLE: &str = include_str!("data/sample.cid");

/// Parses, validates and builds the IED `IED1` of a text, with what this build executes.
fn build_text(text: &str) -> Result<Configuration, Vec<Diagnostic>> {
    let document = parse(text.as_bytes(), &Limits::default()).expect("the document parses");
    validate(&document, "sample.cid").expect("the document is valid");
    build(&document, "sample.cid", "IED1", &Capabilities::current())
}

/// The text with one replacement, which must apply.
fn mutate(from: &str, to: &str) -> String {
    assert!(SAMPLE.contains(from), "the sample has {from:?}");
    SAMPLE.replacen(from, to, 1)
}

/// The build is refused with a diagnostic of this code, whose message has this text.
fn refused_with(diagnostics: Result<Configuration, Vec<Diagnostic>>, code: &str, message: &str) {
    let diagnostics = match diagnostics {
        Ok(_) => panic!("the build should be refused"),
        Err(diagnostics) => diagnostics,
    };
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == code && d.message.contains(message)),
        "expected {code} {message:?}; got {diagnostics:#?}"
    );
}

#[test]
fn the_sample_builds_its_model_data_sets_and_addresses() {
    let configuration = build_text(SAMPLE).expect("the sample builds");
    assert_eq!(configuration.ied, "IED1");

    let devices = configuration.model.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name().to_string(), "LD1");
    let nodes: Vec<String> = devices[0]
        .nodes()
        .iter()
        .map(|n| n.name().to_string())
        .collect();
    assert_eq!(nodes, ["LLN0", "GGIO1"]);

    assert_eq!(configuration.data_sets.len(), 1);
    let set = &configuration.data_sets[0];
    assert_eq!(
        (set.device.as_str(), set.node.as_str(), set.name.as_str()),
        ("LD1", "LLN0", "DS1")
    );
    assert_eq!(set.members.len(), 2);
    assert_eq!(set.members[1].node, "GGIO1");
    assert_eq!(set.members[1].data_object, "Ind1");
    assert_eq!(set.members[1].data_attribute.as_deref(), Some("stVal"));

    assert_eq!(configuration.addresses.len(), 1);
    assert_eq!(configuration.addresses[0].access_point, "AP1");
    assert_eq!(
        configuration.addresses[0].parameters,
        [
            ("IP".to_string(), "192.0.2.10".to_string()),
            ("IP-SUBNET".to_string(), "255.255.255.0".to_string()),
        ]
    );
}

#[test]
fn a_declared_service_the_server_does_not_execute_is_refused() {
    let text = mutate(
        "<ReadWrite/>",
        "<ReadWrite/>\n        <DynDataSet max=\"4\"/>",
    );
    refused_with(
        build_text(&text),
        "SCOPE-002",
        "the declared service DynDataSet is not executed",
    );
}

#[test]
fn a_declared_buffered_report_control_is_refused_because_it_would_be_lost() {
    let text = mutate(
        "            </DataSet>\n          </LN0>",
        "            </DataSet>\n            <ReportControl name=\"RC1\" datSet=\"DS1\" rptID=\"r\" confRev=\"1\" buffered=\"true\">\n              <OptFields seqNum=\"true\" timeStamp=\"true\" reasonCode=\"true\"/>\n            </ReportControl>\n          </LN0>",
    );
    refused_with(
        build_text(&text),
        "SCOPE-002",
        "the declared ReportControl RC1 is a buffered report control block",
    );
}

#[test]
fn an_unbuffered_report_control_is_declared_with_its_options_triggers_and_data_set() {
    let text = mutate(
        "            </DataSet>\n          </LN0>",
        "            </DataSet>\n            <ReportControl name=\"RPT1\" datSet=\"DS1\" rptID=\"RPT1\" confRev=\"4\" bufTime=\"200\" intgPd=\"5000\" buffered=\"false\">\n              <TrgOps dchg=\"true\" qchg=\"true\" period=\"true\" gi=\"true\"/>\n              <OptFields seqNum=\"true\" timeStamp=\"false\" reasonCode=\"true\" dataSet=\"true\" dataRef=\"false\" configRef=\"true\"/>\n            </ReportControl>\n          </LN0>",
    );
    let configuration = build_text(&text).expect("the document builds");
    assert_eq!(configuration.reports.len(), 1);
    let report = &configuration.reports[0];
    assert_eq!(
        (
            report.device.as_str(),
            report.node.as_str(),
            report.name.as_str()
        ),
        ("LD1", "LLN0", "RPT1")
    );
    let config = &report.config;
    assert_eq!(config.rpt_id, "RPT1");
    assert_eq!(config.data_set.as_deref(), Some("DS1"));
    assert_eq!(config.conf_rev, 4);
    assert_eq!(config.buf_tm, std::time::Duration::from_millis(200));
    assert_eq!(config.intg_pd, std::time::Duration::from_millis(5000));
    assert!(
        config.trg_ops.data_change
            && config.trg_ops.quality_change
            && config.trg_ops.integrity
            && config.trg_ops.general_interrogation
    );
    assert!(!config.trg_ops.data_update);
    assert!(
        config.opt_flds.seq_num
            && config.opt_flds.reason_code
            && config.opt_flds.data_set_name
            && config.opt_flds.conf_rev
    );
    assert!(!config.opt_flds.time_stamp && !config.opt_flds.data_ref);
}

#[test]
fn a_variable_of_a_data_set_knows_its_member_and_the_triggers_of_its_attribute() {
    let configuration = build_text(SAMPLE).expect("the sample builds");
    let variables = &configuration.data_sets[0].variables;
    // Mod.stVal is a status attribute of the controllable object: dchg; GGIO1 Ind1.stVal of SPS: dchg.
    assert_eq!(variables[0].member, 0);
    assert_eq!(variables[1].member, 1);
    assert!(
        variables[1].triggers.data_change,
        "{:?}",
        variables[1].triggers
    );
    assert!(!variables[1].triggers.quality_change);
}

#[test]
fn an_instance_value_of_a_basic_attribute_is_kept_as_an_initial_value() {
    let text = mutate(
        "<LN lnClass=\"GGIO\" inst=\"1\" lnType=\"GGIOT\"/>",
        "<LN lnClass=\"GGIO\" inst=\"1\" lnType=\"GGIOT\">\n            <DOI name=\"Ind1\">\n              <DAI name=\"stVal\"><Val>true</Val></DAI>\n            </DOI>\n          </LN>",
    );
    let configuration = build_text(&text).expect("the document builds");
    assert_eq!(configuration.values.len(), 1);
    let value = &configuration.values[0];
    assert_eq!(
        (
            value.device.as_str(),
            value.node.as_str(),
            value.variable.as_str(),
            value.text.as_str()
        ),
        ("LD1", "GGIO1", "GGIO1$ST$Ind1$stVal", "true")
    );
}

#[test]
fn an_instance_without_a_value_keeps_no_value() {
    let text = mutate(
        "<LN lnClass=\"GGIO\" inst=\"1\" lnType=\"GGIOT\"/>",
        "<LN lnClass=\"GGIO\" inst=\"1\" lnType=\"GGIOT\">\n            <DOI name=\"Ind1\">\n              <DAI name=\"stVal\"/>\n            </DOI>\n          </LN>",
    );
    assert!(build_text(&text)
        .expect("the document builds")
        .values
        .is_empty());
}

#[test]
fn an_instance_of_an_object_the_type_does_not_have_is_refused() {
    let text = mutate(
        "<LN lnClass=\"GGIO\" inst=\"1\" lnType=\"GGIOT\"/>",
        "<LN lnClass=\"GGIO\" inst=\"1\" lnType=\"GGIOT\">\n            <DOI name=\"Ind9\"/>\n          </LN>",
    );
    refused_with(
        build_text(&text),
        "SCL-003",
        "DOI Ind9 is not a data object of its LNodeType",
    );
}

#[test]
fn a_template_member_the_catalogue_class_lacks_is_refused_rather_than_dropped() {
    let text = mutate(
        "<DA name=\"stVal\" fc=\"ST\" bType=\"BOOLEAN\"/>",
        "<DA name=\"stVal\" fc=\"ST\" bType=\"BOOLEAN\"/>\n      <DA name=\"bogus\" fc=\"ST\" bType=\"BOOLEAN\"/>",
    );
    refused_with(build_text(&text), "SCL-003", "bogus");
}

#[test]
fn a_mandatory_data_object_missing_from_the_type_is_refused() {
    // Beh is the mandatory data object of the GGIO class (the catalogue, 7-4); the other common ones are optional.
    let text = mutate(
        "      <DO name=\"Mod\" type=\"ENC\"/>\n      <DO name=\"Beh\" type=\"ENS\"/>\n      <DO name=\"Health\" type=\"ENS\"/>\n      <DO name=\"NamPlt\" type=\"LPL\"/>\n      <DO name=\"Ind1\"",
        "      <DO name=\"Mod\" type=\"ENC\"/>\n      <DO name=\"Health\" type=\"ENS\"/>\n      <DO name=\"NamPlt\" type=\"LPL\"/>\n      <DO name=\"Ind1\"",
    );
    refused_with(build_text(&text), "SCL-003", "Beh");
}

#[test]
fn an_unknown_ied_is_refused_by_its_name() {
    let document = parse(SAMPLE.as_bytes(), &Limits::default()).unwrap();
    let refused = build(&document, "sample.cid", "IED9", &Capabilities::current());
    refused_with(refused, "SCL-001", "no IED named IED9");
}

#[test]
fn several_findings_refuse_the_whole_configuration() {
    let text = mutate("<DataSet name=\"DS1\">", "<DataSet name=\"DS1\">");
    let text = text.replacen(
        "<ReadWrite/>",
        "<ReadWrite/>\n        <DynDataSet max=\"4\"/>",
        1,
    );
    let text = text.replacen(
        "<DO name=\"Ind1\" type=\"SPS\"/>",
        "<DO name=\"Ind1\" type=\"SPS\"/>\n      <DO name=\"Ind1\" type=\"SPS\"/>",
        1,
    );
    let diagnostics = match build_text(&text) {
        Ok(_) => panic!("the build should be refused"),
        Err(diagnostics) => diagnostics,
    };
    assert!(diagnostics.len() >= 2, "{diagnostics:#?}");
    let lines: Vec<u32> = diagnostics.iter().map(|d| d.line).collect();
    assert!(lines.windows(2).all(|pair| pair[0] <= pair[1]), "{lines:?}");
}

#[test]
fn a_data_set_selects_the_variables_of_its_members_in_order() {
    let configuration = build_text(SAMPLE).expect("the sample builds");
    let variables: Vec<(&str, &str)> = configuration.data_sets[0]
        .variables
        .iter()
        .map(|v| (v.device.as_str(), v.name.as_str()))
        .collect();
    assert_eq!(
        variables,
        [("LD1", "LLN0$ST$Mod$stVal"), ("LD1", "GGIO1$ST$Ind1$stVal")]
    );
}

#[test]
fn a_member_without_a_data_attribute_selects_every_attribute_of_its_data_object_under_its_constraint(
) {
    let text = mutate(
        "doName=\"Ind1\" daName=\"stVal\" fc=\"ST\"",
        "doName=\"Ind1\" fc=\"ST\"",
    );
    let configuration = build_text(&text).expect("the document builds");
    let names: Vec<&str> = configuration.data_sets[0]
        .variables
        .iter()
        .filter(|v| v.name.starts_with("GGIO1$"))
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["GGIO1$ST$Ind1$stVal", "GGIO1$ST$Ind1$q", "GGIO1$ST$Ind1$t"]
    );
}

#[test]
fn a_member_that_selects_no_variable_of_the_model_refuses_the_configuration() {
    // The template has stVal, but under ST only: a member asking for it under MX passes the validation (the constraint is
    // not compared there) and selects no variable of the model.
    let text = mutate(
        "doName=\"Ind1\" daName=\"stVal\" fc=\"ST\"",
        "doName=\"Ind1\" daName=\"stVal\" fc=\"MX\"",
    );
    refused_with(
        build_text(&text),
        "SCL-003",
        "selects no variable of the model",
    );
}
