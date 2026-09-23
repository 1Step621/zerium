use std::path::{Path, PathBuf};

use ::ui::{
    ContextModal as _, Sizable as _, StyledExt as _,
    input::{InputState, NumberInput},
    modal::{Modal, ModalButtonProps},
};
use gpui::{App, Context, Entity, PathPromptOptions, Task, Window, div, prelude::*};

use crate::{
    app::project_runtime::ProjectRuntime,
    application::project_session::ProjectActivity,
    domain::{
        persistence::PROJECT_EXTENSION,
        timeline::{Frame, FrameRate, ProjectResolution},
    },
    engine::project_io,
    ui::session::UiNotifications,
};

#[derive(Clone, Copy)]
enum PendingProjectChange {
    New,
    Open,
}

pub(crate) struct ProjectController {
    runtime: crate::app::project_runtime::ProjectRuntime,
    notifications: Entity<UiNotifications>,
    path: Option<PathBuf>,
    saved_revision: u64,
    busy: bool,
    _dialog_task: Task<()>,
    _io_task: Task<()>,
}

impl ProjectController {
    pub(crate) fn new(runtime: ProjectRuntime, notifications: Entity<UiNotifications>) -> Self {
        Self {
            runtime,
            notifications,
            path: None,
            saved_revision: 0,
            busy: false,
            _dialog_task: Task::ready(()),
            _io_task: Task::ready(()),
        }
    }

    pub(crate) fn request_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_project_change(PendingProjectChange::New, window, cx);
    }

    pub(crate) fn request_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_project_change(PendingProjectChange::Open, window, cx);
    }

    pub(crate) fn open_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if !has_project_extension(&path) {
            self.notifications.update(cx, |notifications, cx| {
                notifications.push(
                    format!(".{} ファイルを選択してください", PROJECT_EXTENSION),
                    cx,
                );
            });
            return;
        }

        self.busy = true;
        cx.notify();
        let plugins = self.runtime.editor.read(cx).plugin_registry_arc();
        let session = self.runtime.session().clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::Load);
            cx.notify();
            operation
        });
        self._io_task = cx.spawn(async move |controller, cx| {
            let input = path.clone();
            let result = cx
                .background_spawn(async move { project_io::load(&input, &plugins) })
                .await;
            if session.update(cx, |session, _| session.operation_is_current(operation)) {
                controller
                    .update(cx, |controller, cx| match result {
                        Ok(project) => {
                            controller.runtime.advance_session(cx);
                            controller.runtime.reset_transient_state(cx);
                            controller.runtime.editor.update(cx, |editor, cx| {
                                project.apply(editor);
                                cx.notify();
                            });
                            controller.path = Some(path.clone());
                            controller.saved_revision =
                                controller.runtime.editor.read(cx).project_revision();
                            controller.busy = false;
                            controller.notifications.update(cx, |notifications, cx| {
                                notifications.push_success(
                                    format!(
                                        "読み込み完了: {}",
                                        path.file_name()
                                            .map(|name| name.to_string_lossy())
                                            .unwrap_or_default()
                                    ),
                                    cx,
                                );
                            });
                            cx.notify();
                        }
                        Err(error) => {
                            controller.busy = false;
                            controller.notifications.update(cx, |notifications, cx| {
                                notifications.push(format!("読み込み失敗: {error}"), cx);
                            });
                            cx.notify();
                        }
                    })
                    .ok();
            }
            session.update(cx, |session, cx| {
                if session.finish(operation) {
                    cx.notify();
                }
            });
        });
    }

    pub(crate) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if let Some(path) = self.path.clone() {
            self.save_to(path, cx);
        } else {
            self.choose_save_path(window, cx);
        }
    }

    pub(crate) fn save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.busy {
            self.choose_save_path(window, cx);
        }
    }

    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let editor = self.runtime.editor.read(cx);
        let resolution = editor.resolution();
        let frame_rate = editor.frame_rate();
        let width =
            cx.new(|cx| InputState::new(window, cx).default_value(resolution.width().to_string()));
        let height =
            cx.new(|cx| InputState::new(window, cx).default_value(resolution.height().to_string()));
        let frame_rate_numerator = cx.new(|cx| {
            InputState::new(window, cx).default_value(frame_rate.numerator().to_string())
        });
        let frame_rate_denominator = cx.new(|cx| {
            InputState::new(window, cx).default_value(frame_rate.denominator().to_string())
        });
        let controller = cx.entity();
        window.open_modal(cx, move |modal: Modal, _, _| {
            let confirm_controller = controller.clone();
            let confirm_width = width.clone();
            let confirm_height = height.clone();
            let confirm_frame_rate_numerator = frame_rate_numerator.clone();
            let confirm_frame_rate_denominator = frame_rate_denominator.clone();
            modal
                .title(
                    div()
                        .font_family(".SystemUIFont")
                        .font_normal()
                        .child("プロジェクト設定"),
                )
                .width(gpui::px(440.))
                .confirm()
                .button_props(
                    ModalButtonProps::default()
                        .ok_text("適用")
                        .cancel_text("キャンセル"),
                )
                .on_ok(move |_, _, cx| {
                    confirm_controller.update(cx, |controller, cx| {
                        let width = confirm_width.read(cx).value().parse::<u32>();
                        let height = confirm_height.read(cx).value().parse::<u32>();
                        let resolution = width
                            .ok()
                            .zip(height.ok())
                            .and_then(|(width, height)| ProjectResolution::new(width, height));
                        let Some(resolution) = resolution else {
                            controller.notifications.update(cx, |notifications, cx| {
                                notifications.push(
                                    format!(
                                        "解像度は1〜{}の整数で入力してください",
                                        ProjectResolution::MAX_DIMENSION
                                    ),
                                    cx,
                                );
                            });
                            cx.notify();
                            return false;
                        };
                        let numerator = confirm_frame_rate_numerator
                            .read(cx)
                            .value()
                            .trim()
                            .parse::<u32>();
                        let denominator = confirm_frame_rate_denominator
                            .read(cx)
                            .value()
                            .trim()
                            .parse::<u32>();
                        let frame_rate = match (numerator, denominator) {
                            (Ok(numerator), Ok(denominator)) => {
                                match FrameRate::new(numerator, denominator) {
                                    Some(frame_rate) => frame_rate,
                                    None => {
                                        controller.notifications.update(cx, |notifications, cx| {
                                            notifications.push(
                                                "フレームレートは0より大きくしてください",
                                                cx,
                                            );
                                        });
                                        cx.notify();
                                        return false;
                                    }
                                }
                            }
                            _ => {
                                controller.notifications.update(cx, |notifications, cx| {
                                    notifications
                                        .push("フレームレートは1以上の整数で入力してください", cx);
                                });
                                cx.notify();
                                return false;
                            }
                        };
                        match controller.runtime.editor.update(cx, |editor, cx| {
                            let result = editor.update_project_settings(resolution, frame_rate);
                            if matches!(result, Ok(true)) {
                                cx.notify();
                            }
                            result
                        }) {
                            Ok(_) => {
                                controller.runtime.stop_transport(cx);
                                cx.notify();
                                true
                            }
                            Err(error) => {
                                controller.notifications.update(cx, |notifications, cx| {
                                    notifications.push(error.to_string(), cx);
                                });
                                cx.notify();
                                false
                            }
                        }
                    })
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(settings_row(
                            "幅",
                            NumberInput::new(&width).small().w_full(),
                        ))
                        .child(settings_row(
                            "高さ",
                            NumberInput::new(&height).small().w_full(),
                        ))
                        .child(settings_row(
                            "フレームレート",
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().flex_1().child(
                                    NumberInput::new(&frame_rate_numerator).small().w_full(),
                                ))
                                .child(div().flex_none().child("/"))
                                .child(div().flex_1().child(
                                    NumberInput::new(&frame_rate_denominator).small().w_full(),
                                )),
                        )),
                )
        });
    }

    pub(crate) fn window_title(&self, cx: &App) -> String {
        let dirty = self.runtime.editor.read(cx).project_revision() != self.saved_revision;
        let Some(name) = self
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|name| name.to_string_lossy().into_owned())
        else {
            return format!("{}Zerium", if dirty { "*" } else { "" });
        };
        format!("{}{} — Zerium", if dirty { "*" } else { "" }, name)
    }

    fn request_project_change(
        &mut self,
        operation: PendingProjectChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if self.runtime.editor.read(cx).project_revision() == self.saved_revision {
            self.perform_project_change(operation, window, cx);
            return;
        }

        let controller = cx.entity();
        window.open_modal(cx, move |modal: Modal, _, _| {
            let confirm_controller = controller.clone();
            modal
                .title(
                    div()
                        .font_family(".SystemUIFont")
                        .font_normal()
                        .child("未保存の変更"),
                )
                .confirm()
                .button_props(
                    ModalButtonProps::default()
                        .ok_text("変更を破棄")
                        .cancel_text("キャンセル"),
                )
                .on_ok(move |_, window, cx| {
                    confirm_controller.update(cx, |controller, cx| {
                        controller.perform_project_change(operation, window, cx);
                    });
                    true
                })
                .child("保存されていない変更があります。この変更を破棄しますか？")
        });
    }

    fn perform_project_change(
        &mut self,
        operation: PendingProjectChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match operation {
            PendingProjectChange::New => self.new_project(cx),
            PendingProjectChange::Open => self.choose_open_path(window, cx),
        }
    }

    fn new_project(&mut self, cx: &mut Context<Self>) {
        self.runtime.advance_session(cx);
        self.runtime.reset_transient_state(cx);
        self.runtime.editor.update(cx, |editor, cx| {
            editor.reset(ProjectResolution::DEFAULT, FrameRate::FPS_30, Frame::new(0));
            cx.notify();
        });
        self.path = None;
        self.saved_revision = self.runtime.editor.read(cx).project_revision();
        cx.notify();
    }

    fn choose_open_path(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Zeriumプロジェクトを開く".into()),
        });
        self._dialog_task = cx.spawn(async move |controller, cx| {
            let selected: Result<Option<PathBuf>, String> = match receiver.await {
                Ok(Ok(Some(paths))) => Ok(paths.into_iter().next()),
                Ok(Ok(None)) => Ok(None),
                Ok(Err(error)) => Err(format!("ファイルを選択できません: {error}")),
                Err(error) => Err(format!(
                    "ファイル選択ダイアログから応答を取得できません: {error}"
                )),
            };
            match selected {
                Err(message) => {
                    set_failed(&controller, message, cx);
                }
                Ok(None) => {
                    set_idle(&controller, cx);
                }
                Ok(Some(path)) => {
                    controller
                        .update(cx, |controller, cx| controller.open_path(path, cx))
                        .ok();
                }
            }
        });
    }

    fn choose_save_path(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let initial_directory = self
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let suggested_name = self
            .path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("project.zero");
        let receiver = cx.prompt_for_new_path(&initial_directory, Some(suggested_name));
        self.busy = true;
        let session = self.runtime.session().clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::Save);
            cx.notify();
            operation
        });
        cx.notify();

        self._dialog_task = cx.spawn(async move |controller, cx| {
            let selected = match receiver.await {
                Ok(Ok(path)) => path,
                Ok(Err(error)) => {
                    if session.update(cx, |session, _| session.operation_is_current(operation)) {
                        set_failed(&controller, format!("保存先を選択できません: {error}"), cx);
                        session.update(cx, |session, cx| {
                            if session.finish(operation) {
                                cx.notify();
                            }
                        });
                    }
                    return;
                }
                Err(error) => {
                    if session.update(cx, |session, _| session.operation_is_current(operation)) {
                        set_failed(
                            &controller,
                            format!("保存先ダイアログから応答を取得できません: {error}"),
                            cx,
                        );
                        session.update(cx, |session, cx| {
                            if session.finish(operation) {
                                cx.notify();
                            }
                        });
                    }
                    return;
                }
            };
            if !session.update(cx, |session, _| session.operation_is_current(operation)) {
                return;
            }
            let Some(mut path) = selected else {
                set_idle(&controller, cx);
                session.update(cx, |session, cx| {
                    if session.finish(operation) {
                        cx.notify();
                    }
                });
                return;
            };
            if !has_project_extension(&path) {
                path.set_extension(PROJECT_EXTENSION);
            }
            session.update(cx, |session, cx| {
                if session.finish(operation) {
                    cx.notify();
                }
            });
            controller
                .update(cx, |controller, cx| controller.save_to(path, cx))
                .ok();
        });
    }

    fn save_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let snapshot = self.runtime.editor.read(cx).snapshot();
        let revision = snapshot.project_revision();
        self.busy = true;
        cx.notify();

        let session = self.runtime.session().clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::Save);
            cx.notify();
            operation
        });
        self._io_task = cx.spawn(async move |controller, cx| {
            let output = path.clone();
            let result = cx
                .background_spawn(async move { project_io::save(&snapshot, &output) })
                .await;
            if !session.update(cx, |session, _| session.operation_is_current(operation)) {
                return;
            }
            controller
                .update(cx, |controller, cx| {
                    controller.busy = false;
                    match result {
                        Ok(()) => {
                            controller.path = Some(path.clone());
                            controller.saved_revision = revision;
                            controller.notifications.update(cx, |notifications, cx| {
                                notifications.push_success(
                                    format!(
                                        "保存完了: {}",
                                        path.file_name()
                                            .map(|name| name.to_string_lossy())
                                            .unwrap_or_default()
                                    ),
                                    cx,
                                );
                            });
                        }
                        Err(error) => {
                            let message = format!("保存失敗: {error}");
                            controller.notifications.update(cx, |notifications, cx| {
                                notifications.push(message, cx);
                            });
                        }
                    }
                    cx.notify();
                })
                .ok();
            session.update(cx, |session, cx| {
                if session.finish(operation) {
                    cx.notify();
                }
            });
        });
    }

    pub(crate) fn should_close(
        &mut self,
        export_busy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.busy || export_busy || self.runtime.session().read(cx).is_busy() {
            let activities = self.runtime.session().read(cx).busy_activities();
            let activity = if activities.is_empty() {
                if export_busy {
                    "書き出し".to_owned()
                } else {
                    "保存または読み込み".to_owned()
                }
            } else {
                activities
                    .iter()
                    .map(|activity| match activity {
                        ProjectActivity::Import => "ファイル読み込み",
                        ProjectActivity::Probe => "メディア解析",
                        ProjectActivity::Save => "保存",
                        ProjectActivity::Load => "プロジェクト読み込み",
                        ProjectActivity::Export => "書き出し",
                    })
                    .collect::<Vec<_>>()
                    .join("・")
            };
            self.notifications.update(cx, |notifications, cx| {
                notifications.push(format!("{activity}の完了後に終了してください"), cx);
            });
            return false;
        }
        if self.runtime.editor.read(cx).project_revision() == self.saved_revision {
            return true;
        }

        window.open_modal(cx, move |modal: Modal, _, _| {
            modal
                .title(
                    div()
                        .font_family(".SystemUIFont")
                        .font_normal()
                        .child("未保存の変更"),
                )
                .confirm()
                .button_props(
                    ModalButtonProps::default()
                        .ok_text("変更を破棄して終了")
                        .cancel_text("キャンセル"),
                )
                .on_ok(|_, window, cx| {
                    window.defer(cx, |window, _| window.remove_window());
                    true
                })
                .child("保存されていない変更があります。破棄して終了しますか？")
        });
        false
    }
}

fn settings_row(label: &'static str, input: impl IntoElement) -> gpui::Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_3()
        .child(div().w(gpui::px(120.)).flex_none().child(label))
        .child(div().min_w_0().flex_1().child(input))
}

fn has_project_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PROJECT_EXTENSION))
}

fn set_failed(
    controller: &gpui::WeakEntity<ProjectController>,
    error: String,
    cx: &mut gpui::AsyncApp,
) {
    controller
        .update(cx, |controller, cx| {
            controller.busy = false;
            controller.notifications.update(cx, |notifications, cx| {
                notifications.push(error, cx);
            });
            cx.notify();
        })
        .ok();
}

fn set_idle(controller: &gpui::WeakEntity<ProjectController>, cx: &mut gpui::AsyncApp) {
    controller
        .update(cx, |controller, cx| {
            controller.busy = false;
            cx.notify();
        })
        .ok();
}
