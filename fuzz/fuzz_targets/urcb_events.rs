// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use std::collections::BTreeMap;
use std::time::Duration;

use igs_iec61850_mms_core::control::ClientId;
use igs_iec61850_mms_core::report::urcb::{
    Config, Event, OptionalFields, Timer, Trigger, TriggerOptions, Urcb, Write,
};
use libfuzzer_sys::fuzz_target;

/// One event per two bytes: the kind, then the value. The data sets and the members are small, so that the references are reached.
fn event(kind: u8, value: u8) -> Event {
    let client = ClientId(u64::from(value % 3));
    let flags = |bits: u8| OptionalFields {
        seq_num: bits & 1 != 0,
        time_stamp: bits & 2 != 0,
        reason_code: bits & 4 != 0,
        data_set_name: bits & 8 != 0,
        data_ref: bits & 16 != 0,
        conf_rev: bits & 32 != 0,
    };
    let triggers = |bits: u8| TriggerOptions {
        data_change: bits & 1 != 0,
        quality_change: bits & 2 != 0,
        data_update: bits & 4 != 0,
        integrity: bits & 8 != 0,
        general_interrogation: bits & 16 != 0,
    };
    let millis = Duration::from_millis(u64::from(value) * 10);
    match kind % 12 {
        0 => Event::Request { client, write: Write::Reserve(value & 1 != 0) },
        1 => Event::Request { client, write: Write::RptEna(value & 1 != 0) },
        2 => Event::Request { client, write: Write::RptId(format!("rpt{}", value % 4)) },
        3 => Event::Request {
            client,
            write: Write::DataSet(if value & 1 != 0 { Some(format!("DS{}", value % 3)) } else { None }),
        },
        4 => Event::Request { client, write: Write::OptFields(flags(value)) },
        5 => Event::Request { client, write: Write::BufTm(millis) },
        6 => Event::Request { client, write: Write::TrgOps(triggers(value)) },
        7 => Event::Request { client, write: Write::IntgPd(millis) },
        8 => Event::Request { client, write: Write::GeneralInterrogation },
        9 => Event::Notification {
            member: usize::from(value % 4),
            trigger: match value % 3 {
                0 => Trigger::DataChange,
                1 => Trigger::QualityChange,
                _ => Trigger::DataUpdate,
            },
        },
        10 => Event::Timer(if value & 1 != 0 { Timer::BufferTime } else { Timer::Integrity }),
        _ => Event::AssociationLost(client),
    }
}

fuzz_target!(|data: &[u8]| {
    let config = Config {
        rpt_id: "rpt".to_string(),
        data_set: Some("DS1".to_string()),
        data_sets: BTreeMap::from([("DS1".to_string(), 3), ("DS2".to_string(), 1)]),
        opt_flds: OptionalFields {
            seq_num: true,
            time_stamp: true,
            reason_code: true,
            data_set_name: true,
            data_ref: false,
            conf_rev: true,
        },
        buf_tm: Duration::from_millis(100),
        intg_pd: Duration::from_millis(500),
        trg_ops: TriggerOptions {
            data_change: true,
            quality_change: true,
            data_update: false,
            integrity: true,
            general_interrogation: true,
        },
        conf_rev: 1,
    };
    let mut block = Urcb::new(config);
    // The clock only moves forward, as the caller's does. Every event is handled; the answers are not checked here, only that
    // no sequence panics or grows without a bound of its own (the actions of one event are at most the members of its data set).
    let mut now = Duration::ZERO;
    for pair in data.chunks_exact(2) {
        let (Some(&kind), Some(&value)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        now = now.saturating_add(Duration::from_millis(u64::from(value)));
        let _ = block.handle(now, event(kind, value));
    }
});
