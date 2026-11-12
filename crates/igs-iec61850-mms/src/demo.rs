// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! A demonstration model, built in code (task A7): the device `IED1LD1` with the node `GGIO1` (a status, a
//! measurement, a substitution and an enumerated status with its substitution), and the node `MMXU1234567890123456`,
//! whose flattened names pass the identifier limit of the ISO profile. The binaries `igs61850-server` and the tests serve it.

use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::model::{
    is_required, DataObjectSpec, LogicalDevice, LogicalNode, ModelError, ServerModel,
};

/// The name of the demonstration logical device, which is the domain of its MMS names.
pub const DEVICE: &str = "IED1LD1";

/// The mandatory data objects of a class, followed by the named ones.
fn with(class: &str, named: Vec<DataObjectSpec>) -> Result<Vec<DataObjectSpec>, ModelError> {
    let class_definition =
        catalogue::ln_class(class).ok_or_else(|| ModelError::UnknownLnClass(class.to_string()))?;
    let mut specs: Vec<DataObjectSpec> = class_definition
        .all_data_objects()
        .into_iter()
        .filter(|object| is_required(&object.presence))
        .map(|object| DataObjectSpec::new(object.name))
        .collect();
    specs.extend(named);
    Ok(specs)
}

/// The demonstration model.
pub fn demo_model() -> Result<ServerModel, ModelError> {
    let ggio = LogicalNode::new(
        "GGIO1",
        "GGIO",
        &with(
            "GGIO",
            vec![
                DataObjectSpec::new("Ind1").with_attributes(["subVal", "subEna"]),
                DataObjectSpec::new("AnIn1"),
                DataObjectSpec::new("Health").with_attributes(["subVal"]),
            ],
        )?,
    )?;
    let measuring = LogicalNode::new(
        "MMXU1234567890123456",
        "MMXU",
        &with("MMXU", vec![DataObjectSpec::new("TotW")])?,
    )?;
    let mut server = ServerModel::new();
    server.add_device(LogicalDevice::new(DEVICE, vec![ggio, measuring])?)?;
    Ok(server)
}
