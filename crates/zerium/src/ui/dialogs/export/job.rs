use rust_i18n::t;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use futures::StreamExt as _;
use gpui::Context;

use crate::app::{
    export::{ExportError, ExportProgress, export_timeline},
    project::{ProjectActivity, ProjectOperation},
};
use zerium_core::timeline::TimelineView;

use super::{ExportController, ExportState};

pub(super) struct ExportJob {
    pub(super) operation: ProjectOperation,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) completed_frames: u64,
    pub(super) total_frames: u64,
}

impl ExportController {
    /// Completed and total frame counts while an export is running.
    pub(crate) fn export_progress(&self) -> Option<(u64, u64)> {
        match &self.state {
            ExportState::Running { job, .. } => Some((job.completed_frames, job.total_frames)),
            _ => None,
        }
    }

    pub(crate) fn is_cancelling(&self) -> bool {
        matches!(&self.state, ExportState::Running { job, .. }
            if job.cancelled.load(Ordering::Relaxed))
    }

    pub(crate) fn cancel(&self, cx: &mut Context<Self>) {
        if let ExportState::Running { job, .. } = &self.state {
            job.cancelled.store(true, Ordering::Relaxed);
            cx.notify();
        }
    }

    pub(super) fn start_export(&mut self, output: PathBuf, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let renderer = match self
            .render_runtime
            .update(cx, |backend, _| backend.export_session())
        {
            Ok(renderer) => renderer,
            Err(error) => {
                log::warn!("dedicated export device unavailable, sharing preview device: {error}");
                let Some(renderer) = self.render_runtime.read(cx).preview_session() else {
                    let message = t!("export.gpu_renderer_unavailable").to_string();
                    self.notifications
                        .update(cx, |notifications, cx| notifications.push(message, cx));
                    cx.notify();
                    return;
                };
                renderer.fork()
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
        let job = ExportJob {
            operation,
            cancelled: cancelled.clone(),
            completed_frames: 0,
            total_frames,
        };
        let task = cx.spawn(async move |controller, cx| {
            if !session.read_with(cx, |session, _| session.operation_is_current(operation)) {
                return;
            }
            let result = async {
                let destination = output.clone();
                let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded();
                // Blocking export runs on its own thread; the UI only consumes progress.
                std::thread::Builder::new()
                    .name("zerium-export".to_owned())
                    .spawn(move || {
                        let result = export_timeline(
                            snapshot,
                            renderer,
                            media_readers,
                            destination,
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
        self.state = ExportState::Running { job, _task: task };
        cx.notify();
    }

    fn update_progress(
        &mut self,
        operation: ProjectOperation,
        completed: u64,
        cx: &mut Context<Self>,
    ) {
        if let ExportState::Running { job, .. } = &mut self.state
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
        let ExportState::Running { job, .. } = &self.state else {
            return;
        };
        if job.operation != operation {
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

fn output_name(path: &std::path::Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output.mp4")
}
