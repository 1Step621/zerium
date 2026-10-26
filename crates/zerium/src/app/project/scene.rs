//! Scene file operations preserve the current project and its save location.
use std::path::{Path, PathBuf};

use gpui::{AppContext as _, Context};
use rust_i18n::t;
use zerium_core::persistence::{LoadedScene, SCENE_FILE_SUFFIX};

use super::{ProjectActivity, ProjectController, file};

impl ProjectController {
    pub(super) fn apply_scene(&mut self, scene: LoadedScene, cx: &mut Context<Self>) {
        let result = self.editor.update(cx, |editor, cx| {
            let id = scene.import(editor)?;
            let name = editor
                .scene(id)
                .expect("imported scene exists")
                .name
                .clone();
            cx.notify();
            Ok::<_, zerium_core::timeline::TimelineEditError>(name)
        });
        self.notifications
            .update(cx, |notifications, cx| match result {
                Ok(name) => notifications
                    .push_success(t!("scene.import_complete", name = name).to_string(), cx),
                Err(error) => {
                    notifications.push(t!("scene.import_failed", error = error).to_string(), cx)
                }
            });
    }

    pub(crate) fn save_scene_file(&mut self, cx: &mut Context<Self>) {
        let Some(operation) = self.begin_operation(ProjectActivity::Save, cx) else {
            return;
        };
        let editor = self.editor.read(cx);
        let root = editor.active_scene_id();
        let snapshot = editor.snapshot();
        let name = editor
            .scene(root)
            .expect("active scene exists")
            .name
            .chars()
            .map(|ch| {
                if ch.is_control() || "<>:\"/\\|?*".contains(ch) {
                    '_'
                } else {
                    ch
                }
            })
            .collect::<String>();
        let suggested_name = format!("{name}{SCENE_FILE_SUFFIX}");
        let initial_directory = self
            .session
            .read(cx)
            .path()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let session = self.session.clone();
        self._task = cx.spawn(async move |controller, cx| {
            let result: Result<_, String> = async {
                let receiver = cx
                    .update(|cx| cx.prompt_for_new_path(&initial_directory, Some(&suggested_name)));
                let path = receiver
                    .await
                    .map_err(|error| {
                        t!("project.destination_picker_failed", error = error).to_string()
                    })?
                    .map_err(|error| {
                        t!("project.select_destination_failed", error = error).to_string()
                    })?;
                let Some(mut path) = path else {
                    return Ok(None);
                };
                if !path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(SCENE_FILE_SUFFIX)
                }) {
                    path.set_extension(SCENE_FILE_SUFFIX.trim_start_matches('.'));
                }
                if !session.read_with(cx, |session, _| session.operation_is_current(operation)) {
                    return Ok(None);
                }
                let output = path.clone();
                cx.background_spawn(async move { file::save_scene(&snapshot, root, &output) })
                    .await
                    .map_err(|error| t!("scene.save_failed", error = error).to_string())?;
                Ok(Some(path))
            }
            .await;
            controller
                .update(cx, |controller, cx| {
                    if !controller.finish_operation(operation, cx) {
                        return;
                    }
                    controller
                        .notifications
                        .update(cx, |notifications, cx| match result {
                            Ok(Some(path)) => notifications.push_success(
                                t!(
                                    "scene.save_complete",
                                    name = path
                                        .file_name()
                                        .map(|name| name.to_string_lossy())
                                        .unwrap_or_default()
                                )
                                .to_string(),
                                cx,
                            ),
                            Ok(None) => {}
                            Err(error) => notifications.push(error, cx),
                        });
                })
                .ok();
        });
    }
}
