use std::collections::{HashMap, VecDeque};

use gpui::{Context, SharedString};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProjectSessionId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ProjectActivity {
    Import,
    Probe,
    Save,
    Load,
    Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProjectOperation {
    session: ProjectSessionId,
    serial: u64,
    activity: ProjectActivity,
}

pub(crate) struct ProjectSession {
    id: ProjectSessionId,
    next_session: u64,
    next_operation: u64,
    operations: HashMap<u64, ProjectActivity>,
}

impl Default for ProjectSession {
    fn default() -> Self {
        Self {
            id: ProjectSessionId(1),
            next_session: 2,
            next_operation: 1,
            operations: HashMap::new(),
        }
    }
}

impl ProjectSession {
    pub(crate) fn id(&self) -> ProjectSessionId {
        self.id
    }

    pub(crate) fn is_current(&self, id: ProjectSessionId) -> bool {
        self.id == id
    }

    pub(crate) fn advance(&mut self, cx: &mut Context<Self>) -> ProjectSessionId {
        let id = self.advance_state();
        cx.notify();
        id
    }

    fn advance_state(&mut self) -> ProjectSessionId {
        self.id = ProjectSessionId(self.next_session);
        self.next_session = self.next_session.saturating_add(1);
        self.operations.clear();
        self.id
    }

    pub(crate) fn begin(
        &mut self,
        activity: ProjectActivity,
        cx: &mut Context<Self>,
    ) -> ProjectOperation {
        let operation = self.begin_state(activity);
        cx.notify();
        operation
    }

    fn begin_state(&mut self, activity: ProjectActivity) -> ProjectOperation {
        let serial = self.next_operation;
        self.next_operation = self.next_operation.saturating_add(1);
        self.operations.insert(serial, activity);
        ProjectOperation {
            session: self.id,
            serial,
            activity,
        }
    }

    pub(crate) fn operation_is_current(&self, operation: ProjectOperation) -> bool {
        self.is_current(operation.session)
            && self.operations.get(&operation.serial) == Some(&operation.activity)
    }

    pub(crate) fn finish(&mut self, operation: ProjectOperation, cx: &mut Context<Self>) -> bool {
        if !self.finish_state(operation) {
            return false;
        }
        cx.notify();
        true
    }

    fn finish_state(&mut self, operation: ProjectOperation) -> bool {
        if !self.operation_is_current(operation) {
            return false;
        }
        self.operations.remove(&operation.serial);
        true
    }

    pub(crate) fn is_busy(&self) -> bool {
        !self.operations.is_empty()
    }

    pub(crate) fn busy_activities(&self) -> Vec<ProjectActivity> {
        let mut activities = Vec::new();
        for activity in self.operations.values() {
            if !activities.contains(activity) {
                activities.push(*activity);
            }
        }
        activities.sort();
        activities
    }
}

struct QueuedNotification {
    message: SharedString,
    success: bool,
}

#[derive(Default)]
pub(crate) struct UiNotifications {
    messages: VecDeque<QueuedNotification>,
    total_pushed: u64,
}

impl UiNotifications {
    const MAX_MESSAGES: usize = 16;

    /// Pushes a failure notification.
    pub(crate) fn push(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push_inner(message.into(), false);
        cx.notify();
    }

    /// Pushes a success notification.
    pub(crate) fn push_success(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.push_inner(message.into(), true);
        cx.notify();
    }

    fn push_inner(&mut self, message: SharedString, success: bool) {
        if self.messages.len() == Self::MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages
            .push_back(QueuedNotification { message, success });
        self.total_pushed = self.total_pushed.saturating_add(1);
    }

    pub(crate) fn unseen_since(&self, seen: u64) -> (Vec<(SharedString, bool)>, u64) {
        let held = self.messages.len() as u64;
        let first = self.total_pushed.saturating_sub(held);
        let start = seen.saturating_sub(first).min(held) as usize;
        let unseen = self
            .messages
            .iter()
            .skip(start)
            .map(|queued| (queued.message.clone(), queued.success))
            .collect();
        (unseen, self.total_pushed)
    }
}
