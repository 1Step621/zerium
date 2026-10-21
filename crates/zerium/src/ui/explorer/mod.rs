//! File browser entity and subscriptions.
mod directory;
mod render;
mod selection;

pub(crate) use render::ExplorerFileDrag;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{Context, Entity, ScrollHandle, SharedString, Subscription, Task};

use crate::{
    project_session::{ProjectSession, ProjectSessionId},
    ui::notifications::UiNotifications,
};
use zerium_core::plugin::PluginRegistry;

use crate::ui::explorer::{directory::ExplorerEntry, selection::ExplorerMarquee};

pub(crate) struct Explorer {
    plugins: Arc<PluginRegistry>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    current_directory: PathBuf,
    entries: Vec<ExplorerEntry>,
    selected_paths: HashSet<PathBuf>,
    scroll_handle: ScrollHandle,
    marquee: Option<ExplorerMarquee>,
    error: Option<SharedString>,
    load_generation: u64,
    _load_task: Task<()>,
    _session_subscription: Subscription,
}

impl Explorer {
    pub(crate) fn new(
        plugins: Arc<PluginRegistry>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = session.read(cx).id();
        let initial_directory = directory::default_directory(session.read(cx));
        let mut project_path = session.read(cx).path().map(Path::to_path_buf);
        let session_subscription = cx.observe(&session, move |this, _, cx| {
            let session = this.session.read(cx);
            let session_id = session.id();
            let path = session.path().map(Path::to_path_buf);
            if session_id == this.session_id && path == project_path {
                return;
            }
            let directory = directory::default_directory(session);
            this.session_id = session_id;
            project_path = path;
            this.load_directory(directory, cx);
        });
        let mut explorer = Self {
            plugins,
            session,
            session_id,
            notifications,
            current_directory: initial_directory.clone(),
            entries: Vec::new(),
            selected_paths: HashSet::new(),
            scroll_handle: ScrollHandle::new(),
            marquee: None,
            error: None,
            load_generation: 0,
            _load_task: Task::ready(()),
            _session_subscription: session_subscription,
        };
        explorer.load_directory(initial_directory, cx);
        explorer
    }
}
