use rust_i18n::t;

use std::{path::PathBuf, sync::Arc, time::Instant};

use ::ui::{
    ContextModal as _, StyledExt as _,
    modal::{Modal, ModalButtonProps},
};
use futures::StreamExt as _;
use gpui::{Context, Entity, SharedString, Subscription, Task, Window, div, prelude::*, px};

use crate::{
    engine::{
        export::{ExportError, ExportProgress, ExportSettings, export_timeline},
        media::MediaReaderRegistry,
        rendering::RenderRuntime,
    },
    project_session::{ProjectActivity, ProjectOperation, ProjectSession, ProjectSessionId},
    ui::session::UiNotifications,
};
use zerium_core::timeline::{TimelineEditor, TimelineView};

enum ExportState {
    Idle,
    ChoosingPath(ProjectOperation),
    Exporting {
        operation: ProjectOperation,
        completed_frames: u64,
        total_frames: u64,
    },
}

impl ExportState {
    fn operation(&self) -> Option<ProjectOperation> {
        match self {
            Self::ChoosingPath(operation) | Self::Exporting { operation, .. } => Some(*operation),
            _ => None,
        }
    }
}

pub(crate) struct ExportController {
    editor: Entity<TimelineEditor>,
    render_runtime: Entity<RenderRuntime>,
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
        render_runtime: Entity<RenderRuntime>,
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
            render_runtime,
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
        self.state.operation().is_some()
    }

    /// Progress fraction while exporting, if an export is running.
    pub(crate) fn export_progress(&self) -> Option<(u64, u64)> {
        match self.state {
            ExportState::Exporting {
                completed_frames,
                total_frames,
                ..
            } => Some((completed_frames, total_frames)),
            _ => None,
        }
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
                        .font_family(crate::ui::theme::FONT_FAMILY)
                        .font_normal()
                        .child(t!("export.title").to_string()),
                )
                .width(px(440.))
                .confirm()
                .button_props(
                    ModalButtonProps::default()
                        .ok_text(t!("export.choose_destination").to_string())
                        .cancel_text(t!("common.cancel").to_string()),
                )
                .on_ok(move |_, window, cx| {
                    confirm_controller.update(cx, |controller, cx| {
                        controller.choose_output(window, cx);
                    });
                    true
                })
                .child(summary_row(
                    t!("export.format").to_string(),
                    "MP4 / H.264 + AAC",
                ))
                .child(summary_row(
                    t!("export.resolution").to_string(),
                    format!("{} × {}", resolution.width(), resolution.height()),
                ))
                .child(summary_row(
                    t!("export.frame_rate").to_string(),
                    frame_rate_label.clone(),
                ))
                .child(summary_row(
                    t!("export.duration").to_string(),
                    duration_label.clone(),
                ))
        });
    }

    fn choose_output(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let renderer = match self
            .render_runtime
            .update(cx, |backend, _| backend.export_session())
        {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("dedicated export device unavailable, sharing preview device: {error}");
                let Some(renderer) = self.render_runtime.read(cx).renderer() else {
                    let message = t!("export.gpu_renderer_unavailable").to_string();
                    self.state = ExportState::Idle;
                    self.notifications
                        .update(cx, |notifications, cx| notifications.push(message, cx));
                    cx.notify();
                    return;
                };
                renderer
            }
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
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::Export);
            cx.notify();
            operation
        });
        self.state = ExportState::ChoosingPath(operation);
        cx.notify();

        self._task = cx.spawn(async move |controller, cx| {
            let result = async {
                let selected = receiver
                    .await
                    .map_err(|error| {
                        t!("export.destination_picker_failed", error = error).to_string()
                    })?
                    .map_err(|error| {
                        t!("export.select_destination_failed", error = error).to_string()
                    })?;
                let Some(mut output) = selected else {
                    return Ok(None);
                };
                if !session.read_with(cx, |session, _| session.operation_is_current(operation)) {
                    return Ok(None);
                }
                if !output
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
                {
                    output.set_extension("mp4");
                }
                let total_frames = snapshot.end_frame_exclusive().get();
                let _ = controller.update(cx, |controller, cx| {
                    controller.update_progress(operation, 0, total_frames, cx);
                });
                let settings = ExportSettings {
                    output: output.clone(),
                };
                let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded();
                // Blocking export runs on its own thread; the UI only consumes progress.
                let _ = std::thread::Builder::new()
                    .name("zerium-export".to_owned())
                    .spawn(move || {
                        let result = export_timeline(
                            snapshot,
                            renderer,
                            media_readers,
                            settings,
                            progress_tx.clone(),
                        );
                        let _ = progress_tx.unbounded_send(ExportProgress::Finished(result));
                    });
                // Limit progress updates to roughly 100 per export.
                let quantum = total_frames.div_ceil(100).max(1);
                let mut last_reported = 0;
                let started = Instant::now();
                let result = loop {
                    match progress_rx.next().await {
                        Some(ExportProgress::Frame(completed)) => {
                            if completed >= total_frames
                                || completed.saturating_sub(last_reported) >= quantum
                            {
                                last_reported = completed;
                                let _ = controller.update(cx, |controller, cx| {
                                    controller.update_progress(
                                        operation,
                                        completed,
                                        total_frames,
                                        cx,
                                    );
                                });
                            }
                        }
                        Some(ExportProgress::Finished(result)) => break result,
                        None => {
                            break Err(ExportError::encoding(
                                "Export thread terminated unexpectedly",
                            ));
                        }
                    }
                };
                result.map_err(|error| t!("export.failed", error = error).to_string())?;
                let fps = total_frames as f64 / started.elapsed().as_secs_f64().max(f64::EPSILON);
                Ok(Some(
                    t!(
                        "export.complete",
                        name = output_name(&output),
                        fps = format!("{fps:.1}")
                    )
                    .to_string(),
                ))
            }
            .await;
            let _ = controller.update(cx, |controller, cx| {
                controller.finish_export(operation, result, cx);
            });
        });
    }

    fn update_progress(
        &mut self,
        operation: ProjectOperation,
        completed_frames: u64,
        total_frames: u64,
        cx: &mut Context<Self>,
    ) {
        if self.state.operation() != Some(operation)
            || !self.session.read(cx).operation_is_current(operation)
        {
            return;
        }
        self.state = ExportState::Exporting {
            operation,
            completed_frames: completed_frames.min(total_frames),
            total_frames,
        };
        cx.notify();
    }

    fn finish_export(
        &mut self,
        operation: ProjectOperation,
        result: Result<Option<String>, String>,
        cx: &mut Context<Self>,
    ) {
        if self.state.operation() != Some(operation) {
            return;
        }
        let current = self.session.update(cx, |session, cx| {
            let current = session.finish(operation);
            if current {
                cx.notify();
            }
            current
        });
        if !current {
            return;
        }
        self.state = ExportState::Idle;
        match result {
            Ok(None) => {}
            Ok(Some(message)) => {
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push_success(message, cx);
                });
            }
            Err(error) => {
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push(error, cx);
                });
            }
        }
        cx.notify();
    }
}

fn summary_row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> gpui::Div {
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
                .child(label.into()),
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
