// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use std::time::Duration;

use igs_iec61850_mms_core::acsi::{CheckConditions, Originator};
use igs_iec61850_mms_core::control::machine::ControlObject;
use igs_iec61850_mms_core::control::{
    AddCause, Behaviour, ClientId, Config, ControlModel, Event, Parameters, SboClass, Service, Timer,
};
use igs_iso9506_mms_codec::generated::types::Data;
use libfuzzer_sys::fuzz_target;

fn event(kind: u8, value: u8) -> Event {
    let client = ClientId(u64::from(value % 3));
    let parameters = Parameters {
        client,
        ctl_val: if value & 1 != 0 { Some(Data::Boolean(value & 2 != 0)) } else { None },
        origin: Originator {
            category: i64::from(value % 4),
            identity: vec![value; usize::from(value % 3)],
        },
        ctl_num: value,
        test: value & 4 != 0,
        check: CheckConditions::default(),
    };
    let service = match value % 4 {
        0 => Service::Select,
        1 => Service::SelectWithValue,
        2 => Service::Operate,
        _ => Service::Cancel,
    };
    match kind % 8 {
        0 | 1 | 2 => Event::Request { service, parameters },
        3 => Event::Verdict { ok: value & 1 != 0, cause: AddCause::UNKNOWN },
        4 => Event::Changed,
        5 => Event::Deferred { outcome: if value & 1 != 0 { Ok(()) } else { Err(AddCause::UNKNOWN) } },
        6 => Event::Timer(match value % 3 {
            0 => Timer::Deselect,
            1 => Timer::DynamicTest,
            _ => Timer::Operate,
        }),
        _ => Event::AssociationLost(client),
    }
}

fuzz_target!(|data: &[u8]| {
    // The first byte chooses the model and the behaviour; the rest are events, two bytes each, at a moving clock.
    let Some((&choice, rest)) = data.split_first() else {
        return;
    };
    let model = match choice % 4 {
        0 => ControlModel::DirectNormal,
        1 => ControlModel::SboNormal,
        2 => ControlModel::DirectEnhanced,
        _ => ControlModel::SboEnhanced,
    };
    let behaviour = match choice / 4 % 5 {
        0 => Behaviour::On,
        1 => Behaviour::Blocked,
        2 => Behaviour::Test,
        3 => Behaviour::TestBlocked,
        _ => Behaviour::Off,
    };
    let mut object = ControlObject::new(Config {
        model,
        sbo_class: if choice & 128 != 0 { SboClass::OperateMany } else { SboClass::OperateOnce },
        sbo_timeout: Duration::from_secs(10),
        dynamic_test_timeout: Duration::from_secs(5),
        oper_timeout: Duration::from_secs(3),
        behaviour,
    });
    let mut now = Duration::ZERO;
    for pair in rest.chunks_exact(2) {
        let (Some(&kind), Some(&value)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        now = now.saturating_add(Duration::from_millis(u64::from(value) * 100));
        let _ = object.handle(now, event(kind, value));
    }
});
