use ::ui::{Icon, assets::Assets};
use gpui::{App, Entity};

use zerium_core::timeline::TimelineEditor;

pub(crate) mod components;
pub(crate) mod dialogs;
pub(crate) mod panels;
pub(crate) mod theme;
pub(crate) mod workspace;

pub(crate) fn icon(id: &str) -> Icon {
    Icon::default().path(icon_path(id))
}

pub(crate) fn icon_exists(id: &str) -> bool {
    Assets::get(&icon_path(id)).is_some()
}

fn icon_path(id: &str) -> String {
    format!("icons/{id}.svg")
}

pub(crate) trait TimelineEditorEntityExt {
    fn update_if_changed(
        &self,
        cx: &mut App,
        update: impl FnOnce(&mut TimelineEditor) -> bool,
    ) -> bool;
}

impl TimelineEditorEntityExt for Entity<TimelineEditor> {
    fn update_if_changed(
        &self,
        cx: &mut App,
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
