use std::{collections::VecDeque, path::PathBuf};

use gpui::{Context, SharedString};

#[derive(Clone)]
pub(super) struct QueuedNotification {
    pub(super) message: SharedString,
    pub(super) success: bool,
    pub(super) file_path: Option<PathBuf>,
}

#[derive(Default)]
pub(crate) struct UiNotifications {
    messages: VecDeque<QueuedNotification>,
    total_pushed: u64,
}

impl UiNotifications {
    const MAX_MESSAGES: usize = 16;

    /// Pushes a failure notification.
    pub(crate) fn push(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push_inner(message.into(), false, None);
        cx.notify();
    }

    /// Pushes a success notification.
    pub(crate) fn push_success(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.push_inner(message.into(), true, None);
        cx.notify();
    }

    /// Pushes a success notification with an action to reveal the saved file.
    pub(crate) fn push_success_with_file(
        &mut self,
        message: impl Into<SharedString>,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.push_inner(message.into(), true, Some(path));
        cx.notify();
    }

    fn push_inner(&mut self, message: SharedString, success: bool, file_path: Option<PathBuf>) {
        if success {
            log::info!("{message}");
        } else {
            log::warn!("{message}");
        }
        if self.messages.len() == Self::MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(QueuedNotification {
            message,
            success,
            file_path,
        });
        self.total_pushed = self.total_pushed.saturating_add(1);
    }

    pub(super) fn unseen_since(&self, seen: u64) -> (Vec<QueuedNotification>, u64) {
        let held = self.messages.len() as u64;
        let first = self.total_pushed.saturating_sub(held);
        let start = seen.saturating_sub(first).min(held) as usize;
        let unseen = self.messages.iter().skip(start).cloned().collect();
        (unseen, self.total_pushed)
    }
}
