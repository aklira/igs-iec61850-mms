// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use igs_iec61850_mms_core::scl::build::{build, Capabilities};
use igs_iec61850_mms_core::scl::validate::validate;
use igs_iec61850_mms_core::scl::{parse, Limits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The whole path of the import (S1 to S3): a document, then its checks, then its model. Each stage answers with a value or a
    // refusal for any input; the build only runs on a document that the checks accept.
    let limits = Limits {
        max_bytes: 1 << 20,
        max_depth: 32,
        max_elements: 10_000,
        max_instances: 5_000,
        max_attributes: 32,
        max_value_len: 512,
    };
    let Ok(document) = parse(data, &limits) else {
        return;
    };
    if validate(&document, "fuzz.cid").is_err() {
        return;
    }
    let _ = build(&document, "fuzz.cid", "IED1", &Capabilities::current());
});
