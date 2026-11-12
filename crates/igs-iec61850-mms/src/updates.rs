// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

//! The application's path to the values of the served model (task A5; MODEL-006).
//!
//! An application task changes values, qualities and timestamps through an [`Updater`], which it clones and uses from
//! its own task. An update is one call with the changes of one logical node. The call checks every change first (the name
//! exists, the value has the type of the attribute, the enumerations hold defined values, and all the changes belong to
//! the same node); then it applies them to a copy of the node's value and stores the copy under the node's lock. So the
//! update is all or nothing, and a reader that takes the lock sees the value, its quality and its timestamp of one
//! update, never a part of it.
//!
//! The snapshot rules (MODEL-006):
//!
//! - **Inside a node**: a read of one item is taken under the lock, so it is one state of the node. A read of a data
//!   object returns its attributes from one state, which keeps a value, its quality and its timestamp together.
//! - **Between items of one request**: an MMS Read of several items takes each item at its own moment. A client that
//!   needs one state of several nodes reads a data set or a report (tasks D and B), which are then built on the revision
//!   below.
//! - **Event ordering**: each node has a revision, which goes up by one for each change of the node (a write of a client
//!   or an update of the application). The revision that an update returns orders the updates of a node; the revisions
//!   of different nodes are not ordered among themselves.
//! - **Non-blocking**: the lock is held for a copy and a store of the node's value. No I/O and no wait for an association
//!   happens under it, so an application task never waits for the network.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

use igs_iec61850_mms_core::model::layout::{defines_enumerations, set_at, value_at, DefinedValues};
use igs_iso9506_mms::vmd::conforms;
use igs_iso9506_mms_codec::generated::types::{Data, TypeDescription};

use crate::model::NodeValue;

/// One change of an attribute: the name of the attribute as `domain/item` (the MMS name of the served variable,
/// for example `IED1LD1/GGIO1$ST$Ind1$stVal`), and its new value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    name: String,
    value: Data,
}

impl Change {
    /// A change of the attribute `name` to `value`.
    pub fn new(name: impl Into<String>, value: Data) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }

    /// The name of the attribute.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The revision of a node after an update: it goes up by one for each change of the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(u64);

impl Revision {
    /// The number of the revision.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Why an update is refused. Nothing of a refused update is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UpdateError {
    /// The update has no change.
    Empty,
    /// The name is not a served attribute.
    UnknownName(String),
    /// The changes belong to more than one logical node: an update is applied to one node at a time.
    MixedNodes,
    /// The value does not have the type of the attribute.
    TypeMismatch(String),
    /// The value is an enumeration value that the namespace does not define.
    UndefinedValue(String),
    /// The node's value is unavailable: a task that held its lock failed.
    Poisoned,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "the update has no change"),
            Self::UnknownName(name) => write!(f, "{name} is not a served attribute"),
            Self::MixedNodes => write!(f, "the changes belong to more than one logical node"),
            Self::TypeMismatch(name) => write!(f, "the value of {name} does not have its type"),
            Self::UndefinedValue(name) => {
                write!(f, "the value of {name} is not defined by the namespace")
            }
            Self::Poisoned => write!(f, "the value of the node is unavailable"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// What the updater needs of a served variable: its node, the node's value, the path, the type, and the enumerations.
pub(crate) struct Point {
    pub(crate) node: String,
    pub(crate) root: Arc<Mutex<NodeValue>>,
    pub(crate) path: Vec<usize>,
    pub(crate) ty: TypeDescription,
    pub(crate) enumerations: Vec<DefinedValues>,
}

/// The most notices kept for the reports before the oldest are dropped (RUN-001). A dropped notice is counted: it is never
/// silent.
pub(crate) const NOTICE_LIMIT: usize = 65_536;

/// A change applied by an update: the served name of the attribute (`domain/item`), and whether its value changed (the
/// data-change and quality-change triggers need a change; the data-update trigger needs only the update).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Notice {
    pub(crate) name: String,
    pub(crate) changed: bool,
}

/// The notices of the updates, for the reports, with the number of those dropped at the limit.
#[derive(Default)]
pub(crate) struct Notices {
    pub(crate) queue: VecDeque<Notice>,
    pub(crate) dropped: u64,
}

/// The application's handle to the values of the served model. Cheap to clone; safe to use from other tasks.
#[derive(Clone)]
pub struct Updater {
    points: Arc<BTreeMap<String, Point>>,
    notices: Arc<Mutex<Notices>>,
}

impl Updater {
    pub(crate) fn new(points: BTreeMap<String, Point>) -> Self {
        Self {
            points: Arc::new(points),
            notices: Arc::new(Mutex::new(Notices::default())),
        }
    }

    /// The node's value and the path of a served attribute, for the reports (task D4).
    pub(crate) fn point(&self, name: &str) -> Option<(Arc<Mutex<NodeValue>>, Vec<usize>)> {
        self.points
            .get(name)
            .map(|point| (Arc::clone(&point.root), point.path.clone()))
    }

    /// Takes the notices of the updates made since the last call, with the number dropped at the limit.
    pub(crate) fn take_notices(&self) -> (Vec<Notice>, u64) {
        match self.notices.lock() {
            Ok(mut notices) => {
                let dropped = std::mem::take(&mut notices.dropped);
                (notices.queue.drain(..).collect(), dropped)
            }
            Err(_) => (Vec::new(), 0),
        }
    }

    /// Applies the changes of one logical node, all or none, and returns the revision of the node after them.
    pub fn update(&self, changes: &[Change]) -> Result<Revision, UpdateError> {
        if changes.is_empty() {
            return Err(UpdateError::Empty);
        }
        let mut node: Option<&str> = None;
        let mut checked: Vec<(&Change, &Point)> = Vec::with_capacity(changes.len());
        for change in changes {
            let point = self
                .points
                .get(&change.name)
                .ok_or_else(|| UpdateError::UnknownName(change.name.clone()))?;
            match node {
                None => node = Some(point.node.as_str()),
                Some(first) if first != point.node => return Err(UpdateError::MixedNodes),
                Some(_) => {}
            }
            if !conforms(&point.ty, &change.value) {
                return Err(UpdateError::TypeMismatch(change.name.clone()));
            }
            if !defines_enumerations(&change.value, &point.enumerations) {
                return Err(UpdateError::UndefinedValue(change.name.clone()));
            }
            checked.push((change, point));
        }
        // The checks above took the node from the first change; every change is on that node.
        let root = match checked.first() {
            Some((_, point)) => Arc::clone(&point.root),
            None => return Err(UpdateError::Empty),
        };
        let mut state = root.lock().map_err(|_| UpdateError::Poisoned)?;
        let mut next = state.data.clone();
        let mut notices = Vec::with_capacity(checked.len());
        for (change, point) in &checked {
            let before = value_at(&next, &point.path).cloned();
            if !set_at(&mut next, &point.path, change.value.clone()) {
                return Err(UpdateError::UnknownName(change.name.clone()));
            }
            notices.push(Notice {
                name: change.name.clone(),
                changed: before.as_ref() != Some(&change.value),
            });
        }
        state.data = next;
        state.revision = state.revision.saturating_add(1);
        self.notify(notices);
        Ok(Revision(state.revision))
    }

    /// Queues the notices of an applied update, dropping the oldest beyond the limit and counting them.
    fn notify(&self, notices: Vec<Notice>) {
        if let Ok(mut queue) = self.notices.lock() {
            for notice in notices {
                if queue.queue.len() >= NOTICE_LIMIT {
                    queue.queue.pop_front();
                    queue.dropped = queue.dropped.saturating_add(1);
                }
                queue.queue.push_back(notice);
            }
        }
    }
}
