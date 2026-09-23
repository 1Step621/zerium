use std::collections::VecDeque;

use gpui::{Context, SharedString};

struct QueuedNotification {
    message: SharedString,
    success: bool,
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
        self.push_inner(message.into(), false);
        cx.notify();
    }

    /// Pushes a success notification.
    pub(crate) fn push_success(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.push_inner(message.into(), true);
        cx.notify();
    }

    fn push_inner(&mut self, message: SharedString, success: bool) {
        if self.messages.len() == Self::MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages
            .push_back(QueuedNotification { message, success });
        self.total_pushed = self.total_pushed.saturating_add(1);
    }

    pub(crate) fn unseen_since(&self, seen: u64) -> (Vec<(SharedString, bool)>, u64) {
        let held = self.messages.len() as u64;
        let first = self.total_pushed.saturating_sub(held);
        let start = seen.saturating_sub(first).min(held) as usize;
        let unseen = self
            .messages
            .iter()
            .skip(start)
            .map(|queued| (queued.message.clone(), queued.success))
            .collect();
        (unseen, self.total_pushed)
    }
}
