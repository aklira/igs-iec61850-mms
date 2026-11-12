// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The transitions of the control state machines (IEC 61850-7-2 Figures 40, 42, 43 and 44, Table 108; task K1).
//!
//! Two rules of the figures shape the code. A command that does not succeed (a failed check, a timer, a cancel, a refused
//! behaviour, a lost association) ends the selection: it goes back to `Idle`. A command that succeeds ends according to the
//! class of the selection: `operate-once` releases it, `operate-many` keeps it with its owner.

use std::time::Duration;

use igs_iso9506_mms_codec::mms::Data;

use super::{
    Action, AddCause, Behaviour, CheckKind, ClientId, Config, Event, Parameters, Refusal, SboClass,
    Service, Timer,
};
use crate::acsi::{CheckConditions, Originator};

/// The states of the models. `Idle` is `Ready` for a direct model and `Unselected` for a select-before-operate model;
/// `Selected` is the `Ready` of a select-before-operate model, held by its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing is selected or pending.
    Idle,
    /// The object is selected by its owner.
    Selected,
    /// The select request is checked (PerformTest of a selection).
    SelectTest,
    /// The enhanced selection waits for its check (WaitForSelection).
    SelectWait,
    /// The operation is checked (PerformTest of an operation).
    OperTest,
    /// The dynamic test of the operation runs (WaitForExecution).
    DynTest,
    /// The output is activated and the change of the status is awaited (WaitForChange, enhanced models only).
    Change,
}

/// The sequence of a selection or of an operation: its origin, its test flag and its control value. The first request
/// that carries each of them fixes it. A normal-security selection carries none of them (its select is a read of `SBO`), so
/// its sequence is unspecified until its operation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sequence {
    ctl_val: Option<Data>,
    origin: Option<Originator>,
    test: Option<bool>,
}

impl Sequence {
    /// The sequence of a request that carries its parameters.
    fn of(parameters: &Parameters) -> Self {
        Self {
            ctl_val: parameters.ctl_val.clone(),
            origin: Some(parameters.origin.clone()),
            test: Some(parameters.test),
        }
    }

    /// The sequence of a selection that carries no parameters.
    fn unspecified() -> Self {
        Self {
            ctl_val: None,
            origin: None,
            test: None,
        }
    }

    /// Whether a request belongs to this sequence: the same origin, test flag and value, where the sequence has them.
    fn accepts(&self, parameters: &Parameters) -> bool {
        self.origin
            .as_ref()
            .is_none_or(|origin| *origin == parameters.origin)
            && self.test.is_none_or(|test| test == parameters.test)
            && self
                .ctl_val
                .as_ref()
                .is_none_or(|value| parameters.ctl_val.as_ref() == Some(value))
    }

    /// The sequence with the parameters of a request, for those it does not have yet.
    fn adopting(&self, parameters: &Parameters) -> Self {
        let mut next = self.clone();
        if next.ctl_val.is_none() {
            next.ctl_val.clone_from(&parameters.ctl_val);
        }
        if next.origin.is_none() {
            next.origin = Some(parameters.origin.clone());
        }
        if next.test.is_none() {
            next.test = Some(parameters.test);
        }
        next
    }
}

/// The request that a command waits on: its client, its service, its sequence, its checks, and the check it waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    client: ClientId,
    service: Service,
    sequence: Sequence,
    check: CheckConditions,
    kind: CheckKind,
}

/// A control object: its configuration, its state, the owner of its selection, and the command it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlObject {
    config: Config,
    state: State,
    owner: Option<ClientId>,
    sequence: Option<Sequence>,
    pending: Option<Pending>,
}

fn respond(client: ClientId, service: Service, outcome: Result<(), Refusal>) -> Action {
    Action::Respond {
        client,
        service,
        outcome,
    }
}

fn refuse(cause: AddCause) -> Result<(), Refusal> {
    Err(Refusal::Cause(cause))
}

impl ControlObject {
    /// A control object of this configuration, in its idle state, with no owner.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: State::Idle,
            owner: None,
            sequence: None,
            pending: None,
        }
    }

    /// The state of the object.
    pub fn state(&self) -> State {
        self.state
    }

    /// The client that holds the selection, if any.
    pub fn owner(&self) -> Option<ClientId> {
        self.owner
    }

    /// The configuration of the object.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Handles one input at `now`, the time from the start of the machine, and returns the actions to take.
    pub fn handle(&mut self, now: Duration, event: Event) -> Vec<Action> {
        match event {
            Event::Request {
                service,
                parameters,
            } => self.request(now, service, parameters),
            Event::Verdict { ok, cause } => self.verdict(now, ok, cause),
            Event::Changed => self.changed(now),
            Event::Deferred { outcome } => self.deferred(now, outcome),
            Event::Timer(timer) => self.timer(now, timer),
            Event::Behaviour(behaviour) => {
                self.config.behaviour = behaviour;
                Vec::new()
            }
            Event::AssociationLost(client) => self.lost(client),
        }
    }

    fn request(&mut self, now: Duration, service: Service, parameters: Parameters) -> Vec<Action> {
        let client = parameters.client;
        let model = self.config.model;
        // The requests of another model are refused before the state is looked at (Table 108, last row).
        match (model.selects(), service) {
            (false, Service::Select | Service::SelectWithValue) => {
                return vec![respond(client, service, Err(Refusal::ClassNotSupported))];
            }
            (true, Service::SelectWithValue) if !model.enhanced() => {
                return vec![respond(client, service, Err(Refusal::NotInModel))];
            }
            (true, Service::Select) if model.enhanced() => {
                return vec![respond(client, service, Err(Refusal::NotInModel))];
            }
            _ => {}
        }
        match self.state {
            State::Idle => self.idle_request(now, service, parameters),
            State::Selected => self.selected_request(now, service, parameters),
            State::SelectTest
            | State::SelectWait
            | State::OperTest
            | State::DynTest
            | State::Change => self.during_command(service, parameters),
        }
    }

    fn idle_request(
        &mut self,
        now: Duration,
        service: Service,
        parameters: Parameters,
    ) -> Vec<Action> {
        let client = parameters.client;
        match service {
            Service::Select | Service::SelectWithValue => {
                // An enhanced selection (SelectWithValue) carries its control value.
                if service == Service::SelectWithValue && parameters.ctl_val.is_none() {
                    return vec![respond(
                        client,
                        service,
                        refuse(AddCause::INCONSISTENT_PARAMETERS),
                    )];
                }
                // A normal-security select is a read without parameters: its sequence is fixed by its operation.
                let sequence = if self.config.model.enhanced() {
                    Sequence::of(&parameters)
                } else {
                    Sequence::unspecified()
                };
                let check = parameters.check;
                let ctl_val = parameters.ctl_val.clone();
                self.pending = Some(Pending {
                    client,
                    service,
                    sequence: sequence.clone(),
                    check,
                    kind: CheckKind::Select,
                });
                self.sequence = Some(sequence);
                self.state = State::SelectTest;
                vec![Action::Check {
                    kind: CheckKind::Select,
                    check,
                    ctl_val,
                }]
            }
            Service::Operate => {
                if self.config.model.selects() {
                    return vec![respond(
                        client,
                        service,
                        refuse(AddCause::OBJECT_NOT_SELECTED),
                    )];
                }
                if parameters.ctl_val.is_none() {
                    return vec![respond(
                        client,
                        service,
                        refuse(AddCause::INCONSISTENT_PARAMETERS),
                    )];
                }
                self.start_operation(now, service, parameters)
            }
            Service::Cancel => {
                if self.config.model.selects() {
                    // Nothing is selected: the goal of the cancel is reached (Table 108).
                    vec![respond(client, service, Ok(()))]
                } else {
                    vec![respond(client, service, refuse(AddCause::NOT_SUPPORTED))]
                }
            }
        }
    }

    /// Starts the operation of a request: its sequence is the request's, and its operative test is asked for. `now` is
    /// not needed here; the timers start with the dynamic test.
    fn start_operation(
        &mut self,
        _now: Duration,
        service: Service,
        parameters: Parameters,
    ) -> Vec<Action> {
        let sequence = Sequence::of(&parameters);
        let check = parameters.check;
        let action = Action::Check {
            kind: CheckKind::Operative,
            check,
            ctl_val: parameters.ctl_val.clone(),
        };
        self.pending = Some(Pending {
            client: parameters.client,
            service,
            sequence: sequence.clone(),
            check,
            kind: CheckKind::Operative,
        });
        self.sequence = Some(sequence);
        self.state = State::OperTest;
        vec![action]
    }

    fn selected_request(
        &mut self,
        now: Duration,
        service: Service,
        parameters: Parameters,
    ) -> Vec<Action> {
        let client = parameters.client;
        if self.owner != Some(client) {
            // Only the owner of the selection may act on the object (Table 108, Ready).
            let cause = match service {
                Service::Select | Service::SelectWithValue => AddCause::OBJECT_ALREADY_SELECTED,
                Service::Operate | Service::Cancel => AddCause::LOCKED_BY_OTHER_CLIENT,
            };
            return vec![respond(client, service, refuse(cause))];
        }
        match service {
            Service::Select | Service::SelectWithValue => {
                vec![respond(
                    client,
                    service,
                    refuse(AddCause::OBJECT_ALREADY_SELECTED),
                )]
            }
            Service::Operate => {
                let Some(sequence) = self.sequence.clone() else {
                    return vec![respond(
                        client,
                        service,
                        refuse(AddCause::OBJECT_NOT_SELECTED),
                    )];
                };
                if parameters.ctl_val.is_none() || !sequence.accepts(&parameters) {
                    return vec![respond(
                        client,
                        service,
                        refuse(AddCause::INCONSISTENT_PARAMETERS),
                    )];
                }
                // The selection is in use by the operation: its deselect timer stops.
                let adopted = sequence.adopting(&parameters);
                let mut actions = vec![Action::Disarm {
                    timer: Timer::Deselect,
                }];
                actions.extend(self.start_operation_with(now, service, &parameters, adopted));
                actions
            }
            Service::Cancel => {
                let Some(sequence) = self.sequence.clone() else {
                    return vec![respond(client, service, Ok(()))];
                };
                if !sequence.accepts(&parameters) {
                    return vec![respond(
                        client,
                        service,
                        refuse(AddCause::INCONSISTENT_PARAMETERS),
                    )];
                }
                self.release();
                vec![
                    respond(client, service, Ok(())),
                    Action::Disarm {
                        timer: Timer::Deselect,
                    },
                ]
            }
        }
    }

    /// As [`start_operation`](Self::start_operation), with the sequence of the selection adopting the value of the request.
    fn start_operation_with(
        &mut self,
        now: Duration,
        service: Service,
        parameters: &Parameters,
        sequence: Sequence,
    ) -> Vec<Action> {
        let check = parameters.check;
        let action = Action::Check {
            kind: CheckKind::Operative,
            check,
            ctl_val: sequence.ctl_val.clone(),
        };
        self.pending = Some(Pending {
            client: parameters.client,
            service,
            sequence: sequence.clone(),
            check,
            kind: CheckKind::Operative,
        });
        self.sequence = Some(sequence);
        self.state = State::OperTest;
        let _ = now;
        vec![action]
    }

    fn during_command(&mut self, service: Service, parameters: Parameters) -> Vec<Action> {
        let client = parameters.client;
        let Some(pending) = self.pending.clone() else {
            return vec![respond(client, service, refuse(AddCause::UNKNOWN))];
        };
        match service {
            Service::Select | Service::SelectWithValue | Service::Operate => {
                vec![respond(
                    client,
                    service,
                    refuse(AddCause::COMMAND_ALREADY_IN_EXECUTION),
                )]
            }
            Service::Cancel => match self.state {
                // A selection in progress has no owner yet, and an enhanced command in its change is not cancelled
                // (Table 108): both are refused as already in execution.
                State::SelectTest | State::SelectWait | State::Change => {
                    vec![respond(
                        client,
                        service,
                        refuse(AddCause::COMMAND_ALREADY_IN_EXECUTION),
                    )]
                }
                State::OperTest | State::DynTest => {
                    // The operation's client cancels it: any other client is refused (Table 108).
                    if client != pending.client {
                        return vec![respond(
                            client,
                            service,
                            refuse(AddCause::LOCKED_BY_OTHER_CLIENT),
                        )];
                    }
                    if !pending.sequence.accepts(&parameters) {
                        return vec![respond(
                            client,
                            service,
                            refuse(AddCause::INCONSISTENT_PARAMETERS),
                        )];
                    }
                    // The operation is aborted: the cancel is positive, the operation is answered negatively, and the
                    // selection ends with it.
                    let mut actions = vec![
                        respond(client, service, Ok(())),
                        respond(
                            pending.client,
                            pending.service,
                            refuse(AddCause::ABORTION_BY_CANCEL),
                        ),
                    ];
                    if self.state == State::DynTest {
                        actions.push(Action::Disarm {
                            timer: Timer::DynamicTest,
                        });
                    }
                    self.release();
                    actions
                }
                State::Idle | State::Selected => {
                    vec![respond(client, service, refuse(AddCause::UNKNOWN))]
                }
            },
        }
    }

    fn verdict(&mut self, now: Duration, ok: bool, cause: AddCause) -> Vec<Action> {
        let Some(pending) = self.pending.clone() else {
            return Vec::new();
        };
        match (self.state, pending.kind) {
            (State::SelectTest, CheckKind::Select) => {
                if !ok {
                    self.pending = None;
                    self.sequence = None;
                    self.state = State::Idle;
                    return vec![respond(pending.client, pending.service, refuse(cause))];
                }
                if self.config.model.enhanced() {
                    // The enhanced selection waits for its own check (WaitForSelection).
                    let check = pending.check;
                    let ctl_val = pending.sequence.ctl_val.clone();
                    self.pending = Some(Pending {
                        kind: CheckKind::Selection,
                        ..pending
                    });
                    self.state = State::SelectWait;
                    vec![Action::Check {
                        kind: CheckKind::Selection,
                        check,
                        ctl_val,
                    }]
                } else {
                    self.select(now, pending)
                }
            }
            (State::SelectWait, CheckKind::Selection) => {
                if ok {
                    self.select(now, pending)
                } else {
                    self.pending = None;
                    self.sequence = None;
                    self.state = State::Idle;
                    vec![respond(pending.client, pending.service, refuse(cause))]
                }
            }
            (State::OperTest, CheckKind::Operative) => {
                if !ok {
                    // A failed check ends the command and the selection (Figures 42 and 44).
                    let mut actions = vec![respond(pending.client, pending.service, refuse(cause))];
                    self.end_selection(&mut actions);
                    return actions;
                }
                let check = pending.check;
                let ctl_val = pending.sequence.ctl_val.clone();
                self.pending = Some(Pending {
                    kind: CheckKind::Dynamic,
                    ..pending
                });
                self.state = State::DynTest;
                vec![
                    Action::Arm {
                        timer: Timer::DynamicTest,
                        at: now.saturating_add(self.config.dynamic_test_timeout),
                    },
                    Action::Check {
                        kind: CheckKind::Dynamic,
                        check,
                        ctl_val,
                    },
                ]
            }
            (State::DynTest, CheckKind::Dynamic) => {
                let mut actions = vec![Action::Disarm {
                    timer: Timer::DynamicTest,
                }];
                if !ok {
                    actions.push(respond(pending.client, pending.service, refuse(cause)));
                    self.release();
                    return actions;
                }
                // The behaviour of the logical node decides what the command does (7-2 §20.5.2, `Beh`).
                match self.config.behaviour {
                    Behaviour::Blocked | Behaviour::Off => {
                        actions.push(respond(
                            pending.client,
                            pending.service,
                            refuse(AddCause::BLOCKED_BY_MODE),
                        ));
                        self.release();
                    }
                    Behaviour::On | Behaviour::Test => {
                        // The positive answer is sent before the output is activated (Figures 40 and 43).
                        actions.push(respond(pending.client, pending.service, Ok(())));
                        if let Some(ctl_val) = pending.sequence.ctl_val.clone() {
                            actions.push(Action::Execute { ctl_val });
                        }
                        self.after_execution(now, pending, &mut actions);
                    }
                    Behaviour::TestBlocked => {
                        actions.push(respond(pending.client, pending.service, Ok(())));
                        if let Some(ctl_val) = pending.sequence.ctl_val.clone() {
                            actions.push(Action::Mirror { ctl_val });
                        }
                        if self.config.model.enhanced() {
                            actions.push(Action::Terminate {
                                client: pending.client,
                                outcome: Err(AddCause::BLOCKED_BY_MODE),
                            });
                        }
                        self.finish(now, &mut actions);
                    }
                }
                actions
            }
            _ => Vec::new(),
        }
    }

    /// The selection is made: its owner holds it, and its deselect timer runs.
    fn select(&mut self, now: Duration, pending: Pending) -> Vec<Action> {
        self.owner = Some(pending.client);
        self.sequence = Some(pending.sequence.clone());
        self.pending = None;
        self.state = State::Selected;
        vec![
            respond(pending.client, pending.service, Ok(())),
            Action::Arm {
                timer: Timer::Deselect,
                at: now.saturating_add(self.config.sbo_timeout),
            },
        ]
    }

    /// The output is activated: an enhanced model waits for the change of the status, the others are done.
    fn after_execution(&mut self, now: Duration, pending: Pending, actions: &mut Vec<Action>) {
        if self.config.model.enhanced() {
            self.pending = Some(pending);
            self.state = State::Change;
            actions.push(Action::Arm {
                timer: Timer::Operate,
                at: now.saturating_add(self.config.oper_timeout),
            });
        } else {
            self.finish(now, actions);
        }
    }

    /// The outcome of a command that the application reports, when it is not a change of status (CTL-003): a positive outcome
    /// completes the command as a change does; a negative one terminates it with its cause, and the selection ends.
    fn deferred(&mut self, now: Duration, outcome: Result<(), AddCause>) -> Vec<Action> {
        if self.state != State::Change {
            return Vec::new();
        }
        match outcome {
            Ok(()) => self.changed(now),
            Err(cause) => {
                let Some(pending) = self.pending.take() else {
                    return Vec::new();
                };
                let mut actions = vec![
                    Action::Terminate {
                        client: pending.client,
                        outcome: Err(cause),
                    },
                    Action::Disarm {
                        timer: Timer::Operate,
                    },
                ];
                self.finish(now, &mut actions);
                actions
            }
        }
    }

    /// The status changed to the demanded value: the command is terminated positively.
    fn changed(&mut self, now: Duration) -> Vec<Action> {
        if self.state != State::Change {
            return Vec::new();
        }
        let Some(pending) = self.pending.take() else {
            return Vec::new();
        };
        let mut actions = vec![
            Action::Terminate {
                client: pending.client,
                outcome: Ok(()),
            },
            Action::Disarm {
                timer: Timer::Operate,
            },
        ];
        self.finish(now, &mut actions);
        actions
    }

    fn timer(&mut self, now: Duration, timer: Timer) -> Vec<Action> {
        match (timer, self.state) {
            (Timer::Deselect, State::Selected) => {
                self.release();
                Vec::new()
            }
            (Timer::DynamicTest, State::DynTest) => {
                let Some(pending) = self.pending.clone() else {
                    return Vec::new();
                };
                let mut actions = vec![respond(
                    pending.client,
                    pending.service,
                    refuse(AddCause::TIME_LIMIT_OVER),
                )];
                self.end_selection(&mut actions);
                actions
            }
            (Timer::Operate, State::Change) => {
                let Some(pending) = self.pending.clone() else {
                    return Vec::new();
                };
                let mut actions = vec![Action::Terminate {
                    client: pending.client,
                    outcome: Err(AddCause::TIME_LIMIT_OVER),
                }];
                self.finish(now, &mut actions);
                actions
            }
            _ => Vec::new(),
        }
    }

    /// The association of a client was lost: its selection ends, and a command of its own is abandoned.
    fn lost(&mut self, client: ClientId) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.owner == Some(client) && self.state == State::Selected {
            self.release();
            actions.push(Action::Disarm {
                timer: Timer::Deselect,
            });
            return actions;
        }
        let Some(pending) = self.pending.clone() else {
            return actions;
        };
        if pending.client != client {
            return actions;
        }
        match self.state {
            State::Change => {
                actions.push(Action::Terminate {
                    client,
                    outcome: Err(AddCause::ABORTION_BY_COMMUNICATION_LOSS),
                });
                actions.push(Action::Disarm {
                    timer: Timer::Operate,
                });
                self.release();
            }
            State::DynTest => {
                actions.push(Action::Disarm {
                    timer: Timer::DynamicTest,
                });
                self.release();
            }
            _ => self.release(),
        }
        actions
    }

    /// Ends the selection of a select-before-operate model, disarming its deadline; a direct model has none.
    fn end_selection(&mut self, actions: &mut Vec<Action>) {
        if self.config.model.selects() {
            actions.push(Action::Disarm {
                timer: Timer::Deselect,
            });
        }
        self.release();
    }

    /// The selection is released: the owner and the sequence are cleared, and the object is idle.
    fn release(&mut self) {
        self.owner = None;
        self.sequence = None;
        self.pending = None;
        self.state = State::Idle;
    }

    /// The command succeeded: an operate-many selection stays with its owner and its deadline runs again; anything else
    /// is released.
    fn finish(&mut self, now: Duration, actions: &mut Vec<Action>) {
        self.pending = None;
        if self.config.model.selects()
            && self.owner.is_some()
            && self.config.sbo_class == SboClass::OperateMany
        {
            self.state = State::Selected;
            actions.push(Action::Arm {
                timer: Timer::Deselect,
                at: now.saturating_add(self.config.sbo_timeout),
            });
        } else {
            self.release();
        }
    }
}
