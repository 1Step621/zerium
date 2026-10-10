use ::ui::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui::{App, AppContext as _, Context, Entity, Task, Window};
use rust_i18n::t;
use velopack::{UpdateCheck, UpdateManager, VelopackAsset, sources::GithubSource};

use crate::project_session::ProjectSession;

use super::workspace::Workspace;

pub(crate) struct WorkspaceUpdate {
    ready: Option<(UpdateManager, VelopackAsset)>,
    session: Entity<ProjectSession>,
    _task: Task<()>,
}

impl WorkspaceUpdate {
    pub(crate) fn new(session: Entity<ProjectSession>, cx: &mut Context<Workspace>) -> Self {
        let mut updates = Self {
            ready: None,
            session,
            _task: Task::ready(()),
        };
        let source = GithubSource::new("https://github.com/1Step621/zerium", None, false);
        let manager = match UpdateManager::new(source, None, None) {
            Ok(manager) => manager,
            // Ordinary cargo builds are not Velopack installs.
            Err(velopack::Error::NotInstalled(_)) => return updates,
            Err(error) => {
                eprintln!("failed to initialize updates: {error}");
                return updates;
            }
        };
        if let Some(ready) = manager.get_update_pending_restart() {
            updates.ready = Some((manager, ready));
            return updates;
        }
        updates._task = cx.spawn(async move |workspace, cx| {
            let result = cx
                .background_spawn(async move {
                    if let UpdateCheck::UpdateAvailable(update) = manager.check_for_updates()? {
                        manager.download_updates(&update, None)?;
                        Ok::<_, velopack::Error>(Some((manager, update.TargetFullRelease)))
                    } else {
                        Ok(None)
                    }
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| match result {
                Ok(ready) => {
                    workspace.updates.ready = ready;
                    cx.notify();
                }
                Err(error) => eprintln!("automatic update failed: {error}"),
            });
        });
        updates
    }

    pub(crate) fn button(&self, cx: &mut Context<Workspace>) -> Option<Button> {
        let (manager, update) = self.ready.as_ref()?;
        let manager = manager.clone();
        let update = update.clone();
        let session = self.session.clone();
        Some(
            Button::new("restart-to-update")
                .small()
                .compact()
                .ghost()
                .label(t!("update.restart").to_string())
                .tooltip(t!("update.ready", version = update.Version).to_string())
                .on_click(cx.listener(move |this, _, window, cx| {
                    let export_busy = this.export_controller.read(cx).is_exporting();
                    let restart = {
                        let manager = manager.clone();
                        let update = update.clone();
                        let session = session.clone();
                        let notifications = this.notifications.clone();
                        move |_: &mut Window, cx: &mut App| {
                            let path = session.read(cx).path().map(std::path::Path::to_path_buf);
                            match manager.wait_exit_then_apply_updates(&update, false, true, path) {
                                Ok(()) => cx.quit(),
                                Err(error) => notifications.update(cx, |notifications, cx| {
                                    notifications
                                        .push(t!("update.failed", error = error).to_string(), cx);
                                }),
                            }
                        }
                    };
                    if this.project_controller.update(cx, |project, cx| {
                        project.should_close(export_busy, window, cx, restart.clone())
                    }) {
                        restart(window, cx);
                    }
                })),
        )
    }
}
