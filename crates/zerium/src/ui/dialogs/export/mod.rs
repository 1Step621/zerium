//! Export controller wiring; dialogs and background jobs have separate modules.
mod dialog;
mod job;

use gpui::{Context, Entity, Subscription, Task};

use crate::{
    app::{
        project::{ProjectOperation, ProjectSession, ProjectSessionId},
        rendering::RenderRuntime,
    },
    ui::workspace::notifications::UiNotifications,
};
use zerium_core::timeline::TimelineEditor;

use self::job::ExportJob;

enum ExportState {
    Idle,
    ChoosingDestination {
        operation: ProjectOperation,
        _task: Task<()>,
    },
    Running {
        job: ExportJob,
        _task: Task<()>,
    },
}

pub(crate) struct ExportController {
    editor: Entity<TimelineEditor>,
    render_runtime: Entity<RenderRuntime>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    state: ExportState,
    _session_subscription: Subscription,
}

impl ExportController {
    fn is_busy(&self) -> bool {
        !matches!(self.state, ExportState::Idle)
    }

    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        render_runtime: Entity<RenderRuntime>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = session.read(cx).id();
        let session_subscription = cx.observe(&session, |this, _, cx| {
            let session_id = this.session.read(cx).id();
            if session_id != this.session_id {
                this.cancel(cx);
                this.session_id = session_id;
                this.state = ExportState::Idle;
            }
            cx.notify();
        });
        Self {
            editor,
            render_runtime,
            session,
            session_id,
            notifications,
            state: ExportState::Idle,
            _session_subscription: session_subscription,
        }
    }
}
