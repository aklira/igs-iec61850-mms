// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use igs_iec61850_mms_core::scl::{parse, Limits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Any input gives a document or a refusal. The limits are smaller than the defaults, so that the bounds are reached
    // by short inputs too.
    let limits = Limits {
        max_bytes: 1 << 20,
        max_depth: 32,
        max_elements: 10_000,
        max_instances: 5_000,
        max_attributes: 32,
        max_value_len: 512,
    };
    let _ = parse(data, &limits);
});
