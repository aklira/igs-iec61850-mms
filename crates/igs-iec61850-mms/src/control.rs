// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The controls of a served model: the MMS mapping of the control services (task K2; CTL-001, CTL-002; IEC 61850-8-1 §20).
//!
//! A controllable data object is served with its control object: the state machine of task K1 for its control model (`ctlModel`,
//! an initial value of the configuration). The services are MMS services on its `CO` components (8-1 table 95):
//!
//! - **Select** is a read of `SBO`: a positive answer is the object reference, a negative one is the NULL value.
//! - **SelectWithValue** is a write of `SBOw`; **Operate** a write of `Oper`; **Cancel** a write of `Cancel`. Each carries the
//!   structure of table 94: `ctlVal`, `origin`, `ctlNum`, `T`, `Test`, `Check` (the optional `operTm` is absent, since
//!   TimeActivatedOperate is not supported).
//! - A negative answer to a write is its access result (table 100) and, for the services of table 95, an `InformationReport` on
//!   `LastApplError` (table 109), which carries the object, the error, the origin, the control number and the additional cause.
//! - A command with an enhanced model ends with a `CommandTermination`: an `InformationReport` on `Oper`, or on `LastApplError`
//!   when it failed. The termination follows the change of the status (`stVal`) for the models that wait for it.
//!
//! The checks of the application (interlocking, synchrocheck) are asked through [`ControlHandler`]; this release answers them
//! synchronously. The deferred outcomes of CTL-003 are the task K3.
//!
//! Known limit of this release: the `LastApplError` of a negative answer is queued and sent with the next poll, after the write
//! response; the standard puts it before. The client matches it with the origin and the control number.

use std::collections::VecDeque;
use std::time::Duration;

use igs_iec61850_mms_core::acsi::{
    boolean_from_data, enumeration_to_data, CheckConditions, Originator,
};
use igs_iec61850_mms_core::catalogue;
use igs_iec61850_mms_core::control::{
    Action, AddCause, Behaviour, CheckKind, ClientId, Config, ControlModel, ControlObject, Event,
    Parameters, Refusal, SboClass, Service, Timer,
};
use igs_iso9506_mms::server::{Outbound, PeerId};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, Data, DataAccessError, Identifier, InformationReport, ObjectName,
    UnconfirmedService, VariableAccessSpecification, VariableSpecification, WriteResponseItemItem,
};

/// The checks of the application: the interlocking and the synchrocheck of a command (CTL-003), and the execution of the output.
pub trait ControlHandler: Send + 'static {
    /// The check asked for `kind` of the object (`object` is its reference). `Err` refuses the command with its cause.
    fn check(
        &mut self,
        object: &str,
        kind: CheckKind,
        ctl_val: Option<&Data>,
    ) -> Result<(), AddCause> {
        let _ = (object, kind, ctl_val);
        Ok(())
    }

    /// The output of the object is activated with `ctl_val`: the application makes the change of its status.
    fn execute(&mut self, object: &str, ctl_val: &Data) {
        let _ = (object, ctl_val);
    }
}

/// A handler that accepts every check and activates nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct AcceptAll;

impl ControlHandler for AcceptAll {}

/// The default times of the control objects, when the configuration gives none (milliseconds).
#[cfg_attr(not(feature = "scl"), allow(dead_code))]
const DEFAULT_SBO_TIMEOUT_MS: u64 = 10_000;
#[cfg_attr(not(feature = "scl"), allow(dead_code))]
const DEFAULT_DYNAMIC_TEST_MS: u64 = 5_000;
#[cfg_attr(not(feature = "scl"), allow(dead_code))]
const DEFAULT_OPER_TIMEOUT_MS: u64 = 3_000;

/// The operation whose termination is awaited: its client, its service, and the parameters of its structure.
#[derive(Debug, Clone)]
struct Operation {
    client: ClientId,
    ctl_val: Option<Data>,
    origin: Originator,
    ctl_num: u8,
    time: Data,
    test: bool,
    check: CheckConditions,
}

/// One controllable data object of the served model: its state machine, its status, and its timers.
struct Object {
    device: String,
    /// The reference of the control object: `device/node$CO$do`.
    reference: String,
    /// The name of the object within its node: `node$CO$do`.
    component: String,
    /// The served name of the status: `node$ST$do$stVal`, whose changes are the changes the machine waits for.
    status: String,
    machine: ControlObject,
    timers: Vec<(Timer, Duration)>,
    operation: Option<Operation>,
}

/// The controls of a served model.
pub(crate) struct Controls {
    objects: Vec<Object>,
    handler: Box<dyn ControlHandler>,
    outbox: VecDeque<Outbound>,
}

/// What a control item of a request is: an object, and the component that the request names.
pub(crate) enum Component {
    Select,
    SelectWithValue,
    Operate,
    Cancel,
}

impl Controls {
    /// The controls, with no object yet: the checks of the application are made by `handler`.
    pub(crate) fn new(handler: Box<dyn ControlHandler>) -> Self {
        Self {
            objects: Vec::new(),
            handler,
            outbox: VecDeque::new(),
        }
    }

    /// Replaces the handler of the checks, keeping the control objects.
    pub(crate) fn set_handler(&mut self, handler: Box<dyn ControlHandler>) {
        self.handler = handler;
    }

    /// Adds the control object of a data object (built from a configuration, the `scl` feature): `device`, its node and data object, and the parameters of its configuration.
    #[cfg_attr(not(feature = "scl"), allow(dead_code))]
    pub(crate) fn add(
        &mut self,
        device: &str,
        node: &str,
        data_object: &str,
        config: Config,
        status: String,
    ) {
        self.objects.push(Object {
            device: device.to_string(),
            reference: format!("{device}/{node}$CO${data_object}"),
            component: format!("{node}$CO${data_object}"),
            status,
            machine: ControlObject::new(config),
            timers: Vec::new(),
            operation: None,
        });
    }

    /// The object at `index`, if there is one.
    fn object(&mut self, index: usize) -> Option<&mut Object> {
        self.objects.get_mut(index)
    }

    /// The reference of the object at `index` (empty when there is none).
    fn reference_of(&self, index: usize) -> String {
        self.objects
            .get(index)
            .map(|object| object.reference.clone())
            .unwrap_or_default()
    }

    /// The control object of a device and a component (`node$CO$do`), by its index.
    fn find(&self, device: &str, component: &str) -> Option<usize> {
        self.objects
            .iter()
            .position(|object| object.device == device && object.component == component)
    }

    /// The control item of a request, if it is one: the object and the component it names (`$SBO`, `$SBOw`, `$Oper`,
    /// `$Cancel`). A read selects, a write carries the other components.
    pub(crate) fn item(&self, name: &ObjectName, read: bool) -> Option<(usize, Component)> {
        let ObjectName::DomainSpecific(object) = name else {
            return None;
        };
        let (component, suffix) = object.item_id.0.rsplit_once('$')?;
        let index = self.find(&object.domain_id.0, component)?;
        let kind = match (read, suffix) {
            (true, "SBO") => Component::Select,
            (false, "SBOw") => Component::SelectWithValue,
            (false, "Oper") => Component::Operate,
            (false, "Cancel") => Component::Cancel,
            _ => return None,
        };
        Some((index, kind))
    }

    /// The answer to the read of `SBO`: a selection of the client. Positive: the object reference; negative: the NULL value.
    pub(crate) fn select_read(
        &mut self,
        now: Duration,
        peer: PeerId,
        index: usize,
    ) -> AccessResult {
        let parameters = empty_parameters(ClientId(peer.0));
        let answer = self.request(now, index, Service::Select, parameters, Data::Unsigned(0));
        let reference = self.reference_of(index);
        match answer {
            Ok(()) => AccessResult::Success(Data::VisibleString(reference)),
            Err(_) => AccessResult::Success(Data::VisibleString(String::new())),
        }
    }

    /// The answer to the write of `SBOw`, `Oper` or `Cancel`: its result, and a `LastApplError` when it is refused.
    pub(crate) fn write(
        &mut self,
        now: Duration,
        peer: PeerId,
        index: usize,
        component: Component,
        data: &Data,
    ) -> WriteResponseItemItem {
        let service = match component {
            Component::SelectWithValue => Service::SelectWithValue,
            Component::Operate => Service::Operate,
            Component::Cancel => Service::Cancel,
            Component::Select => Service::Select,
        };
        let Some((parameters, time)) = decode(data, ClientId(peer.0)) else {
            return WriteResponseItemItem::Failure(DataAccessError::TYPE_INCONSISTENT);
        };
        match self.request(now, index, service, parameters.clone(), time.clone()) {
            Ok(()) => WriteResponseItemItem::Success,
            Err(refusal) => {
                let cause = match &refusal {
                    Refusal::Cause(cause) => *cause,
                    Refusal::ClassNotSupported | Refusal::NotInModel => AddCause::NOT_SUPPORTED,
                };
                self.last_error(index, peer, &parameters, time, cause);
                WriteResponseItemItem::Failure(access_error(&refusal))
            }
        }
    }

    /// A request of a client to an object: the machine answers it, and the actions it asks are carried out.
    fn request(
        &mut self,
        now: Duration,
        index: usize,
        service: Service,
        parameters: Parameters,
        time: Data,
    ) -> Result<(), Refusal> {
        let Some(object) = self.object(index) else {
            return Err(Refusal::NotInModel);
        };
        if matches!(
            service,
            Service::Operate | Service::Cancel | Service::SelectWithValue
        ) {
            object.operation = Some(Operation {
                client: parameters.client,
                ctl_val: parameters.ctl_val.clone(),
                origin: parameters.origin.clone(),
                ctl_num: parameters.ctl_num,
                time,
                test: parameters.test,
                check: parameters.check,
            });
        }
        let actions = object.machine.handle(
            now,
            Event::Request {
                service,
                parameters,
            },
        );
        self.run(now, index, actions).unwrap_or(Ok(()))
    }

    /// Carries out the actions of an object. Returns the answer to the request: the first `Respond` of the actions.
    fn run(
        &mut self,
        now: Duration,
        index: usize,
        actions: Vec<Action>,
    ) -> Option<Result<(), Refusal>> {
        let mut answer = None;
        let mut queue: VecDeque<Action> = actions.into();
        while let Some(action) = queue.pop_front() {
            match action {
                Action::Respond { outcome, .. } => {
                    if answer.is_none() {
                        answer = Some(outcome);
                    }
                }
                Action::Check { kind, ctl_val, .. } => {
                    let reference = self.reference_of(index);
                    let verdict = self.handler.check(&reference, kind, ctl_val.as_ref());
                    let event = Event::Verdict {
                        ok: verdict.is_ok(),
                        cause: verdict.err().unwrap_or(AddCause::UNKNOWN),
                    };
                    let more = match self.object(index) {
                        Some(object) => object.machine.handle(now, event),
                        None => Vec::new(),
                    };
                    for action in more.into_iter().rev() {
                        queue.push_front(action);
                    }
                }
                Action::Execute { ctl_val } => {
                    let reference = self.reference_of(index);
                    self.handler.execute(&reference, &ctl_val);
                }
                Action::Mirror { .. } => {}
                Action::Terminate { outcome, .. } => self.terminate(index, outcome),
                Action::Arm { timer, at } => {
                    if let Some(object) = self.object(index) {
                        object.timers.retain(|(armed, _)| *armed != timer);
                        object.timers.push((timer, at));
                    }
                }
                Action::Disarm { timer } => {
                    if let Some(object) = self.object(index) {
                        object.timers.retain(|(armed, _)| *armed != timer);
                    }
                }
            }
        }
        answer
    }

    /// The end of an operation: the `CommandTermination` (an `Oper` report when it succeeded, a `LastApplError` when it failed).
    fn terminate(&mut self, index: usize, outcome: Result<(), AddCause>) {
        let Some(object) = self.object(index) else {
            return;
        };
        let Some(operation) = object.operation.take() else {
            return;
        };
        let device = object.device.clone();
        let component = object.component.clone();
        let reference = object.reference.clone();
        match outcome {
            Ok(()) => {
                let data = operation_data(&operation);
                let report = information(&device, &format!("{component}$Oper"), data);
                self.queue(operation.client, report);
            }
            Err(cause) => {
                let report = last_error(&reference, &operation, cause);
                self.queue(operation.client, report);
            }
        }
    }

    /// A negative answer to a write: its `LastApplError`, to the client that wrote it.
    fn last_error(
        &mut self,
        index: usize,
        peer: PeerId,
        parameters: &Parameters,
        time: Data,
        cause: AddCause,
    ) {
        let operation = Operation {
            client: ClientId(peer.0),
            ctl_val: parameters.ctl_val.clone(),
            origin: parameters.origin.clone(),
            ctl_num: parameters.ctl_num,
            time,
            test: parameters.test,
            check: parameters.check,
        };
        let reference = self.reference_of(index);
        let report = last_error(&reference, &operation, cause);
        self.queue(ClientId(peer.0), report);
    }

    fn queue(&mut self, client: ClientId, report: UnconfirmedService) {
        self.outbox.push_back(Outbound {
            peer: Some(PeerId(client.0)),
            report,
        });
    }

    /// One poll: the expired timers, the status changes that the machines wait for, and the reports to send.
    pub(crate) fn poll(&mut self, now: Duration, changed: &[String]) -> Vec<Outbound> {
        for index in 0..self.objects.len() {
            while let Some((timer, _)) = self.take_due(index, now) {
                let actions = match self.object(index) {
                    Some(object) => object.machine.handle(now, Event::Timer(timer)),
                    None => Vec::new(),
                };
                self.run(now, index, actions);
            }
        }
        for index in 0..self.objects.len() {
            let status = match self.objects.get(index) {
                Some(object) => format!("{}/{}", object.device, object.status),
                None => continue,
            };
            if changed.contains(&status) {
                let actions = match self.object(index) {
                    Some(object) => object.machine.handle(now, Event::Changed),
                    None => Vec::new(),
                };
                self.run(now, index, actions);
            }
        }
        self.outbox.drain(..).collect()
    }

    /// Removes the first timer of the object at `index` that has expired at `now`.
    fn take_due(&mut self, index: usize, now: Duration) -> Option<(Timer, Duration)> {
        let object = self.object(index)?;
        let position = due(&object.timers, now)?;
        Some(object.timers.remove(position))
    }

    /// The association of a client is lost: the objects it selected are released.
    pub(crate) fn released(&mut self, now: Duration, peer: PeerId) {
        for index in 0..self.objects.len() {
            let actions = match self.object(index) {
                Some(object) => object
                    .machine
                    .handle(now, Event::AssociationLost(ClientId(peer.0))),
                None => Vec::new(),
            };
            self.run(now, index, actions);
        }
    }
}

/// The parameters of a request that carries none (a select read).
fn empty_parameters(client: ClientId) -> Parameters {
    Parameters {
        client,
        ctl_val: None,
        origin: Originator {
            category: 0,
            identity: Vec::new(),
        },
        ctl_num: 0,
        test: false,
        check: CheckConditions {
            synchrocheck: false,
            interlock_check: false,
        },
    }
}

/// The parameters of a write of the structure of table 94 (`ctlVal`, `origin`, `ctlNum`, `T`, `Test`, `Check`), and its `T`.
fn decode(data: &Data, client: ClientId) -> Option<(Parameters, Data)> {
    let Data::Structure(components) = data else {
        return None;
    };
    let [ctl_val, origin, ctl_num, time, test, check] = components.as_slice() else {
        return None;
    };
    let Data::Unsigned(number) = ctl_num else {
        return None;
    };
    let parameters = Parameters {
        client,
        ctl_val: Some(ctl_val.clone()),
        origin: Originator::from_data(origin).ok()?,
        ctl_num: u8::try_from(*number).ok()?,
        test: boolean_from_data(test).ok()?,
        check: CheckConditions::from_data(check).ok()?,
    };
    Some((parameters, time.clone()))
}

/// The structure of an operation, as the `CommandTermination` reports it: the same components as the request.
fn operation_data(operation: &Operation) -> Data {
    Data::Structure(vec![
        operation.ctl_val.clone().unwrap_or(Data::Unsigned(0)),
        operation
            .origin
            .to_data()
            .unwrap_or(Data::Structure(Vec::new())),
        Data::Unsigned(u128::from(operation.ctl_num)),
        operation.time.clone(),
        Data::Boolean(operation.test),
        operation.check.to_data(),
    ])
}

/// The `LastApplError` of an operation that failed (table 109): the object, the error, the origin, the control number, and the
/// additional cause. The error is `Operator Test Not OK` (3) for a test that failed, and `No Error` (0) otherwise.
fn last_error(reference: &str, operation: &Operation, cause: AddCause) -> UnconfirmedService {
    let error = if operation.test { 3 } else { 0 };
    let origin = operation
        .origin
        .to_data()
        .unwrap_or(Data::Structure(Vec::new()));
    let data = Data::Structure(vec![
        Data::VisibleString(reference.to_string()),
        enumeration_to_data(error),
        origin,
        Data::Unsigned(u128::from(operation.ctl_num)),
        enumeration_to_data(i64::from(add_cause_value(cause))),
    ]);
    UnconfirmedService::InformationReport(InformationReport {
        variable_access_specification: VariableAccessSpecification::VariableListName(
            ObjectName::VmdSpecific(Identifier("LastApplError".to_string())),
        ),
        list_of_access_result: vec![AccessResult::Success(data)],
    })
}

/// An `InformationReport` on one named variable of a device, with its value.
fn information(device: &str, component: &str, data: Data) -> UnconfirmedService {
    use igs_iso9506_mms_codec::generated::types::{
        ObjectNameDomainSpecific, VariableAccessSpecificationListOfVariableItem,
    };
    UnconfirmedService::InformationReport(InformationReport {
        variable_access_specification: VariableAccessSpecification::ListOfVariable(vec![
            VariableAccessSpecificationListOfVariableItem {
                variable_specification: VariableSpecification::Name(ObjectName::DomainSpecific(
                    ObjectNameDomainSpecific {
                        domain_id: Identifier(device.to_string()),
                        item_id: Identifier(component.to_string()),
                    },
                )),
                alternate_access: None,
            },
        ]),
        list_of_access_result: vec![AccessResult::Success(data)],
    })
}

/// The value of an additional cause in the catalogue enumeration `ControlServiceStatusKind` (0, Unknown, when it is not there).
fn add_cause_value(cause: AddCause) -> i32 {
    catalogue::enumeration("ControlServiceStatusKind")
        .and_then(|enumeration| {
            enumeration
                .literals
                .iter()
                .find(|literal| literal.name == cause.name())
                .map(|literal| literal.value)
        })
        .unwrap_or(0)
}

/// The access result of a refusal (table 100): a selection in progress is temporarily unavailable, and the other refusals are
/// access denied, except an inconsistent parameter. The table of the prepared text is incomplete; see D-026.
fn access_error(refusal: &Refusal) -> DataAccessError {
    match refusal {
        Refusal::Cause(cause)
            if *cause == AddCause::OBJECT_ALREADY_SELECTED
                || *cause == AddCause::LOCKED_BY_OTHER_CLIENT
                || *cause == AddCause::COMMAND_ALREADY_IN_EXECUTION =>
        {
            DataAccessError::TEMPORARILY_UNAVAILABLE
        }
        Refusal::Cause(cause) if *cause == AddCause::INCONSISTENT_PARAMETERS => {
            DataAccessError::OBJECT_ATTRIBUTE_INCONSISTENT
        }
        _ => DataAccessError::OBJECT_ACCESS_DENIED,
    }
}

/// The first timer that has expired at `now`.
fn due(timers: &[(Timer, Duration)], now: Duration) -> Option<usize> {
    timers.iter().position(|(_, at)| *at <= now)
}

/// The control model of a `ctlModel` value, and the parameters of its configuration (the other values of the configuration).
#[cfg_attr(not(feature = "scl"), allow(dead_code))]
pub(crate) fn control_config(
    ctl_model: i64,
    sbo_class: Option<i64>,
    sbo_timeout: Option<u64>,
    oper_timeout: Option<u64>,
    behaviour: Option<i64>,
) -> Option<Config> {
    let model = ControlModel::from_ctl_model(ctl_model)?;
    Some(Config {
        model,
        sbo_class: sbo_class
            .and_then(SboClass::from_value)
            .unwrap_or(SboClass::OperateOnce),
        sbo_timeout: Duration::from_millis(sbo_timeout.unwrap_or(DEFAULT_SBO_TIMEOUT_MS)),
        dynamic_test_timeout: Duration::from_millis(DEFAULT_DYNAMIC_TEST_MS),
        oper_timeout: Duration::from_millis(oper_timeout.unwrap_or(DEFAULT_OPER_TIMEOUT_MS)),
        behaviour: behaviour
            .and_then(Behaviour::from_value)
            .unwrap_or(Behaviour::On),
    })
}
