use std::collections::{HashMap, VecDeque};

use gpui::{Context, SharedString};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProjectSessionId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    pub(crate) fn busy_label(&self) -> Option<&'static str> {
        let activity = self.operations.values().next()?;
        Some(match activity {
            ProjectActivity::Import => "ファイル読み込み",
            ProjectActivity::Probe => "メディア解析",
            ProjectActivity::Save => "保存",
            ProjectActivity::Load => "プロジェクト読み込み",
            ProjectActivity::Export => "書き出し",
        })
    }
}

#[derive(Default)]
pub(crate) struct UiNotifications {
    messages: VecDeque<SharedString>,
}

impl UiNotifications {
    const MAX_MESSAGES: usize = 16;

    pub(crate) fn push(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        if self.messages.len() == Self::MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(message.into());
        cx.notify();
    }

    pub(crate) fn latest(&self) -> Option<SharedString> {
        self.messages.back().cloned()
    }
}
