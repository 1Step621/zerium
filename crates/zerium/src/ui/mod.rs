use gpui::{Context, Entity};

use zerium_core::timeline::TimelineEditor;

pub(crate) mod components;
pub(crate) mod dialogs;
pub(crate) mod panels;
pub(crate) mod theme;
pub(crate) mod workspace;

pub(crate) trait TimelineEditorEntityExt {
    fn update_if_changed<T>(
        &self,
        cx: &mut Context<T>,
        update: impl FnOnce(&mut TimelineEditor) -> bool,
    ) -> bool;
}

impl TimelineEditorEntityExt for Entity<TimelineEditor> {
    fn update_if_changed<T>(
        &self,
        cx: &mut Context<T>,
        update: impl FnOnce(&mut TimelineEditor) -> bool,
    ) -> bool {
        self.update(cx, |editor, cx| {
            let changed = update(editor);
            if changed {
                cx.notify();
            }
            changed
        })
    }
}
