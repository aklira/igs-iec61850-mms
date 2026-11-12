// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The control state machines (task K1; CTL-001, CTL-002, CTL-004): every transition and refusal of the four models of
//! 7-2 §20, with the time given to the machine. The tests are deterministic: the same inputs give the same actions.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::time::Duration;

use igs_iec61850_mms_core::acsi::{CheckConditions, Originator};
use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::control::machine::State;
use igs_iec61850_mms_core::control::{
    Action, AddCause, Behaviour, CheckKind, ClientId, Config, ControlModel, ControlObject, Event,
    Parameters, Refusal, SboClass, Service, Timer,
};
use igs_iso9506_mms_codec::mms::Data;

const ALICE: ClientId = ClientId(1);
const BOB: ClientId = ClientId(2);

fn config(model: ControlModel, sbo_class: SboClass, behaviour: Behaviour) -> Config {
    Config {
        model,
        sbo_class,
        sbo_timeout: Duration::from_secs(10),
        dynamic_test_timeout: Duration::from_secs(5),
        oper_timeout: Duration::from_secs(3),
        behaviour,
    }
}

fn origin() -> Originator {
    Originator {
        category: 1,
        identity: b"bay1".to_vec(),
    }
}

fn parameters(client: ClientId, ctl_val: Option<Data>) -> Parameters {
    Parameters {
        client,
        ctl_val,
        origin: origin(),
        ctl_num: 1,
        test: false,
        check: CheckConditions::default(),
    }
}

fn request(client: ClientId, service: Service, ctl_val: Option<Data>) -> Event {
    Event::Request {
        service,
        parameters: parameters(client, ctl_val),
    }
}

fn ok() -> Event {
    Event::Verdict {
        ok: true,
        cause: AddCause::UNKNOWN,
    }
}

fn fail(cause: AddCause) -> Event {
    Event::Verdict { ok: false, cause }
}

fn at(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn respond(client: ClientId, service: Service, outcome: Result<(), Refusal>) -> Action {
    Action::Respond {
        client,
        service,
        outcome,
    }
}

fn value(v: bool) -> Data {
    Data::Boolean(v)
}

fn has(actions: &[Action], wanted: &Action) -> bool {
    actions.iter().any(|action| action == wanted)
}

// ---- direct control with normal security (7-2 Figure 40) -------------------------------------------------------------

#[test]
fn a_direct_operation_with_normal_security_is_checked_twice_and_executed() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    let start = object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    assert_eq!(
        start,
        vec![Action::Check {
            kind: CheckKind::Operative,
            check: CheckConditions::default(),
            ctl_val: Some(value(true)),
        }]
    );
    assert_eq!(object.state(), State::OperTest);

    let dynamic = object.handle(at(1), ok());
    assert!(has(
        &dynamic,
        &Action::Arm {
            timer: Timer::DynamicTest,
            at: at(6)
        }
    ));
    assert_eq!(object.state(), State::DynTest);

    let done = object.handle(at(2), ok());
    assert_eq!(
        done,
        vec![
            Action::Disarm {
                timer: Timer::DynamicTest
            },
            respond(ALICE, Service::Operate, Ok(())),
            Action::Execute {
                ctl_val: value(true)
            },
        ]
    );
    assert_eq!(
        object.state(),
        State::Idle,
        "a direct model is idle after the operation"
    );
}

#[test]
fn a_failed_operative_test_is_answered_negatively_and_nothing_is_executed() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    let actions = object.handle(at(1), fail(AddCause::BLOCKED_BY_INTERLOCKING));
    assert_eq!(
        actions,
        vec![respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::named("Blocked-by-interlocking")))
        )]
    );
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn a_failed_dynamic_test_is_answered_negatively() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    let actions = object.handle(at(2), fail(AddCause::BLOCKED_BY_SYNCHROCHECK));
    assert!(has(
        &actions,
        &respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::named("Blocked-by-synchrocheck")))
        )
    ));
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn a_dynamic_test_that_runs_out_of_time_is_refused_with_time_limit_over() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    let actions = object.handle(at(6), Event::Timer(Timer::DynamicTest));
    assert!(has(
        &actions,
        &respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::TIME_LIMIT_OVER))
        )
    ));
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn a_direct_operation_without_a_value_is_inconsistent_and_a_select_is_not_of_the_model() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    assert_eq!(
        object.handle(at(0), request(ALICE, Service::Operate, None)),
        vec![respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::INCONSISTENT_PARAMETERS))
        )]
    );
    assert_eq!(
        object.handle(at(0), request(ALICE, Service::Select, None)),
        vec![respond(
            ALICE,
            Service::Select,
            Err(Refusal::ClassNotSupported)
        )]
    );
    assert_eq!(
        object.handle(at(0), request(ALICE, Service::Cancel, None)),
        vec![respond(
            ALICE,
            Service::Cancel,
            Err(Refusal::Cause(AddCause::NOT_SUPPORTED))
        )]
    );
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn a_behaviour_that_tests_the_command_mirrors_it_without_activating_the_output() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectNormal,
        SboClass::OperateOnce,
        Behaviour::TestBlocked,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    let actions = object.handle(at(2), ok());
    assert!(has(
        &actions,
        &Action::Mirror {
            ctl_val: value(true)
        }
    ));
    assert!(
        !actions.iter().any(|a| matches!(a, Action::Execute { .. })),
        "a mirrored command is not executed"
    );
    assert!(has(&actions, &respond(ALICE, Service::Operate, Ok(()))));
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn a_behaviour_that_blocks_the_command_refuses_it_before_any_output() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectNormal,
        SboClass::OperateOnce,
        Behaviour::Off,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    let actions = object.handle(at(2), ok());
    assert!(has(
        &actions,
        &respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::BLOCKED_BY_MODE))
        )
    ));
    assert!(!actions
        .iter()
        .any(|a| matches!(a, Action::Execute { .. } | Action::Mirror { .. })));
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn a_second_operation_during_an_operation_is_refused_and_another_client_cannot_cancel_it() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    assert_eq!(
        object.handle(at(0), request(BOB, Service::Operate, Some(value(true)))),
        vec![respond(
            BOB,
            Service::Operate,
            Err(Refusal::Cause(AddCause::COMMAND_ALREADY_IN_EXECUTION))
        )]
    );
    object.handle(at(1), ok());
    assert_eq!(
        object.handle(at(1), request(BOB, Service::Cancel, None)),
        vec![respond(
            BOB,
            Service::Cancel,
            Err(Refusal::Cause(AddCause::LOCKED_BY_OTHER_CLIENT))
        )]
    );
    assert_eq!(
        object.state(),
        State::DynTest,
        "the other client changed nothing"
    );
}

#[test]
fn the_operating_client_cancels_its_dynamic_test_and_the_operation_is_aborted() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    let actions = object.handle(at(2), request(ALICE, Service::Cancel, Some(value(true))));
    assert!(has(&actions, &respond(ALICE, Service::Cancel, Ok(()))));
    assert!(has(
        &actions,
        &respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::ABORTION_BY_CANCEL))
        )
    ));
    assert!(has(
        &actions,
        &Action::Disarm {
            timer: Timer::DynamicTest
        }
    ));
    assert_eq!(object.state(), State::Idle);
}

// ---- select before operate with normal security (7-2 Figure 42) ------------------------------------------------------

#[test]
fn a_selection_with_normal_security_is_held_by_its_owner_until_an_operation_ends_it() {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    let selected = object.handle(at(0), request(ALICE, Service::Select, None));
    assert_eq!(
        selected,
        vec![Action::Check {
            kind: CheckKind::Select,
            check: CheckConditions::default(),
            ctl_val: None,
        }]
    );
    let held = object.handle(at(1), ok());
    assert_eq!(
        held,
        vec![
            respond(ALICE, Service::Select, Ok(())),
            Action::Arm {
                timer: Timer::Deselect,
                at: at(11)
            },
        ]
    );
    assert_eq!(
        (object.state(), object.owner()),
        (State::Selected, Some(ALICE))
    );

    object.handle(at(2), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(3), ok());
    let done = object.handle(at(4), ok());
    assert!(has(&done, &respond(ALICE, Service::Operate, Ok(()))));
    assert!(has(
        &done,
        &Action::Execute {
            ctl_val: value(true)
        }
    ));
    assert_eq!(
        (object.state(), object.owner()),
        (State::Idle, None),
        "operate-once releases the selection"
    );
}

#[test]
fn an_operate_many_selection_is_kept_after_an_operation_and_refuses_another_value() {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateMany,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Select, None));
    object.handle(at(1), ok());
    object.handle(at(2), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(3), ok());
    let done = object.handle(at(4), ok());
    assert!(has(
        &done,
        &Action::Arm {
            timer: Timer::Deselect,
            at: at(14)
        }
    ));
    assert_eq!(
        (object.state(), object.owner()),
        (State::Selected, Some(ALICE))
    );
    assert_eq!(
        object.handle(at(5), request(ALICE, Service::Operate, Some(value(false)))),
        vec![respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::INCONSISTENT_PARAMETERS))
        )],
        "the value of the sequence is fixed by its first operation"
    );
}

#[test]
fn another_client_cannot_operate_or_cancel_a_selection_it_does_not_hold() {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Select, None));
    object.handle(at(1), ok());
    assert_eq!(
        object.handle(at(2), request(BOB, Service::Operate, Some(value(true)))),
        vec![respond(
            BOB,
            Service::Operate,
            Err(Refusal::Cause(AddCause::LOCKED_BY_OTHER_CLIENT))
        )]
    );
    assert_eq!(
        object.handle(at(2), request(BOB, Service::Cancel, None)),
        vec![respond(
            BOB,
            Service::Cancel,
            Err(Refusal::Cause(AddCause::LOCKED_BY_OTHER_CLIENT))
        )]
    );
    assert_eq!(
        object.handle(at(2), request(BOB, Service::Select, None)),
        vec![respond(
            BOB,
            Service::Select,
            Err(Refusal::Cause(AddCause::OBJECT_ALREADY_SELECTED))
        )]
    );
    assert_eq!(object.owner(), Some(ALICE));
}

#[test]
fn an_operation_without_a_selection_is_refused_as_object_not_selected_and_a_cancel_reaches_its_goal(
) {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    assert_eq!(
        object.handle(at(0), request(ALICE, Service::Operate, Some(value(true)))),
        vec![respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::OBJECT_NOT_SELECTED))
        )]
    );
    assert_eq!(
        object.handle(at(0), request(ALICE, Service::Cancel, None)),
        vec![respond(ALICE, Service::Cancel, Ok(()))]
    );
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn a_refused_selection_leaves_the_object_unselected() {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Select, None));
    let actions = object.handle(at(1), fail(AddCause::UNKNOWN));
    assert_eq!(
        actions,
        vec![respond(
            ALICE,
            Service::Select,
            Err(Refusal::Cause(AddCause::UNKNOWN))
        )]
    );
    assert_eq!((object.state(), object.owner()), (State::Idle, None));
}

#[test]
fn the_selection_ends_when_its_deselect_timer_expires_before_an_operation() {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Select, None));
    object.handle(at(1), ok());
    assert!(object
        .handle(at(11), Event::Timer(Timer::Deselect))
        .is_empty());
    assert_eq!((object.state(), object.owner()), (State::Idle, None));
}

#[test]
fn the_owner_cancels_its_selection_and_releases_the_object() {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Select, None));
    object.handle(at(1), ok());
    let actions = object.handle(at(2), request(ALICE, Service::Cancel, None));
    assert_eq!(
        actions,
        vec![
            respond(ALICE, Service::Cancel, Ok(())),
            Action::Disarm {
                timer: Timer::Deselect
            }
        ]
    );
    assert_eq!((object.state(), object.owner()), (State::Idle, None));
}

#[test]
fn the_association_of_the_owner_lost_releases_the_selection() {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Select, None));
    object.handle(at(1), ok());
    let actions = object.handle(at(2), Event::AssociationLost(ALICE));
    assert_eq!(
        actions,
        vec![Action::Disarm {
            timer: Timer::Deselect
        }]
    );
    assert_eq!((object.state(), object.owner()), (State::Idle, None));
}

#[test]
fn a_select_with_value_is_not_of_the_normal_model() {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    assert_eq!(
        object.handle(
            at(0),
            request(ALICE, Service::SelectWithValue, Some(value(true)))
        ),
        vec![respond(
            ALICE,
            Service::SelectWithValue,
            Err(Refusal::NotInModel)
        )]
    );
}

// ---- direct control with enhanced security (7-2 Figure 43) ------------------------------------------------------------

#[test]
fn an_enhanced_operation_waits_for_the_change_and_is_terminated_positively() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectEnhanced,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    let executed = object.handle(at(2), ok());
    assert!(has(
        &executed,
        &Action::Execute {
            ctl_val: value(true)
        }
    ));
    assert!(has(
        &executed,
        &Action::Arm {
            timer: Timer::Operate,
            at: at(5)
        }
    ));
    assert_eq!(object.state(), State::Change);

    let terminated = object.handle(at(3), Event::Changed);
    assert_eq!(
        terminated,
        vec![
            Action::Terminate {
                client: ALICE,
                outcome: Ok(())
            },
            Action::Disarm {
                timer: Timer::Operate
            },
        ]
    );
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn an_enhanced_change_that_does_not_come_in_time_is_terminated_negatively() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectEnhanced,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    object.handle(at(2), ok());
    let actions = object.handle(at(5), Event::Timer(Timer::Operate));
    assert_eq!(
        actions,
        vec![Action::Terminate {
            client: ALICE,
            outcome: Err(AddCause::TIME_LIMIT_OVER)
        }]
    );
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn a_cancel_during_the_change_of_an_enhanced_command_is_refused() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectEnhanced,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    object.handle(at(2), ok());
    assert_eq!(
        object.handle(at(3), request(ALICE, Service::Cancel, None)),
        vec![respond(
            ALICE,
            Service::Cancel,
            Err(Refusal::Cause(AddCause::COMMAND_ALREADY_IN_EXECUTION))
        )]
    );
    assert_eq!(object.state(), State::Change);
}

#[test]
fn a_mirrored_enhanced_command_is_terminated_as_blocked_by_mode() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectEnhanced,
        SboClass::OperateOnce,
        Behaviour::TestBlocked,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    let actions = object.handle(at(2), ok());
    assert!(has(
        &actions,
        &Action::Mirror {
            ctl_val: value(true)
        }
    ));
    assert!(has(
        &actions,
        &Action::Terminate {
            client: ALICE,
            outcome: Err(AddCause::BLOCKED_BY_MODE)
        }
    ));
    assert_eq!(object.state(), State::Idle);
}

#[test]
fn a_lost_association_during_the_change_terminates_the_command_as_aborted() {
    let mut object = ControlObject::new(config(
        ControlModel::DirectEnhanced,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(1), ok());
    object.handle(at(2), ok());
    let actions = object.handle(at(3), Event::AssociationLost(ALICE));
    assert!(has(
        &actions,
        &Action::Terminate {
            client: ALICE,
            outcome: Err(AddCause::ABORTION_BY_COMMUNICATION_LOSS)
        }
    ));
    assert_eq!(object.state(), State::Idle);
}

// ---- select before operate with enhanced security (7-2 Figure 44) -----------------------------------------------------

#[test]
fn an_enhanced_selection_is_checked_twice_then_operated_and_terminated() {
    let mut object = ControlObject::new(config(
        ControlModel::SboEnhanced,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(
        at(0),
        request(ALICE, Service::SelectWithValue, Some(value(true))),
    );
    let selection_check = object.handle(at(1), ok());
    assert_eq!(
        selection_check,
        vec![Action::Check {
            kind: CheckKind::Selection,
            check: CheckConditions::default(),
            ctl_val: Some(value(true)),
        }]
    );
    assert_eq!(object.state(), State::SelectWait);

    let selected = object.handle(at(2), ok());
    assert!(has(
        &selected,
        &respond(ALICE, Service::SelectWithValue, Ok(()))
    ));
    assert_eq!(
        (object.state(), object.owner()),
        (State::Selected, Some(ALICE))
    );

    object.handle(at(3), request(ALICE, Service::Operate, Some(value(true))));
    object.handle(at(4), ok());
    object.handle(at(5), ok());
    object.handle(at(6), Event::Changed);
    assert_eq!((object.state(), object.owner()), (State::Idle, None));
}

#[test]
fn an_enhanced_selection_refused_by_its_selection_check_leaves_the_object_unselected() {
    let mut object = ControlObject::new(config(
        ControlModel::SboEnhanced,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(
        at(0),
        request(ALICE, Service::SelectWithValue, Some(value(true))),
    );
    object.handle(at(1), ok());
    let actions = object.handle(at(2), fail(AddCause::BLOCKED_BY_INTERLOCKING));
    assert_eq!(
        actions,
        vec![respond(
            ALICE,
            Service::SelectWithValue,
            Err(Refusal::Cause(AddCause::named("Blocked-by-interlocking")))
        )]
    );
    assert_eq!((object.state(), object.owner()), (State::Idle, None));
}

#[test]
fn an_enhanced_selection_needs_its_value_and_a_plain_select_is_not_of_the_model() {
    let mut object = ControlObject::new(config(
        ControlModel::SboEnhanced,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    assert_eq!(
        object.handle(at(0), request(ALICE, Service::SelectWithValue, None)),
        vec![respond(
            ALICE,
            Service::SelectWithValue,
            Err(Refusal::Cause(AddCause::INCONSISTENT_PARAMETERS))
        )]
    );
    assert_eq!(
        object.handle(at(0), request(ALICE, Service::Select, None)),
        vec![respond(ALICE, Service::Select, Err(Refusal::NotInModel))]
    );
}

#[test]
fn an_enhanced_operation_with_another_value_than_its_selection_is_inconsistent() {
    let mut object = ControlObject::new(config(
        ControlModel::SboEnhanced,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    object.handle(
        at(0),
        request(ALICE, Service::SelectWithValue, Some(value(true))),
    );
    object.handle(at(1), ok());
    object.handle(at(2), ok());
    assert_eq!(
        object.handle(at(3), request(ALICE, Service::Operate, Some(value(false)))),
        vec![respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::INCONSISTENT_PARAMETERS))
        )]
    );
    assert_eq!(
        object.state(),
        State::Selected,
        "the selection is kept after an inconsistent operation"
    );
}

// ---- the sequence, the causes and the determinism ----------------------------------------------------------------------

#[test]
fn a_request_of_another_origin_or_test_flag_is_inconsistent_within_a_sequence() {
    let mut object = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateMany,
        Behaviour::On,
    ));
    object.handle(at(0), request(ALICE, Service::Select, None));
    object.handle(at(1), ok());
    let mut other_origin = parameters(ALICE, Some(value(true)));
    other_origin.origin = Originator {
        category: 2,
        identity: b"station".to_vec(),
    };
    assert_eq!(
        object.handle(
            at(2),
            Event::Request {
                service: Service::Operate,
                parameters: other_origin,
            }
        ),
        vec![respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::INCONSISTENT_PARAMETERS))
        )]
    );
    let mut test = parameters(ALICE, Some(value(true)));
    test.test = true;
    assert_eq!(
        object.handle(
            at(2),
            Event::Request {
                service: Service::Operate,
                parameters: test,
            }
        ),
        vec![respond(
            ALICE,
            Service::Operate,
            Err(Refusal::Cause(AddCause::INCONSISTENT_PARAMETERS))
        )]
    );
}

#[test]
fn every_cause_the_machine_gives_is_a_name_of_the_catalogue() {
    let names: Vec<&str> = catalogue::enumeration("ControlServiceStatusKind")
        .expect("ControlServiceStatusKind is in the catalogue")
        .literals
        .iter()
        .map(|literal| literal.name)
        .collect();
    for cause in [
        AddCause::OBJECT_NOT_SELECTED,
        AddCause::OBJECT_ALREADY_SELECTED,
        AddCause::INCONSISTENT_PARAMETERS,
        AddCause::LOCKED_BY_OTHER_CLIENT,
        AddCause::COMMAND_ALREADY_IN_EXECUTION,
        AddCause::BLOCKED_BY_MODE,
        AddCause::NOT_SUPPORTED,
        AddCause::ABORTION_BY_CANCEL,
        AddCause::TIME_LIMIT_OVER,
        AddCause::ABORTION_BY_COMMUNICATION_LOSS,
        AddCause::UNKNOWN,
        AddCause::BLOCKED_BY_INTERLOCKING,
        AddCause::BLOCKED_BY_SYNCHROCHECK,
    ] {
        assert!(
            names.contains(&cause.name()),
            "{} is a catalogue name",
            cause.name()
        );
    }
}

#[test]
fn the_same_inputs_give_the_same_actions() {
    let script = |object: &mut ControlObject| -> Vec<Vec<Action>> {
        vec![
            object.handle(at(0), request(ALICE, Service::Select, None)),
            object.handle(at(1), ok()),
            object.handle(at(2), request(BOB, Service::Operate, Some(value(true)))),
            object.handle(at(3), request(ALICE, Service::Operate, Some(value(true)))),
            object.handle(at(4), ok()),
            object.handle(at(5), ok()),
        ]
    };
    let mut first = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    let mut second = ControlObject::new(config(
        ControlModel::SboNormal,
        SboClass::OperateOnce,
        Behaviour::On,
    ));
    assert_eq!(script(&mut first), script(&mut second));
    assert_eq!(first, second);
}

#[test]
fn a_model_of_a_ctl_model_value_is_read_and_the_status_only_has_none() {
    assert_eq!(
        ControlModel::from_ctl_model(0),
        None,
        "status-only is not a control model"
    );
    assert_eq!(
        ControlModel::from_ctl_model(4),
        Some(ControlModel::SboEnhanced)
    );
    assert_eq!(SboClass::from_value(1), Some(SboClass::OperateMany));
    assert_eq!(Behaviour::from_value(4), Some(Behaviour::TestBlocked));
}
