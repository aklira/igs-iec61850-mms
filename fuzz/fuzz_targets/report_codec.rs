// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use std::time::Duration;

use igs_iec61850_mms_core::report::encode::{
    binary_time, bit_count, object_reference, optional_fields_bits, optional_fields_from_bits,
    reasons_from_bits, triggers_from_bits, triggers_to_bits,
};
use igs_iso9506_mms_codec::schema::BitString;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&unused, octets)) = data.split_first() else {
        return;
    };
    // Any bit string, with any number of unused bits: the decoders answer for it, and the encoders give back a bit string
    // that decodes to the same values (the round trip of the options and the triggers).
    let bits = BitString {
        unused,
        bytes: octets.to_vec(),
    };
    let _ = bit_count(&bits);
    let _ = reasons_from_bits(&bits);

    let fields = optional_fields_from_bits(&bits);
    assert_eq!(optional_fields_from_bits(&optional_fields_bits(fields)), fields);

    let triggers = triggers_from_bits(&bits);
    assert_eq!(triggers_from_bits(&triggers_to_bits(triggers)), triggers);

    let millis = data.iter().fold(0u64, |acc, byte| acc.wrapping_mul(31).wrapping_add(u64::from(*byte)));
    let _ = binary_time(Duration::from_millis(millis));

    let name = String::from_utf8_lossy(data);
    let _ = object_reference("LD1", &name);
});
