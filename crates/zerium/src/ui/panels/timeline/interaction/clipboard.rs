use rust_i18n::t;

use std::collections::HashSet;

use gpui::{App, ClipboardFormat, ClipboardItem, Context, Window};

use crate::ui::{TimelineEditorEntityExt as _, panels::timeline::Timeline};
use zerium_core::{
    persistence::{
        DecodedTimelineClipboard, ProjectError, decode_timeline_clipboard,
        encode_timeline_clipboard,
    },
    timeline::{Frame, LayerId, TimelineEditor},
};

const TIMELINE_CLIPBOARD_FORMAT: ClipboardFormat = ClipboardFormat {
    mime_type: "application/x-zerium-timeline-items",
    macos_uti: "dev.1step621.zerium.timeline-items",
};

impl Timeline {
    pub(crate) fn can_copy_items(&self, cx: &App) -> bool {
        self.editor.read(cx).selected_item_ids().next().is_some()
    }

    pub(crate) fn can_paste_items(&self, cx: &App) -> bool {
        self.clipboard_data(cx).is_ok()
    }

    pub(crate) fn copy_selected_items(&mut self, cx: &mut Context<Self>) -> bool {
        let (item_count, data) = {
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
                (layer.get(), item.start().get(), item.id.get())
            });
            let data = match encode_timeline_clipboard(&items, editor) {
                Ok(data) => data,
                Err(error) => {
                    log::warn!("Could not encode timeline clipboard data: {error}");
                    return false;
                }
            };
            (items.len(), data)
        };
        cx.write_to_clipboard(
            ClipboardItem::new_string(t!("clipboard.item_count", count = item_count).to_string())
                .with_custom(TIMELINE_CLIPBOARD_FORMAT, data.into_bytes()),
        );
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

    fn clipboard_data(&self, cx: &App) -> Result<DecodedTimelineClipboard, ProjectError> {
        let item = cx
            .read_from_clipboard_with_formats(&[TIMELINE_CLIPBOARD_FORMAT])
            .ok_or_else(|| ProjectError::invalid_data("Clipboard is empty or could not be read"))?;
        let data = item.custom(TIMELINE_CLIPBOARD_FORMAT).ok_or_else(|| {
            ProjectError::invalid_data("Clipboard does not contain timeline items")
        })?;
        let source = std::str::from_utf8(data).map_err(|error| {
            ProjectError::invalid_data(format!(
                "Timeline clipboard data is not valid UTF-8: {error}"
            ))
        })?;
        decode_timeline_clipboard(source, self.editor.read(cx))
    }

    pub(crate) fn paste_items(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        let layer = self
            .cursor_layer_at(window)
            .or(self.cursor_layer)
            .or_else(|| self.editor.read(cx).selected_items_layer());
        let target = layer.map(|layer| (layer, self.editor.read(cx).playhead()));
        self.paste_items_at(target, cx)
    }

    fn cursor_layer_at(&self, window: &Window) -> Option<LayerId> {
        let scroll = crate::ui::panels::timeline::layer_scroll_base(&self.layer_scroll);
        let bounds = scroll.bounds();
        let position = window.mouse_position();
        if !bounds.contains(&position) {
            return None;
        }
        let offset_y = f32::from(scroll.offset().y);
        let content_y = f32::from(position.y - bounds.origin.y) - offset_y;
        (content_y >= 0.).then(|| LayerId::new((content_y / self.viewport.layer_height) as u64))
    }

    pub(in crate::ui::panels::timeline) fn paste_items_at(
        &mut self,
        target: Option<(LayerId, Frame)>,
        cx: &mut Context<Self>,
    ) -> bool {
        let clipboard = match self.clipboard_data(cx) {
            Ok(clipboard) => clipboard,
            Err(error) => {
                log::warn!("Could not paste timeline items: {error}");
                return false;
            }
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
                &clipboard.hidden_effects,
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
