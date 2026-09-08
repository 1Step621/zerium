use std::{path::PathBuf, sync::Arc};

use ::ui::{
    ContextModal as _, StyledExt as _,
    modal::{Modal, ModalButtonProps},
};
use gpui::{Context, Entity, SharedString, Subscription, Task, Window, div, prelude::*, px};

use crate::{
    domain::timeline::TimelineEditor,
    engine::{
        export::{ExportSettings, export_timeline},
        media::MediaReaderRegistry,
    },
    ui::{
        preview::RenderBackend,
        session::{
            ProjectActivity, ProjectOperation, ProjectSession, ProjectSessionId, UiNotifications,
        },
    },
};

enum ExportState {
    Idle,
    ChoosingPath,
    Exporting(PathBuf),
    Complete(PathBuf),
    Failed(String),
}

pub(crate) struct ExportController {
    editor: Entity<TimelineEditor>,
    backend: Entity<RenderBackend>,
    media_readers: Arc<MediaReaderRegistry>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    state: ExportState,
    _task: Task<()>,
    _session_subscription: Subscription,
}

impl ExportController {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        backend: Entity<RenderBackend>,
        media_readers: Arc<MediaReaderRegistry>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = session.read(cx).id();
        let session_subscription = cx.observe(&session, |this, _, cx| {
            let session_id = this.session.read(cx).id();
            if session_id == this.session_id {
                return;
            }
            this.session_id = session_id;
            this._task = Task::ready(());
            this.state = ExportState::Idle;
            cx.notify();
        });
        Self {
            editor,
            backend,
            media_readers,
            session,
            session_id,
            notifications,
            state: ExportState::Idle,
            _task: Task::ready(()),
            _session_subscription: session_subscription,
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        matches!(
            self.state,
            ExportState::ChoosingPath | ExportState::Exporting(_)
        )
    }

    pub(crate) fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let resolution = editor.resolution();
        let frame_count = editor.end_frame_exclusive();
        let duration_label = format_duration(frame_rate.frame_to_seconds(frame_count));
        let frame_rate_label = format!("{:.2} fps", frame_rate.frames_per_second());
        let controller = cx.entity();
        window.open_modal(cx, move |modal: Modal, _, _| {
            let confirm_controller = controller.clone();
            modal
                .title(
                    div()
                        .font_family(".SystemUIFont")
                        .font_normal()
                        .child("書き出し"),
                )
                .width(px(440.))
                .confirm()
                .button_props(
                    ModalButtonProps::default()
                        .ok_text("保存先を選択")
                        .cancel_text("キャンセル"),
                )
                .on_ok(move |_, window, cx| {
                    confirm_controller.update(cx, |controller, cx| {
                        controller.choose_output(window, cx);
                    });
                    true
                })
                .child(summary_row("形式", "MP4 / H.264 + AAC"))
                .child(summary_row(
                    "解像度",
                    format!("{} × {}", resolution.width(), resolution.height()),
                ))
                .child(summary_row("フレームレート", frame_rate_label.clone()))
                .child(summary_row("長さ", duration_label.clone()))
        });
    }

    pub(crate) fn status(&self) -> Option<String> {
        match &self.state {
            ExportState::Idle => None,
            ExportState::ChoosingPath => Some("保存先を選択中…".to_owned()),
            ExportState::Exporting(path) => Some(format!("書き出し中… {}", output_name(path))),
            ExportState::Complete(path) => Some(format!("書き出し完了: {}", output_name(path))),
            ExportState::Failed(error) => Some(format!("書き出し失敗: {error}")),
        }
    }

    fn choose_output(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(renderer) = self.backend.read(cx).renderer() else {
            let message = "GPUレンダラーが利用できないため書き出しを開始できません".to_owned();
            self.state = ExportState::Failed(message.clone());
            self.notifications
                .update(cx, |notifications, cx| notifications.push(message, cx));
            cx.notify();
            return;
        };
        let snapshot = self.editor.read(cx).snapshot();
        let media_readers = self.media_readers.clone();
        let initial_directory = directories::UserDirs::new()
            .and_then(|directories| {
                directories
                    .video_dir()
                    .or_else(|| Some(directories.home_dir()))
                    .map(ToOwned::to_owned)
            })
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&initial_directory, Some("zerium-export.mp4"));
        let session = self.session.clone();
        let notifications = self.notifications.clone();
        let operation =
            session.update(cx, |session, cx| session.begin(ProjectActivity::Export, cx));
        self.state = ExportState::ChoosingPath;
        cx.notify();

        self._task = cx.spawn(async move |controller, cx| {
            let selected = match receiver.await {
                Ok(Ok(path)) => path,
                Ok(Err(error)) => {
                    fail_operation(
                        &controller,
                        &session,
                        &notifications,
                        operation,
                        format!("保存先を選択できません: {error}"),
                        cx,
                    );
                    return;
                }
                Err(error) => {
                    fail_operation(
                        &controller,
                        &session,
                        &notifications,
                        operation,
                        format!("保存先ダイアログから応答を取得できません: {error}"),
                        cx,
                    );
                    return;
                }
            };
            let Some(mut output) = selected else {
                if operation_is_current(&session, operation, cx) {
                    let _ = controller.update(cx, |controller, cx| {
                        controller.state = ExportState::Idle;
                        cx.notify();
                    });
                    session.update(cx, |session, cx| {
                        session.finish(operation, cx);
                    });
                }
                return;
            };
            if !output
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
            {
                output.set_extension("mp4");
            }
            if !operation_is_current(&session, operation, cx) {
                return;
            }
            let _ = controller.update(cx, |controller, cx| {
                controller.state = ExportState::Exporting(output.clone());
                cx.notify();
            });
            let settings = ExportSettings {
                output: output.clone(),
            };
            let result = cx
                .background_spawn(async move {
                    export_timeline(snapshot, renderer, media_readers, settings)
                })
                .await;
            if !operation_is_current(&session, operation, cx) {
                return;
            }
            let error = result.as_ref().err().map(ToString::to_string);
            let _ = controller.update(cx, |controller, cx| {
                controller.state = match result {
                    Ok(()) => ExportState::Complete(output),
                    Err(error) => ExportState::Failed(error.to_string()),
                };
                cx.notify();
            });
            if let Some(error) = error {
                notifications.update(cx, |notifications, cx| {
                    notifications.push(format!("書き出し失敗: {error}"), cx);
                });
            }
            session.update(cx, |session, cx| {
                session.finish(operation, cx);
            });
        });
    }
}

fn operation_is_current(
    session: &Entity<ProjectSession>,
    operation: ProjectOperation,
    cx: &mut gpui::AsyncApp,
) -> bool {
    session.update(cx, |session, _| session.operation_is_current(operation))
}

fn fail_operation(
    controller: &gpui::WeakEntity<ExportController>,
    session: &Entity<ProjectSession>,
    notifications: &Entity<UiNotifications>,
    operation: ProjectOperation,
    error: String,
    cx: &mut gpui::AsyncApp,
) {
    if !operation_is_current(session, operation, cx) {
        return;
    }
    let _ = controller.update(cx, |controller, cx| {
        controller.state = ExportState::Failed(error.clone());
        cx.notify();
    });
    notifications.update(cx, |notifications, cx| {
        notifications.push(format!("書き出し失敗: {error}"), cx);
    });
    session.update(cx, |session, cx| {
        session.finish(operation, cx);
    });
}

fn summary_row(label: &'static str, value: impl Into<SharedString>) -> gpui::Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(120.))
                .flex_none()
                .text_color(gpui::rgb(0x888888))
                .child(label),
        )
        .child(div().min_w_0().flex_1().child(value.into()))
}

fn output_name(path: &std::path::Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output.mp4")
}

fn format_duration(seconds: f64) -> String {
    let total_seconds = seconds.max(0.).ceil() as u64;
    let hours = total_seconds / 3_600;
    let minutes = total_seconds % 3_600 / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}
