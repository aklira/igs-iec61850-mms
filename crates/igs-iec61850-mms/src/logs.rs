// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The logs of a served device (task X2; IEC 61850-7-2 §17.3; IEC 61850-8-1 §17.3): each log control block of the configuration
//! records the changes of its data set into the MMS journal of its node, which ReadJournal reads. The state machine of the block is in
//! the core (`igs_iec61850_mms_core::logs`); this module gives it its notices and its values, and writes its entries (8-1 Table 77 and
//! §17.3.3.4). The writes of the block over MMS (GetLCBValues, SetLCBValues) are not served yet.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use igs_iec61850_mms_core::logs::{Action, Content, Event, Lcb, Write};
use igs_iec61850_mms_core::model::layout::value_at;
use igs_iec61850_mms_core::report::encode::reason_bits;
use igs_iec61850_mms_core::report::urcb::{Reason, Timer, Trigger};
use igs_iec61850_mms_core::scl::build::{Configuration, DataSet, LogDecl};
use igs_iso9506_mms::vmd::{Target, Vmd};
use igs_iso9506_mms_codec::generated::types::{
    ApplicationReference, Data, ECState, EntryContent, EntryContentEntryForm,
    EntryContentEntryFormData, EntryContentEntryFormDataEvent, Identifier, JournalEntry,
    JournalVariable, MMS255String, ObjectName, ObjectNameDomainSpecific, TimeOfDay,
};

use crate::model::NodeValue;
use crate::model::ServeError;
use crate::updates::{Notice, Updater};

/// The entries that a journal holds before its oldest is overwritten: a bounded buffer, as the log of 7-2 §17.3.1.2.
const JOURNAL_CAPACITY: usize = 512;

/// The days from 1970-01-01 to 1984-01-01: the time of day counts its day from 1 January 1984 (8-1 Annex; the six-octet time).
const DAYS_1970_TO_1984: u64 = 5_113;

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
    /// The name of the block in MMS (`LN$LG$name`): the event condition of its transitions.
    lcb_name: String,
    /// The journal the block records into (`LN$name` of the `Log` it names).
    journal: Target,
    members: Vec<Member>,
    next_id: u64,
    /// The instant of the integrity period, when one is armed.
    integrity: Option<Duration>,
}

/// The log control blocks of a served device, and the entries that their journals refused.
#[derive(Default)]
pub(crate) struct Logs {
    blocks: Vec<Block>,
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
        let mut logs = Self::default();
        for declared in &configuration.logs {
            let members = data_set_of(configuration, declared)
                .map(members_of)
                .unwrap_or_default();
            let mut block = block(declared, &members, vmd)?;
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

    /// Feeds the notices of the updates to the blocks, and fires their integrity period when it is due. `now` is the instant of the
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
}

/// The data set of a declared block, from the data sets of the configuration.
fn data_set_of<'a>(configuration: &'a Configuration, declared: &LogDecl) -> Option<&'a DataSet> {
    let reference = declared.config.data_set.as_deref()?;
    // The data set names of the configuration are node-qualified (`LLN0$DS1`), as the references of the blocks are.
    configuration
        .data_sets
        .iter()
        .find(|set| set.device == declared.device && set.name == reference)
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

fn block(declared: &LogDecl, members: &[Member], vmd: &mut Vmd) -> Result<Block, ServeError> {
    let domain = vmd.name(&declared.device)?;
    let log = declared.config.log_ref.clone().unwrap_or_default();
    let item = vmd.name(&format!("{}${}", declared.node, log))?;
    Ok(Block {
        lcb: Lcb::new(declared.config.clone()),
        device: declared.device.clone(),
        lcb_name: format!("{}$LG${}", declared.node, declared.name),
        journal: Target::in_domain(domain, item),
        members: members.to_vec(),
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

/// The wall-clock time of the record: the time of day in milliseconds and the day since 1 January 1984.
fn time_of_day() -> TimeOfDay {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let seconds = since_epoch.as_secs();
    let day = seconds.checked_div(86_400).unwrap_or(0);
    let second_of_day = seconds.checked_rem(86_400).unwrap_or(0);
    let milliseconds = second_of_day
        .saturating_mul(1000)
        .saturating_add(u64::from(since_epoch.subsec_millis()));
    let day_since_1984 = day.saturating_sub(DAYS_1970_TO_1984);
    let milliseconds = u32::try_from(milliseconds).unwrap_or(u32::MAX);
    let day_since_1984 = u16::try_from(day_since_1984).unwrap_or(u16::MAX);
    TimeOfDay::from_milliseconds_and_day(milliseconds, day_since_1984)
        .unwrap_or(TimeOfDay(Vec::new()))
}
