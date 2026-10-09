use rust_i18n::t;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use ::ui::{
    ContextModal as _, Disableable as _, IconName, Sizable as _, StyledExt as _,
    button::Button,
    modal::{Modal, ModalButtonProps},
};
use futures::StreamExt as _;
use gpui::{App, Context, Entity, SharedString, Subscription, Task, Window, div, prelude::*, px};

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

struct ExportJob {
    operation: ProjectOperation,
    cancelled: Arc<AtomicBool>,
    completed_frames: u64,
    total_frames: u64,
}

pub(crate) struct ExportController {
    editor: Entity<TimelineEditor>,
    render_runtime: Entity<RenderRuntime>,
    media_readers: Arc<MediaReaderRegistry>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    job: Option<ExportJob>,
    output_selection: Option<ProjectOperation>,
    output: Option<PathBuf>,
    _export_task: Task<()>,
    _output_selection_task: Task<()>,
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
            if session_id != this.session_id {
                this.cancel(cx);
                this.session_id = session_id;
                this._export_task = Task::ready(());
                this._output_selection_task = Task::ready(());
                this.job = None;
                this.output_selection = None;
                this.output = None;
            }
            cx.notify();
        });
        Self {
            editor,
            render_runtime,
            media_readers,
            session,
            session_id,
            notifications,
            job: None,
            output_selection: None,
            output: None,
            _export_task: Task::ready(()),
            _output_selection_task: Task::ready(()),
            _session_subscription: session_subscription,
        }
    }

    pub(crate) fn is_exporting(&self) -> bool {
        self.job.is_some()
    }

    /// Completed and total frame counts while an export is running.
    pub(crate) fn export_progress(&self) -> Option<(u64, u64)> {
        self.job
            .as_ref()
            .map(|job| (job.completed_frames, job.total_frames))
    }

    pub(crate) fn is_cancelling(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| job.cancelled.load(Ordering::Relaxed))
    }

    pub(crate) fn cancel(&self, cx: &mut Context<Self>) {
        if let Some(job) = &self.job {
            job.cancelled.store(true, Ordering::Relaxed);
            cx.notify();
        }
    }

    pub(crate) fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_exporting() || self.output_selection.is_some() {
            return;
        }
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let resolution = editor.resolution();
        let frame_count = editor.end_frame_exclusive();
        let duration_label = format_duration(frame_rate.frame_to_seconds(frame_count));
        let frame_rate_label = format!("{:.2} fps", frame_rate.frames_per_second());
        let controller = cx.entity();
        let session_id = self.session.read(cx).id();
        window.open_modal(cx, move |modal: Modal, _, cx| {
            let export = controller.read(cx);
            let output = export.output_path(cx).display().to_string();
            let busy = export.is_exporting() || export.output_selection.is_some();
            let destination_controller = controller.clone();
            let confirm_controller = controller.clone();
            modal
                .title(
                    div()
                        .font_family(crate::ui::theme::FONT_FAMILY)
                        .font_normal()
                        .child(t!("export.title").to_string()),
                )
                .width(px(560.))
                .confirm()
                .button_props(
                    ModalButtonProps::default()
                        .ok_text(t!("export.start").to_string())
                        .cancel_text(t!("common.cancel").to_string()),
                )
                .on_ok(move |_, window, cx| {
                    confirm_controller.update(cx, |controller, cx| {
                        if controller.is_exporting()
                            || controller.output_selection.is_some()
                            || !controller.session.read(cx).is_current(session_id)
                        {
                            return false;
                        }
                        controller.request_export(window, cx);
                        true
                    })
                })
                .child(summary_row(
                    t!("export.destination").to_string(),
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().min_w_0().flex_1().truncate().child(output.clone()))
                        .child(
                            Button::new("export-destination")
                                .small()
                                .icon(IconName::Folder)
                                .tooltip(format!("{}\n{output}", t!("export.choose_destination")))
                                .disabled(busy)
                                .on_click(move |_, _, cx| {
                                    destination_controller.update(cx, |controller, cx| {
                                        if controller.session.read(cx).is_current(session_id) {
                                            controller.choose_output(cx);
                                        }
                                    });
                                }),
                        ),
                ))
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

    fn output_path(&self, cx: &App) -> PathBuf {
        if let Some(output) = &self.output {
            return output.clone();
        }
        if let Some(path) = self.session.read(cx).path() {
            return path.with_extension("mp4");
        }
        let initial_directory = directories::UserDirs::new()
            .map(|directories| {
                directories
                    .video_dir()
                    .unwrap_or(directories.home_dir())
                    .to_path_buf()
            })
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        initial_directory.join("output.mp4")
    }

    fn choose_output(&mut self, cx: &mut Context<Self>) {
        if self.is_exporting() || self.output_selection.is_some() {
            return;
        }
        let output = self.output_path(cx);
        let receiver = cx.prompt_for_new_path(
            output.parent().unwrap_or(Path::new(".")),
            output.file_name().and_then(|name| name.to_str()),
        );
        let session = self.session.clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::SelectFile);
            cx.notify();
            operation
        });
        self.output_selection = Some(operation);
        cx.notify();
        self._output_selection_task = cx.spawn(async move |controller, cx| {
            let result = receiver
                .await
                .map_err(|error| t!("export.destination_picker_failed", error = error).to_string())
                .and_then(|result| {
                    result.map_err(|error| {
                        t!("export.select_destination_failed", error = error).to_string()
                    })
                });
            let _ = controller.update(cx, |controller, cx| {
                if controller.output_selection != Some(operation) {
                    return;
                }
                let current = controller.session.update(cx, |session, cx| {
                    let current = session.finish(operation);
                    if current {
                        cx.notify();
                    }
                    current
                });
                if !current {
                    return;
                }
                controller.output_selection = None;
                match result {
                    Ok(Some(mut output)) => {
                        if !output
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
                        {
                            output.set_extension("mp4");
                        }
                        controller.output = Some(output);
                    }
                    Ok(None) => {}
                    Err(error) => controller.notifications.update(cx, |notifications, cx| {
                        notifications.push(error, cx);
                    }),
                }
                cx.notify();
            });
        });
    }

    fn request_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let output = self.output_path(cx);
        if !output.exists() {
            self.start_export(output, cx);
            return;
        }
        let controller = cx.entity();
        let session_id = self.session.read(cx).id();
        // Open after the export settings modal closes.
        window.defer(cx, move |window, cx| {
            window.open_modal(cx, move |modal: Modal, _, _| {
                let controller = controller.clone();
                let destination = output.clone();
                modal
                    .title(
                        div()
                            .font_family(crate::ui::theme::FONT_FAMILY)
                            .font_normal()
                            .child(t!("export.overwrite_title").to_string()),
                    )
                    .confirm()
                    .button_props(
                        ModalButtonProps::default()
                            .ok_text(t!("export.overwrite").to_string())
                            .cancel_text(t!("common.cancel").to_string()),
                    )
                    .on_ok(move |_, _, cx| {
                        controller.update(cx, |controller, cx| {
                            if controller.session.read(cx).is_current(session_id) {
                                controller.start_export(destination.clone(), cx);
                            }
                        });
                        true
                    })
                    .child(t!("export.overwrite_confirmation").to_string())
                    .child(div().text_sm().child(output.display().to_string()))
            });
        });
    }

    fn start_export(&mut self, output: PathBuf, cx: &mut Context<Self>) {
        if self.is_exporting() || self.output_selection.is_some() {
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
        let session = self.session.clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::Export);
            cx.notify();
            operation
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let total_frames = snapshot.end_frame_exclusive().get();
        self.job = Some(ExportJob {
            operation,
            cancelled: cancelled.clone(),
            completed_frames: 0,
            total_frames,
        });
        cx.notify();

        self._export_task = cx.spawn(async move |controller, cx| {
            if !session.read_with(cx, |session, _| session.operation_is_current(operation)) {
                return;
            }
            let result = async {
                let settings = ExportSettings {
                    output: output.clone(),
                };
                let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded();
                // Blocking export runs on its own thread; the UI only consumes progress.
                std::thread::Builder::new()
                    .name("zerium-export".to_owned())
                    .spawn(move || {
                        let result = export_timeline(
                            snapshot,
                            renderer,
                            media_readers,
                            settings,
                            &cancelled,
                            progress_tx.clone(),
                        );
                        let _ = progress_tx.unbounded_send(ExportProgress::Finished(result));
                    })
                    .map_err(ExportError::encoding)?;
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
                                    controller.update_progress(operation, completed, cx);
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
                result?;
                let fps = total_frames as f64 / started.elapsed().as_secs_f64().max(f64::EPSILON);
                Ok(t!(
                    "export.complete",
                    name = output_name(&output),
                    fps = format!("{fps:.1}")
                )
                .to_string())
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
        completed: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(job) = &mut self.job
            && job.operation == operation
            && self.session.read(cx).operation_is_current(operation)
        {
            job.completed_frames = completed.min(job.total_frames);
            cx.notify();
        }
    }

    fn finish_export(
        &mut self,
        operation: ProjectOperation,
        result: Result<String, ExportError>,
        cx: &mut Context<Self>,
    ) {
        if self
            .job
            .as_ref()
            .is_none_or(|job| job.operation != operation)
        {
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
        self.job = None;
        cx.notify();
        match result {
            Ok(message) => {
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push_success(message, cx);
                });
            }
            Err(ExportError::Cancelled) => {}
            Err(error) => {
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push(t!("export.failed", error = error).to_string(), cx);
                });
            }
        }
    }
}

fn summary_row(label: impl Into<SharedString>, value: impl IntoElement) -> gpui::Div {
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
        .child(div().min_w_0().flex_1().child(value))
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
