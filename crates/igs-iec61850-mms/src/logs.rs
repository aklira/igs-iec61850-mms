// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The logs of a served device (task X2; IEC 61850-7-2 §17.3; IEC 61850-8-1 §17.3): each log control block of the configuration
//! records the changes of its data set into the MMS journal of its node, which ReadJournal reads. The state machine of the block is in
//! the core (`igs_iec61850_mms_core::logs`); this module gives it its notices and its values, writes its entries (8-1 Table 77 and
//! §17.3.3.4), and serves its components over MMS (8-1 Table 75: the structure `LLN0$LG$name` and its components).

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use igs_iec61850_mms_core::logs::{Action, Content, Event, Lcb, Refusal, Write};
use igs_iec61850_mms_core::model::layout::value_at;
use igs_iec61850_mms_core::report::encode::{
    binary_time, reason_bits, triggers_from_bits, triggers_to_bits,
};
use igs_iec61850_mms_core::report::urcb::{Reason, Timer, Trigger};
use igs_iec61850_mms_core::scl::build::{Configuration, DataSet, LogDecl};
use igs_iso9506_mms::vmd::{Target, Vmd};
use igs_iso9506_mms_codec::generated::types::{
    AccessResult, ApplicationReference, Data, DataAccessError, ECState, EntryContent,
    EntryContentEntryForm, EntryContentEntryFormData, EntryContentEntryFormDataEvent, Identifier,
    Integer32, JournalEntry, JournalVariable, MMS255String, ObjectName, ObjectNameDomainSpecific,
    TimeOfDay, TypeDescription, TypeDescriptionStructure, TypeDescriptionStructureComponentsItem,
    TypeSpecification, Unsigned8, WriteResponseItemItem,
};

use crate::model::{NodeValue, ServeError};
use crate::setting_groups::ListedVariable;
use crate::updates::{Notice, Updater};

/// The entries that a journal holds before its oldest is overwritten: a bounded buffer, as the log of 7-2 §17.3.1.2.
const JOURNAL_CAPACITY: usize = 512;

/// The days from 1970-01-01 to 1984-01-01: the time of day counts its day from 1 January 1984 (the six-octet time of 8-1).
const DAYS_1970_TO_1984: u64 = 5_113;

/// The length of an object reference, as the visible string of a reference holds it (8-1 §8.1.3.2.2).
const REFERENCE_LENGTH: i64 = 129;

/// A component of the log control block, as 8-1 Table 75 maps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Component {
    LogEna,
    LogRef,
    DatSet,
    OldEntrTm,
    NewEntrTm,
    OldEnt,
    NewEnt,
    TrgOps,
    IntgPd,
}

/// The components with their MMS names, in the order of the structure (Table 75: `OldEntr` and `NewEntr` are `OldEnt` and `NewEnt`).
const COMPONENTS: [(Component, &str); 9] = [
    (Component::LogEna, "LogEna"),
    (Component::LogRef, "LogRef"),
    (Component::DatSet, "DatSet"),
    (Component::OldEntrTm, "OldEntrTm"),
    (Component::NewEntrTm, "NewEntrTm"),
    (Component::OldEnt, "OldEnt"),
    (Component::NewEnt, "NewEnt"),
    (Component::TrgOps, "TrgOps"),
    (Component::IntgPd, "IntgPd"),
];

/// What an item names, for a log control block: the whole structure, or one of its components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Locus {
    Structure,
    Component(Component),
}

/// The MMS type of a component (Table 75).
fn component_type(component: Component) -> TypeDescription {
    match component {
        Component::LogEna => TypeDescription::Boolean,
        Component::LogRef | Component::DatSet => {
            TypeDescription::VisibleString(Integer32(REFERENCE_LENGTH))
        }
        Component::OldEntrTm | Component::NewEntrTm => TypeDescription::BinaryTime(true),
        Component::OldEnt | Component::NewEnt => TypeDescription::OctetString(Integer32(8)),
        Component::TrgOps => TypeDescription::BitString(Integer32(6)),
        Component::IntgPd => TypeDescription::Unsigned(Unsigned8(32)),
    }
}

/// Whether a client writes a component (Table 75: the times and the entries are read-only).
fn component_writable(component: Component) -> bool {
    matches!(
        component,
        Component::LogEna
            | Component::LogRef
            | Component::DatSet
            | Component::TrgOps
            | Component::IntgPd
    )
}

/// A member of the data set of a log control block: its served name, and its index in the data set (the reference of the entries).
#[derive(Debug, Clone)]
struct Member {
    name: String,
    index: usize,
}

/// A log control block, with its journal, its members and the identifier of its next entry.
struct Block {
    lcb: Lcb,
    /// The logical device of the block.
    device: String,
    /// The logical node of the block.
    node: String,
    /// The name of the block in MMS (`LN$LG$name`): its structure, and the event condition of its transitions.
    lcb_name: String,
    /// The journal the block records into (`LN$name` of the `Log` it names).
    journal: Target,
    members: Vec<Member>,
    next_id: u64,
    /// The instant of the integrity period, when one is armed.
    integrity: Option<Duration>,
}

/// The log control blocks of a served device, the data sets they may record, and the entries that the journals refused.
#[derive(Default)]
pub(crate) struct Logs {
    blocks: Vec<Block>,
    /// The members of each data set of the configuration, by device and by the node-qualified name of the set (`LLN0$DS1`).
    sets: Vec<(String, String, Vec<Member>)>,
    refused: u64,
}

impl Logs {
    /// Declares the journals and the log control blocks of the configuration in the VMD, and starts the blocks that the file enables.
    pub(crate) fn build(
        configuration: &Configuration,
        vmd: &mut Vmd,
        updater: &Updater,
    ) -> Result<Self, ServeError> {
        for journal in &configuration.journals {
            let domain = vmd.name(&journal.device)?;
            let item = vmd.name(&format!("{}${}", journal.node, journal.name))?;
            vmd.define_journal(&Target::in_domain(domain, item), JOURNAL_CAPACITY)?;
        }
        let sets = configuration
            .data_sets
            .iter()
            .map(|set| (set.device.clone(), set.name.clone(), members_of(set)))
            .collect();
        let mut logs = Self {
            blocks: Vec::new(),
            sets,
            refused: 0,
        };
        for declared in &configuration.logs {
            let members = logs.members(&declared.device, declared.config.data_set.as_deref());
            let mut block = block(declared, members, vmd)?;
            if declared.log_ena {
                let actions = block
                    .lcb
                    .handle(Duration::ZERO, Event::Request(Write::LogEna(true)));
                apply(&mut block, actions, updater, vmd, &mut logs.refused);
            }
            logs.blocks.push(block);
        }
        Ok(logs)
    }

    /// The members of a data set of a device, by its node-qualified name (none when the device has no such set).
    fn members(&self, device: &str, data_set: Option<&str>) -> Vec<Member> {
        let Some(name) = data_set else {
            return Vec::new();
        };
        self.sets
            .iter()
            .find(|(set_device, set_name, _)| set_device == device && set_name == name)
            .map(|(_, _, members)| members.clone())
            .unwrap_or_default()
    }

    /// Declares the structure of each block and its components as named variables of its device: GetNameList lists them, and their
    /// reads and writes are answered here, before the VMD sees them.
    pub(crate) fn list(&self, vmd: &mut Vmd) -> Result<(), ServeError> {
        for block in &self.blocks {
            let domain = vmd.name(&block.device)?;
            let components = COMPONENTS
                .iter()
                .map(|(component, name)| TypeDescriptionStructureComponentsItem {
                    component_name: Some(Identifier((*name).to_string())),
                    component_type: TypeSpecification::TypeDescription(Box::new(component_type(
                        *component,
                    ))),
                })
                .collect();
            let structure = TypeDescription::Structure(TypeDescriptionStructure {
                packed: None,
                components,
            });
            let item = vmd.name(&block.lcb_name)?;
            vmd.add_variable(
                &Target::in_domain(domain.clone(), item),
                Box::new(ListedVariable {
                    ty: structure,
                    writable: false,
                }),
            )?;
            for (component, name) in COMPONENTS {
                let item = vmd.name(&format!("{}${name}", block.lcb_name))?;
                vmd.add_variable(
                    &Target::in_domain(domain.clone(), item),
                    Box::new(ListedVariable {
                        ty: component_type(component),
                        writable: component_writable(component),
                    }),
                )?;
            }
        }
        Ok(())
    }

    /// Feeds the notices of the updates to the blocks, and fires their integrity periods when they are due. `now` is the instant of the
    /// poll on the monotonic clock of the server.
    pub(crate) fn poll(
        &mut self,
        now: Duration,
        notices: &[Notice],
        vmd: &mut Vmd,
        updater: &Updater,
    ) {
        for block in &mut self.blocks {
            if block.integrity.is_some_and(|at| now >= at) {
                block.integrity = None;
                let actions = block.lcb.handle(now, Event::Timer(Timer::Integrity));
                apply(block, actions, updater, vmd, &mut self.refused);
            }
            for notice in notices {
                let trigger = if notice.changed {
                    Trigger::DataChange
                } else {
                    Trigger::DataUpdate
                };
                let indices: Vec<usize> = block
                    .members
                    .iter()
                    .filter(|member| member.name == notice.name)
                    .map(|member| member.index)
                    .collect();
                for index in indices {
                    let actions = block.lcb.handle(
                        now,
                        Event::Notification {
                            member: index,
                            trigger,
                        },
                    );
                    apply(block, actions, updater, vmd, &mut self.refused);
                }
            }
        }
    }

    /// What an item names, when it is a log control block of a device (`LN$LG$name`, or one of its components).
    pub(crate) fn locate(&self, domain: &str, item: &str) -> Option<(usize, Locus)> {
        self.blocks.iter().enumerate().find_map(|(index, block)| {
            if block.device != domain {
                return None;
            }
            if item == block.lcb_name {
                return Some((index, Locus::Structure));
            }
            let rest = item
                .strip_prefix(block.lcb_name.as_str())?
                .strip_prefix('$')?;
            COMPONENTS
                .iter()
                .find(|(_, name)| *name == rest)
                .map(|(component, _)| (index, Locus::Component(*component)))
        })
    }

    /// A read of a block or of one of its components (GetLCBValues, 8-1 §17.3.4). The values are the state of the block and the first
    /// and last entries of its journal; an empty journal gives the NULL times and identifiers, which are zero.
    pub(crate) fn read(&self, vmd: &Vmd, index: usize, locus: Locus) -> AccessResult {
        let Some(block) = self.blocks.get(index) else {
            return AccessResult::Failure(DataAccessError::OBJECT_NON_EXISTENT);
        };
        match locus {
            Locus::Structure => AccessResult::Success(Data::Structure(
                COMPONENTS
                    .iter()
                    .map(|(component, _)| value(block, vmd, *component))
                    .collect(),
            )),
            Locus::Component(component) => AccessResult::Success(value(block, vmd, component)),
        }
    }

    /// A write of a component of a block (SetLCBValues, 8-1 §17.3.4). The state machine of the block takes it, and its refusal is the
    /// answer. A log reference names a journal of the node; a data set reference names a data set of the device.
    pub(crate) fn write(
        &mut self,
        now: Duration,
        vmd: &mut Vmd,
        updater: &Updater,
        index: usize,
        component: Component,
        data: &Data,
    ) -> WriteResponseItemItem {
        let refused = &mut self.refused;
        let sets = &self.sets;
        let Some(block) = self.blocks.get_mut(index) else {
            return WriteResponseItemItem::Failure(DataAccessError::OBJECT_NON_EXISTENT);
        };
        if !component_writable(component) {
            return WriteResponseItemItem::Failure(DataAccessError::OBJECT_ACCESS_DENIED);
        }
        match (component, data) {
            (Component::LogEna, Data::Boolean(enabled)) => {
                answer(block, now, vmd, updater, refused, Write::LogEna(*enabled))
            }
            (Component::TrgOps, Data::BitString(bits)) => answer(
                block,
                now,
                vmd,
                updater,
                refused,
                Write::TrgOps(triggers_from_bits(bits)),
            ),
            (Component::IntgPd, Data::Unsigned(millis)) => answer(
                block,
                now,
                vmd,
                updater,
                refused,
                Write::IntgPd(Duration::from_millis(
                    u64::try_from(*millis).unwrap_or(u64::MAX),
                )),
            ),
            (Component::LogRef, Data::VisibleString(text)) => {
                let Some(name) = log_name(text) else {
                    return answer(block, now, vmd, updater, refused, Write::LogRef(None));
                };
                let Some(target) = journal_target(vmd, &block.device, &block.node, &name) else {
                    return WriteResponseItemItem::Failure(DataAccessError::OBJECT_VALUE_INVALID);
                };
                let result = answer(block, now, vmd, updater, refused, Write::LogRef(Some(name)));
                if matches!(result, WriteResponseItemItem::Success) {
                    block.journal = target;
                }
                result
            }
            (Component::DatSet, Data::VisibleString(text)) => {
                let Some(name) = data_set_name(text) else {
                    return answer(block, now, vmd, updater, refused, Write::DataSet(None));
                };
                let members = sets
                    .iter()
                    .find(|(device, set, _)| *device == block.device && *set == name)
                    .map(|(_, _, members)| members.clone());
                let result = answer(
                    block,
                    now,
                    vmd,
                    updater,
                    refused,
                    Write::DataSet(Some(name)),
                );
                if matches!(result, WriteResponseItemItem::Success) {
                    block.members = members.unwrap_or_default();
                }
                result
            }
            _ => WriteResponseItemItem::Failure(DataAccessError::OBJECT_VALUE_INVALID),
        }
    }
}

/// The part of a reference after its device: `LD1/LLN0$DS1` gives `LLN0$DS1`, and a reference without a device is itself.
fn reference_tail(text: &str) -> &str {
    text.rsplit('/').next().unwrap_or_default()
}

/// The name of a log in its node: `LD1/LLN0$C1` names `C1`. An empty reference names nothing (NULL).
fn log_name(text: &str) -> Option<String> {
    let name = reference_tail(text).rsplit('$').next().unwrap_or_default();
    (!name.is_empty()).then(|| name.to_string())
}

/// The node-qualified name of a data set: `LD1/LLN0$DS1` gives `LLN0$DS1`. An empty reference names nothing (NULL).
fn data_set_name(text: &str) -> Option<String> {
    let name = reference_tail(text);
    (!name.is_empty()).then(|| name.to_string())
}

/// The journal of a log of a node, when the VMD holds it.
fn journal_target(vmd: &Vmd, device: &str, node: &str, name: &str) -> Option<Target> {
    let domain = vmd.name(device).ok()?;
    let item = vmd.name(&format!("{node}${name}")).ok()?;
    let target = Target::in_domain(domain, item);
    vmd.journal(&target).ok()?;
    Some(target)
}

/// Hands a write to the state machine of its block, and turns its answer into the answer of the write.
fn answer(
    block: &mut Block,
    now: Duration,
    vmd: &mut Vmd,
    updater: &Updater,
    refused: &mut u64,
    write: Write,
) -> WriteResponseItemItem {
    let actions = block.lcb.handle(now, Event::Request(write));
    let respond = actions.iter().find_map(|action| match action {
        Action::Respond(result) => Some(*result),
        _ => None,
    });
    apply(block, actions, updater, vmd, refused);
    match respond {
        None | Some(Ok(())) => WriteResponseItemItem::Success,
        Some(Err(Refusal::Enabled)) => {
            WriteResponseItemItem::Failure(DataAccessError::OBJECT_ACCESS_DENIED)
        }
        Some(Err(_)) => WriteResponseItemItem::Failure(DataAccessError::OBJECT_VALUE_INVALID),
    }
}

/// The value of a component of a block: the state of the block, and the first and last entries of its journal.
fn value(block: &Block, vmd: &Vmd, component: Component) -> Data {
    let config = block.lcb.config();
    let journal = vmd.journal(&block.journal).ok();
    let first = journal.and_then(|journal| journal.entries().next());
    let last = journal.and_then(|journal| journal.entries().last());
    match component {
        Component::LogEna => Data::Boolean(block.lcb.is_enabled()),
        Component::LogRef => Data::VisibleString(match config.log_ref.as_deref() {
            Some(name) => format!("{}/{}${name}", block.device, block.node),
            None => String::new(),
        }),
        Component::DatSet => Data::VisibleString(match config.data_set.as_deref() {
            Some(name) => format!("{}/{name}", block.device),
            None => String::new(),
        }),
        Component::OldEntrTm => Data::BinaryTime(first.map_or_else(zero_time, |entry| {
            entry.entry_content.occurrence_time.clone()
        })),
        Component::NewEntrTm => Data::BinaryTime(last.map_or_else(zero_time, |entry| {
            entry.entry_content.occurrence_time.clone()
        })),
        Component::OldEnt => Data::OctetString(
            first.map_or_else(zero_identifier, |entry| entry.entry_identifier.clone()),
        ),
        Component::NewEnt => Data::OctetString(
            last.map_or_else(zero_identifier, |entry| entry.entry_identifier.clone()),
        ),
        Component::TrgOps => Data::BitString(triggers_to_bits(config.trg_ops)),
        Component::IntgPd => Data::Unsigned(u128::from(
            u64::try_from(config.intg_pd.as_millis()).unwrap_or(u64::MAX),
        )),
    }
}

/// The six octets of a time that the journal does not hold: zero, the NULL time.
fn zero_time() -> TimeOfDay {
    TimeOfDay(vec![0; 6])
}

/// The eight octets of an identifier that the journal does not hold: zero, the NULL identifier (8-1 §17.3.3.4.2).
fn zero_identifier() -> Vec<u8> {
    vec![0; 8]
}

/// The members of a data set, with the served name of each variable and its index in the data set.
fn members_of(set: &DataSet) -> Vec<Member> {
    set.variables
        .iter()
        .map(|variable| Member {
            name: format!("{}/{}", variable.device, variable.name),
            index: variable.member,
        })
        .collect()
}

fn block(declared: &LogDecl, members: Vec<Member>, vmd: &mut Vmd) -> Result<Block, ServeError> {
    let domain = vmd.name(&declared.device)?;
    let log = declared.config.log_ref.clone().unwrap_or_default();
    let item = vmd.name(&format!("{}${}", declared.node, log))?;
    Ok(Block {
        lcb: Lcb::new(declared.config.clone()),
        device: declared.device.clone(),
        node: declared.node.clone(),
        lcb_name: format!("{}$LG${}", declared.node, declared.name),
        journal: Target::in_domain(domain, item),
        members,
        next_id: 1,
        integrity: None,
    })
}

/// Writes the entries of the actions of a block, and keeps its integrity period.
fn apply(
    block: &mut Block,
    actions: Vec<Action>,
    updater: &Updater,
    vmd: &mut Vmd,
    refused: &mut u64,
) {
    for action in actions {
        match action {
            Action::Respond(_) => {}
            Action::Record { content, .. } => {
                let entry = entry(block, &content, updater);
                if vmd.append_journal(&block.journal, entry).is_err() {
                    *refused = refused.saturating_add(1);
                }
            }
            Action::Arm { timer, at } => {
                if timer == Timer::Integrity {
                    block.integrity = Some(at);
                }
            }
            Action::Disarm(timer) => {
                if timer == Timer::Integrity {
                    block.integrity = None;
                }
            }
        }
    }
}

/// The MMS journal entry of a record (8-1 Table 77 and §17.3.3.4): its identifier (eight octets, never zero), its time, and either the
/// variables of the members (the reference of each, its value, and its reason codes) or the transition of `LogEna` (EVENT-DATA).
fn entry(block: &mut Block, content: &Content, updater: &Updater) -> JournalEntry {
    let identifier = block.next_id;
    block.next_id = block.next_id.saturating_add(1).max(1);
    let form = match content {
        Content::Members(members) => {
            let mut variables = Vec::with_capacity(members.len().saturating_mul(2));
            for member in members {
                let Some(served) = block
                    .members
                    .iter()
                    .find(|item| item.index == member.member)
                else {
                    continue;
                };
                let Some(value) = current_value(updater, &served.name) else {
                    continue;
                };
                variables.push(JournalVariable {
                    variable_tag: MMS255String(served.name.clone()),
                    value_specification: value,
                });
                let reasons: Vec<Reason> = member.reasons.iter().copied().collect();
                variables.push(JournalVariable {
                    variable_tag: MMS255String("ReasonCode".to_string()),
                    value_specification: Data::BitString(reason_bits(&reasons)),
                });
            }
            EntryContentEntryFormData {
                event: None,
                list_of_variables: Some(variables),
            }
        }
        Content::Transition { enabled, .. } => EntryContentEntryFormData {
            event: Some(EntryContentEntryFormDataEvent {
                event_condition_name: ObjectName::DomainSpecific(ObjectNameDomainSpecific {
                    domain_id: Identifier(block.device.clone()),
                    item_id: Identifier(block.lcb_name.clone()),
                }),
                current_state: if *enabled {
                    ECState::ACTIVE
                } else {
                    ECState::DISABLED
                },
            }),
            list_of_variables: None,
        },
    };
    JournalEntry {
        entry_identifier: identifier.to_be_bytes().to_vec(),
        originating_application: ApplicationReference {
            ap_title: None,
            ap_invocation_id: None,
            ae_qualifier: None,
            ae_invocation_id: None,
        },
        entry_content: EntryContent {
            occurrence_time: time_of_day(),
            entry_form: EntryContentEntryForm::Data(form),
        },
    }
}

/// The current value of a served variable, from the model.
fn current_value(updater: &Updater, name: &str) -> Option<Data> {
    let (root, path): (Arc<Mutex<NodeValue>>, Vec<usize>) = updater.point(name)?;
    let state = root.lock().ok()?;
    value_at(&state.data, &path).cloned()
}

/// The wall-clock time of the record, as the six-octet time of 8-1: the time of day, and the day since 1 January 1984.
fn time_of_day() -> TimeOfDay {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let since_1984 = since_epoch.saturating_sub(Duration::from_secs(
        DAYS_1970_TO_1984.saturating_mul(86_400),
    ));
    binary_time(since_1984)
}
