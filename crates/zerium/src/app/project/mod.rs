pub(super) mod file;
pub(crate) mod media;
mod scene;
mod session;

pub(crate) use session::{ProjectActivity, ProjectOperation, ProjectSession, ProjectSessionId};

use rust_i18n::t;
use std::path::{Path, PathBuf};

use gpui::{App, AppContext as _, Context, Entity, PathPromptOptions, Task, Window};

use self::file as project_files;
use super::transport::TransportController;
use crate::ui::{
    dialogs::project as project_dialog, panels::animation_curve::AnimationSelection,
    workspace::notifications::UiNotifications,
};
use zerium_core::{
    persistence::{LoadedDocument, LoadedProject, PROJECT_EXTENSION},
    timeline::{Frame, FrameRate, ProjectResolution, TimelineEditor},
};

#[derive(Clone, Copy)]
enum OpenMode {
    Auto,
    SceneOnly,
}

#[derive(Clone)]
enum PendingProjectChange {
    New,
    Open(PathBuf, LoadedProject),
}

pub(crate) struct ProjectController {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    animation_selection: Entity<AnimationSelection>,
    session: Entity<ProjectSession>,
    notifications: Entity<UiNotifications>,
    saved_revision: u64,
    operation: Option<ProjectOperation>,
    _task: Task<()>,
}

impl ProjectController {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        animation_selection: Entity<AnimationSelection>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
    ) -> Self {
        Self {
            editor,
            transport,
            animation_selection,
            session,
            notifications,
            saved_revision: 0,
            operation: None,
            _task: Task::ready(()),
        }
    }

    pub(crate) fn request_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_project_change(PendingProjectChange::New, window, cx);
    }

    pub(crate) fn request_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open(None, OpenMode::Auto, window, cx);
    }

    pub(crate) fn request_open_path(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open(Some(path), OpenMode::Auto, window, cx);
    }

    pub(crate) fn import_scene(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open(None, OpenMode::SceneOnly, window, cx);
    }

    pub(crate) fn save(&mut self, cx: &mut Context<Self>) {
        self.save_to(self.session.read(cx).path().map(Path::to_path_buf), cx);
    }

    pub(crate) fn save_as(&mut self, cx: &mut Context<Self>) {
        self.save_to(None, cx);
    }

    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation.is_none() {
            project_dialog::open_settings(
                &self.editor,
                &self.transport,
                &self.notifications,
                window,
                cx,
            );
        }
    }

    pub(crate) fn window_title(&self, cx: &App) -> String {
        let dirty = self.editor.read(cx).project_revision() != self.saved_revision;
        let name = self
            .session
            .read(cx)
            .path()
            .and_then(Path::file_stem)
            .map(|name| name.to_string_lossy());
        match name {
            Some(name) => format!("{}{} — Zerium", if dirty { "*" } else { "" }, name),
            None => format!("{}Zerium", if dirty { "*" } else { "" }),
        }
    }

    fn begin_operation(
        &mut self,
        activity: ProjectActivity,
        cx: &mut Context<Self>,
    ) -> Option<ProjectOperation> {
        if self.operation.is_some() {
            return None;
        }
        let operation = self.session.update(cx, |session, cx| {
            let operation = session.begin(activity);
            cx.notify();
            operation
        });
        self.operation = Some(operation);
        cx.notify();
        Some(operation)
    }

    fn finish_operation(&mut self, operation: ProjectOperation, cx: &mut Context<Self>) -> bool {
        if self.operation != Some(operation) {
            return false;
        }
        self.operation = None;
        let current = self.session.update(cx, |session, cx| {
            let current = session.finish(operation);
            if current {
                cx.notify();
            }
            current
        });
        cx.notify();
        current
    }

    fn replace_project(
        &mut self,
        path: Option<PathBuf>,
        update: impl FnOnce(&mut TimelineEditor),
        cx: &mut Context<Self>,
    ) {
        self.session.update(cx, |session, cx| {
            session.advance(path);
            cx.notify();
        });
        self.transport
            .update(cx, |transport, cx| transport.reset_for_project_change(cx));
        self.animation_selection
            .update(cx, |selection, cx| selection.clear(cx));
        self.editor.update(cx, |editor, cx| {
            update(editor);
            cx.notify();
        });
        self.saved_revision = self.editor.read(cx).project_revision();
        cx.notify();
    }

    fn request_project_change(
        &mut self,
        operation: PendingProjectChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation.is_some() {
            return;
        }
        if self.editor.read(cx).project_revision() == self.saved_revision {
            self.perform_project_change(operation, cx);
            return;
        }

        let controller = cx.entity();
        project_dialog::confirm_discard(window, cx, move |_, cx| {
            controller.update(cx, |controller, cx| {
                controller.perform_project_change(operation.clone(), cx);
            });
        });
    }

    fn perform_project_change(&mut self, operation: PendingProjectChange, cx: &mut Context<Self>) {
        if self.operation.is_some() {
            return;
        }
        match operation {
            PendingProjectChange::New => self.new_project(cx),
            PendingProjectChange::Open(path, project) => {
                self.replace_project(Some(path.clone()), |editor| project.apply(editor), cx);
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push_success(
                        t!(
                            "project.load_complete",
                            name = path
                                .file_name()
                                .map(|name| name.to_string_lossy())
                                .unwrap_or_default()
                        )
                        .to_string(),
                        cx,
                    );
                });
            }
        }
    }

    fn new_project(&mut self, cx: &mut Context<Self>) {
        self.replace_project(
            None,
            |editor| {
                editor.reset(ProjectResolution::DEFAULT, FrameRate::FPS_30, Frame::new(0));
            },
            cx,
        );
    }

    fn open(
        &mut self,
        path: Option<PathBuf>,
        mode: OpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (activity, prompt) = match mode {
            OpenMode::Auto => (ProjectActivity::Load, t!("project.open_prompt").to_string()),
            OpenMode::SceneOnly => (
                ProjectActivity::Import,
                t!("scene.import_prompt").to_string(),
            ),
        };
        let Some(operation) = self.begin_operation(activity, cx) else {
            return;
        };
        let plugins = self.editor.read(cx).plugin_registry_arc();
        let session = self.session.clone();
        self._task = cx.spawn_in(window, async move |controller, cx| {
            let result: Result<_, String> = async {
                let path = match path {
                    Some(path) => Some(path),
                    None => {
                        let receiver = cx
                            .update(|_, cx| {
                                cx.prompt_for_paths(PathPromptOptions {
                                    files: true,
                                    directories: false,
                                    multiple: false,
                                    prompt: Some(prompt.into()),
                                })
                            })
                            .map_err(|error| {
                                t!("project.file_picker_failed", error = error).to_string()
                            })?;
                        receiver
                            .await
                            .map_err(|error| {
                                t!("project.file_picker_failed", error = error).to_string()
                            })?
                            .map_err(|error| {
                                t!("project.select_file_failed", error = error).to_string()
                            })?
                            .and_then(|paths| paths.into_iter().next())
                    }
                };
                let Some(path) = path else {
                    return Ok(None);
                };
                if !has_extension(&path, PROJECT_EXTENSION) {
                    return Err(
                        t!("project.extension_prompt", extension = PROJECT_EXTENSION).to_string(),
                    );
                }
                if !session.read_with(cx, |session, _| session.operation_is_current(operation)) {
                    return Ok(None);
                }
                let input = path.clone();
                let project = cx
                    .background_spawn(async move { project_files::load(&input, &plugins) })
                    .await
                    .map_err(|error| match mode {
                        OpenMode::Auto => t!("project.load_failed", error = error).to_string(),
                        OpenMode::SceneOnly => t!("scene.import_failed", error = error).to_string(),
                    })?;
                if let (OpenMode::SceneOnly, LoadedDocument::Project(_)) = (mode, &project) {
                    return Err(t!("scene.expected_file").to_string());
                }
                Ok(Some((path, project)))
            }
            .await;
            controller
                .update_in(cx, |controller, window, cx| {
                    if !controller.finish_operation(operation, cx) {
                        return;
                    }
                    match result {
                        Ok(Some((path, LoadedDocument::Project(project)))) => {
                            controller.request_project_change(
                                PendingProjectChange::Open(path, project),
                                window,
                                cx,
                            );
                        }
                        Ok(Some((_, LoadedDocument::Scene(scene)))) => {
                            controller.apply_scene(scene, cx);
                        }
                        Ok(None) => {}
                        Err(error) => controller
                            .notifications
                            .update(cx, |notifications, cx| notifications.push(error, cx)),
                    }
                })
                .ok();
        });
    }

    fn save_to(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(operation) = self.begin_operation(ProjectActivity::Save, cx) else {
            return;
        };
        let current_path = self.session.read(cx).path().map(Path::to_path_buf);
        // Save captures the current edit immediately; Save As captures after path selection.
        let snapshot = path.as_ref().map(|_| self.editor.read(cx).snapshot());
        let editor = self.editor.clone();
        let session = self.session.clone();
        self._task = cx.spawn(async move |controller, cx| {
            let result: Result<_, String> = async {
                let path = match path {
                    Some(path) => Some(path),
                    None => {
                        let initial_directory = current_path
                            .as_deref()
                            .and_then(Path::parent)
                            .map(Path::to_path_buf)
                            .or_else(|| std::env::current_dir().ok())
                            .unwrap_or_else(|| PathBuf::from("."));
                        let suggested_name = current_path
                            .as_deref()
                            .and_then(Path::file_name)
                            .and_then(|name| name.to_str())
                            .unwrap_or("project.zero");
                        let receiver = cx.update(|cx| {
                            cx.prompt_for_new_path(&initial_directory, Some(suggested_name))
                        });
                        receiver
                            .await
                            .map_err(|error| {
                                t!("project.destination_picker_failed", error = error).to_string()
                            })?
                            .map_err(|error| {
                                t!("project.select_destination_failed", error = error).to_string()
                            })?
                    }
                };
                let Some(mut path) = path else {
                    return Ok(None);
                };
                if !has_extension(&path, PROJECT_EXTENSION) {
                    path.set_extension(PROJECT_EXTENSION);
                }
                if !session.read_with(cx, |session, _| session.operation_is_current(operation)) {
                    return Ok(None);
                }
                let snapshot =
                    snapshot.unwrap_or_else(|| editor.read_with(cx, |editor, _| editor.snapshot()));
                let revision = snapshot.project_revision();
                let output = path.clone();
                cx.background_spawn(async move { project_files::save(&snapshot, &output) })
                    .await
                    .map_err(|error| t!("project.save_failed", error = error).to_string())?;
                Ok(Some((path, revision)))
            }
            .await;
            controller
                .update(cx, |controller, cx| {
                    if !controller.finish_operation(operation, cx) {
                        return;
                    }
                    match result {
                        Ok(Some((path, revision))) => {
                            controller.session.update(cx, |session, cx| {
                                session.set_path(Some(path.clone()));
                                cx.notify();
                            });
                            controller.saved_revision = revision;
                            controller.notifications.update(cx, |notifications, cx| {
                                notifications.push_success(
                                    t!(
                                        "project.save_complete",
                                        name = path
                                            .file_name()
                                            .map(|name| name.to_string_lossy())
                                            .unwrap_or_default()
                                    )
                                    .to_string(),
                                    cx,
                                );
                            });
                        }
                        Ok(None) => {}
                        Err(error) => controller
                            .notifications
                            .update(cx, |notifications, cx| notifications.push(error, cx)),
                    }
                })
                .ok();
        });
    }

    pub(crate) fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let activities = self.session.read(cx).busy_activities();
        if !activities.is_empty() {
            let activity = activities
                .iter()
                .map(|activity| match activity {
                    ProjectActivity::Import => t!("project.import_activity").to_string(),
                    ProjectActivity::SelectFile => t!("project.select_file_activity").to_string(),
                    ProjectActivity::Save => t!("project.save_activity").to_string(),
                    ProjectActivity::Load => t!("project.load_activity").to_string(),
                    ProjectActivity::Export => t!("project.export_activity").to_string(),
                })
                .collect::<Vec<_>>()
                .join("・");
            self.notifications.update(cx, |notifications, cx| {
                notifications.push(
                    t!("project.wait_activity", activity = activity).to_string(),
                    cx,
                );
            });
            return false;
        }
        if self.editor.read(cx).project_revision() == self.saved_revision {
            return true;
        }

        project_dialog::confirm_exit(window, cx);
        false
    }
}

fn has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}
