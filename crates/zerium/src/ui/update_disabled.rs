use ::ui::button::Button;
use gpui::{Context, Entity};

use crate::project_session::ProjectSession;

use super::workspace::Workspace;

pub(crate) struct WorkspaceUpdate;

impl WorkspaceUpdate {
    pub(crate) fn new(_session: Entity<ProjectSession>, _cx: &mut Context<Workspace>) -> Self {
        Self
    }

    pub(crate) fn button(&self, _cx: &mut Context<Workspace>) -> Option<Button> {
        None
    }
}
