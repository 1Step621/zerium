use gpui::{Context, Entity};

use zerium_core::timeline::TimelineEditor;

pub(crate) mod animation_curve;
pub(crate) mod explorer;
pub(crate) mod export;
pub(crate) mod file_input;
mod input;
mod number_input;
mod numeric_property;
pub(crate) mod pane;
pub(crate) mod preview;
pub(crate) mod project_dialogs;
pub(crate) mod property_inspector;
pub(crate) mod scene_settings;
pub(crate) mod search_picker;
pub(crate) mod session;
pub(crate) mod theme;
pub(crate) mod time_grid;
pub(crate) mod timeline;
pub(crate) mod transport;
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
