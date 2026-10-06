//! Lifetime and scheduling of automatic project metadata refreshes.

use std::{path::PathBuf, sync::Arc, time::Duration};

use futures::{FutureExt as _, StreamExt as _, channel::mpsc};
use gpui::{AppContext as _, Context, Entity, Subscription, Task};
use zerium_core::timeline::TimelineEditor;

use crate::{
    engine::media::{MediaMetadataUpdater, MediaReaderRegistry},
    project_session::ProjectSession,
    ui::session::UiNotifications,
};

enum RefreshRequest {
    Refresh,
    Retry(PathBuf),
}

pub(crate) struct MediaMetadataController {
    requests: mpsc::UnboundedSender<RefreshRequest>,
    _task: Task<()>,
    _editor_subscription: Subscription,
    _session_subscription: Subscription,
}

impl MediaMetadataController {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        readers: Arc<MediaReaderRegistry>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (requests, mut receiver) = mpsc::unbounded();
        let mut revision = (
            editor.read(cx).snapshot().project_id(),
            editor.read(cx).project_revision(),
        );
        let editor_subscription = cx.observe(&editor, {
            let requests = requests.clone();
            move |_, editor, cx| {
                let editor = editor.read(cx);
                let next = (editor.snapshot().project_id(), editor.project_revision());
                if next != revision {
                    revision = next;
                    let _ = requests.unbounded_send(RefreshRequest::Refresh);
                }
            }
        });
        let mut session_id = session.read(cx).id();
        let session_subscription = cx.observe(&session, {
            let requests = requests.clone();
            move |_, session, cx| {
                let next = session.read(cx).id();
                if next != session_id {
                    session_id = next;
                    let _ = requests.unbounded_send(RefreshRequest::Refresh);
                }
            }
        });
        let task = cx.spawn(async move |_, cx| {
            let mut updater = MediaMetadataUpdater::default();
            let mut previous_session = None;
            loop {
                let current_session = session.read_with(cx, |session, _| session.id());
                if previous_session != Some(current_session) {
                    updater = MediaMetadataUpdater::default();
                    previous_session = Some(current_session);
                }
                let snapshot = editor.read_with(cx, |editor, _| editor.snapshot());
                let project_id = snapshot.project_id();
                let readers = readers.clone();
                let (next_updater, files, errors) = cx
                    .background_spawn(async move {
                        let (files, errors) = updater.refresh(&snapshot, &readers);
                        (updater, files, errors)
                    })
                    .await;
                updater = next_updater;
                if session.read_with(cx, |session, _| session.id()) == current_session {
                    editor.update(cx, |editor, cx| {
                        if editor.snapshot().project_id() != project_id {
                            return;
                        }
                        let mut changed = false;
                        for file in files {
                            changed |= editor.cache_media_file(&file);
                        }
                        for error in errors {
                            notifications.update(cx, |notifications, cx| {
                                notifications.push(error.to_string(), cx)
                            });
                        }
                        if changed {
                            cx.notify();
                        }
                    });
                }
                // One worker handles both edit requests and periodic checks. Requests
                // arriving during a probe are consumed afterward, so retries cannot
                // be overwritten by results from an older background task.
                futures::select_biased! {
                    request = receiver.next().fuse() => match request {
                        Some(RefreshRequest::Retry(path)) => updater.retry(&path),
                        Some(RefreshRequest::Refresh) => {},
                        None => break,
                    },
                    _ = cx.background_executor().timer(Duration::from_secs(1)).fuse() => {},
                }
                while let Some(Some(request)) = receiver.next().now_or_never() {
                    if let RefreshRequest::Retry(path) = request {
                        updater.retry(&path);
                    }
                }
            }
        });
        Self {
            requests,
            _task: task,
            _editor_subscription: editor_subscription,
            _session_subscription: session_subscription,
        }
    }

    pub(crate) fn retry(&self, path: PathBuf) {
        let _ = self.requests.unbounded_send(RefreshRequest::Retry(path));
    }
}
