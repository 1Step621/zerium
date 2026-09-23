use gpui::{Context, Entity};

use crate::{
    domain::timeline::TimelineEditor,
    ui::{animation_curve::AnimationSelection, transport::TransportController},
};

use super::project_session::{ProjectSession, ProjectSessionId};

/// Application-owned handles for state that moves together when a project is
/// replaced. Persistent edits still flow through `TimelineEditor`.
pub(crate) struct ProjectRuntime {
    pub(crate) editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    animation_selection: Entity<AnimationSelection>,
    session: Entity<ProjectSession>,
}

impl ProjectRuntime {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        animation_selection: Entity<AnimationSelection>,
        session: Entity<ProjectSession>,
    ) -> Self {
        Self {
            editor,
            transport,
            animation_selection,
            session,
        }
    }

    pub(crate) fn session(&self) -> &Entity<ProjectSession> {
        &self.session
    }

    pub(crate) fn advance_session<T>(&self, cx: &mut Context<T>) -> ProjectSessionId {
        self.session.update(cx, |session, cx| session.advance(cx))
    }

    pub(crate) fn reset_transient_state<T>(&self, cx: &mut Context<T>) {
        self.transport.update(cx, |transport, cx| {
            transport.reset_for_project_change(cx);
        });
        self.animation_selection
            .update(cx, |selection, cx| selection.clear(cx));
    }

    pub(crate) fn stop_transport<T>(&self, cx: &mut Context<T>) {
        self.transport
            .update(cx, |transport, cx| transport.stop(cx));
    }
}
