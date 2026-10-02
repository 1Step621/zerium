use std::collections::HashMap;

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

    pub(crate) fn advance(&mut self) -> ProjectSessionId {
        self.id = ProjectSessionId(self.next_session);
        self.next_session = self.next_session.saturating_add(1);
        self.operations.clear();
        self.id
    }

    pub(crate) fn begin(&mut self, activity: ProjectActivity) -> ProjectOperation {
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

    pub(crate) fn finish(&mut self, operation: ProjectOperation) -> bool {
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
