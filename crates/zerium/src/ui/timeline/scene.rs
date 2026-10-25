use rust_i18n::t;

use gpui::{ClickEvent, Context, Window};

use zerium_core::timeline::{ItemId, SceneId, TimelineEditor, TimelineItem};

use crate::ui::TimelineEditorEntityExt as _;

use crate::ui::timeline::Timeline;

impl Timeline {
    pub(super) fn switch_scene(&mut self, scene_id: Option<SceneId>, cx: &mut Context<Self>) {
        if self.editor.read(cx).active_scene_id() == scene_id {
            return;
        }
        self.stop_playback(cx);
        while self.editor.read(cx).active_scene_id().is_some() {
            self.editor
                .update_if_changed(cx, TimelineEditor::close_scene);
        }
        if let Some(scene_id) = scene_id {
            self.editor
                .update_if_changed(cx, |editor| editor.open_scene(scene_id));
        }
    }

    pub(super) fn group_selected_as_scene(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.editor.update(cx, |editor, cx| {
            if editor
                .group_selected_as_scene(t!("timeline.new_scene").as_ref())
                .is_some()
            {
                cx.notify();
            }
        });
    }

    pub(super) fn open_scene_item(
        &mut self,
        item_id: ItemId,
        event: &ClickEvent,
        cx: &mut Context<Self>,
    ) {
        if event.click_count() < 2 {
            return;
        }
        let scene_id = self
            .editor
            .read(cx)
            .item(item_id)
            .and_then(TimelineItem::scene_id);
        let Some(scene_id) = scene_id else {
            return;
        };
        self.stop_playback(cx);
        self.editor
            .update_if_changed(cx, |editor| editor.open_scene(scene_id));
    }

    pub(super) fn delete_empty_scene(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_playback(cx);
        self.editor.update_if_changed(cx, |editor| {
            let Some(scene_id) = editor.active_scene_id() else {
                return false;
            };
            if !editor.scene(scene_id).is_some_and(|scene| scene.is_empty()) {
                return false;
            }
            editor.delete_scene(scene_id)
        });
    }
}
