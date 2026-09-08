use std::collections::HashSet;

use gpui::{App, ClipboardItem, Context};

use crate::{
    domain::{
        persistence::{
            DecodedTimelineClipboard, decode_timeline_clipboard, encode_timeline_clipboard,
        },
        timeline::{Frame, LayerId, TimelineEditor},
    },
    ui::TimelineEditorEntityExt as _,
};

use super::Timeline;

impl Timeline {
    pub(crate) fn can_copy_items(&self, cx: &App) -> bool {
        self.editor.read(cx).selected_item_ids().next().is_some()
    }

    pub(crate) fn can_paste_items(&self, cx: &App) -> bool {
        self.clipboard_data(cx).is_some()
    }

    pub(crate) fn copy_selected_items(&mut self, cx: &mut Context<Self>) -> bool {
        let (item_count, metadata) = {
            let editor = self.editor.read(cx);
            let selected = editor.selected_item_ids().collect::<HashSet<_>>();
            if selected.is_empty() {
                return false;
            }
            let mut items = selected
                .iter()
                .filter_map(|id| Some((editor.item_layer(*id)?, editor.item(*id)?.clone())))
                .collect::<Vec<_>>();
            items.sort_unstable_by_key(|(layer, item)| {
                (layer.get(), item.start.get(), item.id.get())
            });
            let source_scene = editor.active_scene_id();
            let scene_bindings = source_scene
                .and_then(|scene_id| editor.scene(scene_id))
                .into_iter()
                .flat_map(|scene| &scene.arguments)
                .flat_map(|argument| {
                    argument
                        .bindings
                        .iter()
                        .filter(|binding| selected.contains(&binding.item_id()))
                        .cloned()
                        .map(|binding| (argument.schema.id().to_owned(), binding))
                })
                .collect::<Vec<_>>();
            let Ok(metadata) = encode_timeline_clipboard(&items, source_scene, &scene_bindings)
            else {
                return false;
            };
            (items.len(), metadata)
        };
        cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(
            format!("Zeriumのアイテム {item_count}件"),
            metadata,
        ));
        true
    }

    pub(crate) fn cut_selected_items(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.copy_selected_items(cx) {
            return false;
        }
        self.stop_playback(cx);
        self.editor
            .update_if_changed(cx, TimelineEditor::remove_selected_item)
    }

    fn clipboard_data(&self, cx: &App) -> Option<DecodedTimelineClipboard> {
        let item = cx.read_from_clipboard()?;
        decode_timeline_clipboard(item.metadata()?, self.editor.read(cx)).ok()
    }

    pub(crate) fn paste_items(&mut self, cx: &mut Context<Self>) -> bool {
        self.paste_items_at(None, cx)
    }

    pub(super) fn paste_items_at(
        &mut self,
        target: Option<(LayerId, Frame)>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(clipboard) = self.clipboard_data(cx) else {
            return false;
        };
        let default_layer = clipboard
            .items
            .iter()
            .map(|(layer, _)| *layer)
            .min_by_key(|layer| layer.get())
            .unwrap_or_else(|| LayerId::new(0));
        let (target_layer, target_start) = target.unwrap_or_else(|| {
            let editor = self.editor.read(cx);
            (default_layer, editor.playhead())
        });
        self.stop_playback(cx);
        self.editor.update(cx, |editor, cx| {
            let pasted = editor.paste_items(
                &clipboard.items,
                clipboard.source_scene,
                &clipboard.scene_bindings,
                target_layer,
                target_start,
            );
            if pasted.is_some() {
                cx.notify();
            }
            pasted.is_some()
        })
    }
}
