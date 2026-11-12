// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The control state machines of a controllable data object (IEC 61850-7-2 §20.1 to §20.3; tasks K1, CTL-001, CTL-002).
//!
//! A control object is driven by four models, chosen by its `ctlModel`: direct control with normal security, select-before-
//! operate with normal security, direct control with enhanced security, and select-before-operate with enhanced security.
//! The machine here is sans-I/O. It takes a request, a verdict of the application, a timer or a change of status, with the
//! current time, and returns the actions to take: the responses to the clients, the checks and the commands that the
//! application performs, the output it activates, and the timers it arms (CTL-003: the application makes the checks and
//! the commands, and the machine waits for its answer without blocking).
//!
//! Each request of a sequence carries the same origin, test flag and control value; a request that does not is refused
//! with `Inconsistent-parameters` (CTL-004). The owner of a selection is the client that selected the object: any other
//! client is refused while the object is selected. The time-activated operate (§20.4) is not part of this machine: the
//! mapping of K2 declares it unsupported.
//!
//! The state diagrams are those of 7-2 Figures 40, 42, 43 and 44; Table 108 gives the refusals of each state. The
//! behaviour of the logical node (`Beh`) decides whether the output is activated: `on` and `test` activate it, `test/blocked`
//! mirrors the command without activating it, and `blocked` and `off` refuse it with `Blocked-by-Mode` (D-020).

use std::time::Duration;

use igs_iso9506_mms_codec::mms::Data;

use crate::acsi::{CheckConditions, Originator};

pub mod machine;

/// The client of a control request: the association it comes from. Opaque to the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(pub u64);

/// The control model of a control object, from its `ctlModel` (catalogue `CtlModelKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlModel {
    /// Direct control with normal security (`ctlModel` 1).
    DirectNormal,
    /// Select before operate with normal security (`ctlModel` 2).
    SboNormal,
    /// Direct control with enhanced security (`ctlModel` 3).
    DirectEnhanced,
    /// Select before operate with enhanced security (`ctlModel` 4).
    SboEnhanced,
}

impl ControlModel {
    /// The model of a `ctlModel` value. `status-only` (0) and the unknown values have no control model.
    pub fn from_ctl_model(value: i64) -> Option<Self> {
        match value {
            1 => Some(Self::DirectNormal),
            2 => Some(Self::SboNormal),
            3 => Some(Self::DirectEnhanced),
            4 => Some(Self::SboEnhanced),
            _ => None,
        }
    }

    /// Whether the model selects before it operates.
    pub fn selects(self) -> bool {
        matches!(self, Self::SboNormal | Self::SboEnhanced)
    }

    /// Whether the model is enhanced: its command is terminated by a CommandTermination (§20.3).
    pub fn enhanced(self) -> bool {
        matches!(self, Self::DirectEnhanced | Self::SboEnhanced)
    }
}

/// The class of a selection: whether it is kept after one operation (catalogue `SboClassKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SboClass {
    /// `operate-once` (0): the selection ends with the operation.
    OperateOnce,
    /// `operate-many` (1): the selection is kept after the operation.
    OperateMany,
}

impl SboClass {
    /// The class of a `sboClass` value.
    pub fn from_value(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::OperateOnce),
            1 => Some(Self::OperateMany),
            _ => None,
        }
    }
}

/// The behaviour of the logical node that contains the control object (catalogue `BehaviourModeKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// `on` (1): the outputs are activated.
    On,
    /// `blocked` (2): the commands are refused.
    Blocked,
    /// `test` (3): the outputs are activated for a test.
    Test,
    /// `test/blocked` (4): the commands are mirrored, not executed.
    TestBlocked,
    /// `off` (5): the commands are refused.
    Off,
}

impl Behaviour {
    /// The behaviour of a `Beh` value.
    pub fn from_value(value: i64) -> Option<Self> {
        match value {
            1 => Some(Self::On),
            2 => Some(Self::Blocked),
            3 => Some(Self::Test),
            4 => Some(Self::TestBlocked),
            5 => Some(Self::Off),
            _ => None,
        }
    }
}

/// The configuration of a control object: its model and the attributes that set its timers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The control model.
    pub model: ControlModel,
    /// The class of the selection (`sboClass`); used by the select-before-operate models.
    pub sbo_class: SboClass,
    /// The deselect timer (`sboTimeout`): the selection ends when it expires before an operation.
    pub sbo_timeout: Duration,
    /// The time allowed for the dynamic test (the check made while the command is pending).
    pub dynamic_test_timeout: Duration,
    /// The time allowed for the change of the status after the output is activated (`operTimeout`).
    pub oper_timeout: Duration,
    /// The behaviour of the logical node.
    pub behaviour: Behaviour,
}

/// The service of a control request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// Select (`Sel`), with normal security.
    Select,
    /// SelectWithValue (`SelVal`), with enhanced security.
    SelectWithValue,
    /// Operate (`Oper`).
    Operate,
    /// Cancel.
    Cancel,
}

/// The parameters of a control request (§20.5.2). All requests of a sequence carry the same origin, test flag and value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameters {
    /// The client that sends the request.
    pub client: ClientId,
    /// The control value (`ctlVal`). A selection of normal security carries none.
    pub ctl_val: Option<Data>,
    /// The originator of the control.
    pub origin: Originator,
    /// The control sequence number (`ctlNum`). Carried, not checked: its use is an issue of the client.
    pub ctl_num: u8,
    /// Whether the request is for a test.
    pub test: bool,
    /// The checks to make before the operation (`Check`).
    pub check: CheckConditions,
}

/// The reason of a negative response (Table 108, §20.5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// An AddCause (`ControlServiceStatusKind`).
    Cause(AddCause),
    /// The request is not of the model of the object: `class-not-supported` for a direct model (Table 108).
    ClassNotSupported,
    /// The request does not belong to the model of the object: an unknown control request (Table 108, last row).
    NotInModel,
}

/// An AddCause: one of the names of the catalogue enumeration `ControlServiceStatusKind`. The constants are the ones the
/// machine gives; the application gives any other name with [`AddCause::named`] (checked by the catalogue in the tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddCause(&'static str);

impl AddCause {
    /// The cause with this name, as the catalogue gives it.
    pub fn named(name: &'static str) -> Self {
        Self(name)
    }

    /// The name of the cause.
    pub fn name(self) -> &'static str {
        self.0
    }

    /// The object is not selected.
    pub const OBJECT_NOT_SELECTED: Self = Self("Object-not-selected");
    /// The object is selected by another client.
    pub const OBJECT_ALREADY_SELECTED: Self = Self("Object-already-selected");
    /// The parameters of the request do not match those of its sequence.
    pub const INCONSISTENT_PARAMETERS: Self = Self("Inconsistent-parameters");
    /// Another client holds the selection.
    pub const LOCKED_BY_OTHER_CLIENT: Self = Self("Locked-by-other-client");
    /// A command is already in execution.
    pub const COMMAND_ALREADY_IN_EXECUTION: Self = Self("Command-already-in-execution");
    /// The behaviour of the logical node refuses the command.
    pub const BLOCKED_BY_MODE: Self = Self("Blocked-by-Mode");
    /// The control is not supported in this state (a cancel of a direct model).
    pub const NOT_SUPPORTED: Self = Self("Not-supported");
    /// The command was cancelled.
    pub const ABORTION_BY_CANCEL: Self = Self("Abortion-by-cancel");
    /// A timer of the command expired.
    pub const TIME_LIMIT_OVER: Self = Self("Time-limit-over");
    /// The association of the selecting client was lost.
    pub const ABORTION_BY_COMMUNICATION_LOSS: Self = Self("Abortion-by-communication-loss");
    /// The cause is unknown.
    pub const UNKNOWN: Self = Self("Unknown");
    /// An interlock of the application refuses the command (a check of the application, CTL-003).
    pub const BLOCKED_BY_INTERLOCKING: Self = Self("Blocked-by-interlocking");
    /// The synchrocheck of the application refuses the command (a check of the application, CTL-003).
    pub const BLOCKED_BY_SYNCHROCHECK: Self = Self("Blocked-by-synchrocheck");
}

/// A timer of the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    /// The selection expires without an operation (`sboTimeout`).
    Deselect,
    /// The dynamic test is not completed in time.
    DynamicTest,
    /// The status does not change in time after the output is activated (`operTimeout`).
    Operate,
}

/// The check of the application that the machine asks for (CTL-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
    /// The operative test of a selection (the check of its select request).
    Select,
    /// The selection check of an enhanced select-before-operate.
    Selection,
    /// The operative test of an operation (performed in the state PerformTest).
    Operative,
    /// The dynamic test of an operation (performed in the state WaitForExecution).
    Dynamic,
}

/// An input of the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A control request of a client.
    Request {
        /// The service.
        service: Service,
        /// Its parameters.
        parameters: Parameters,
    },
    /// The answer of the application to the last check asked for by [`Action::Check`].
    Verdict {
        /// Whether the check succeeded.
        ok: bool,
        /// The cause, when it did not.
        cause: AddCause,
    },
    /// The status of the controlled object has reached the demanded value.
    Changed,
    /// The outcome of the command, reported by the application when it does not come as a change of status (CTL-003): the
    /// deferred outcome of an enhanced command that waits for the change (a negative one carries its cause).
    Deferred {
        /// The outcome: `Ok` completes the command as a change does.
        outcome: Result<(), AddCause>,
    },
    /// A timer that the machine armed has expired.
    Timer(Timer),
    /// The behaviour of the logical node changed.
    Behaviour(Behaviour),
    /// The association of a client was lost.
    AssociationLost(ClientId),
}

/// An output of the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The response to a request of a client: positive, or negative with its reason.
    Respond {
        /// The client that sent the request.
        client: ClientId,
        /// The service of the request.
        service: Service,
        /// The outcome.
        outcome: Result<(), Refusal>,
    },
    /// The CommandTermination of an enhanced command, positive or negative.
    Terminate {
        /// The client that sent the operation.
        client: ClientId,
        /// The outcome: `Ok` when the status changed, or the cause of the failure.
        outcome: Result<(), AddCause>,
    },
    /// A check the application makes, and answers with [`Event::Verdict`].
    Check {
        /// The check.
        kind: CheckKind,
        /// The check conditions of the request.
        check: CheckConditions,
        /// The control value of the sequence, when it has one.
        ctl_val: Option<Data>,
    },
    /// The output is activated with this value (the application makes the change).
    Execute {
        /// The control value.
        ctl_val: Data,
    },
    /// The command is mirrored, not executed (the behaviour is `test/blocked`).
    Mirror {
        /// The control value.
        ctl_val: Data,
    },
    /// A timer is armed to expire at this time, from the start of the machine.
    Arm {
        /// The timer.
        timer: Timer,
        /// The instant at which it expires.
        at: Duration,
    },
    /// A timer is disarmed.
    Disarm {
        /// The timer.
        timer: Timer,
    },
}

pub use machine::ControlObject;
