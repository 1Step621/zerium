use std::{cell::Cell, rc::Rc};

use gpui::{Context, Empty, EntityId, MouseDownEvent, Render, Window, prelude::*};

use zerium_core::timeline::{
    Frame, FrameDuration, FrameRate, ItemId, LayerId, ResizeEdge, ResizeMode, TimelineEditor,
    TimelineItem,
};

use crate::ui::{TimelineEditorEntityExt as _, components::time_grid, panels::timeline::Timeline};

#[derive(Clone)]
pub(in crate::ui::panels::timeline) struct ResizeTimelineItem {
    pub(in crate::ui::panels::timeline) timeline_id: EntityId,
    pub(in crate::ui::panels::timeline) origins: Rc<[TimelineItem]>,
    pub(in crate::ui::panels::timeline) anchor_id: ItemId,
    pub(in crate::ui::panels::timeline) edge: ResizeEdge,
    // GPUI retains this payload; the entity returned at drag start is only a preview.
    pub(in crate::ui::panels::timeline) mode: Cell<ResizeMode>,
}

impl Render for ResizeTimelineItem {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone)]
pub(in crate::ui::panels::timeline) struct MoveTimelineItem {
    pub(in crate::ui::panels::timeline) timeline_id: EntityId,
    pub(in crate::ui::panels::timeline) item_id: ItemId,
}

impl Render for MoveTimelineItem {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui::panels::timeline) struct MovingItemOrigin {
    pub(in crate::ui::panels::timeline) item_id: ItemId,
    pub(in crate::ui::panels::timeline) source_layer: LayerId,
    pub(in crate::ui::panels::timeline) start: Frame,
    pub(in crate::ui::panels::timeline) duration: FrameDuration,
}

#[derive(Clone, Debug)]
pub(in crate::ui::panels::timeline) struct ItemMoveOrigin {
    pub(in crate::ui::panels::timeline) snap_playhead: Frame,
    pub(in crate::ui::panels::timeline) item_id: ItemId,
    pub(in crate::ui::panels::timeline) items: Vec<MovingItemOrigin>,
    pub(in crate::ui::panels::timeline) pointer_x: f32,
    pub(in crate::ui::panels::timeline) pointer_y: f32,
}

impl ItemMoveOrigin {
    fn delta_at(
        &self,
        pointer_x: f32,
        pointer_y: f32,
        pixels_per_second: f64,
        frame_rate: FrameRate,
        layer_height: f32,
    ) -> (i64, i64) {
        let delta_seconds = (pointer_x - self.pointer_x) as f64 / pixels_per_second;
        let delta_frames = frame_rate.seconds_delta_to_frames(delta_seconds);
        let layer_delta = ((pointer_y - self.pointer_y) / layer_height).round() as i64;
        self.clamp_delta(delta_frames, layer_delta)
    }

    fn clamp_delta(&self, frame_delta: i64, layer_delta: i64) -> (i64, i64) {
        let origins = self.items.iter().map(|item| {
            (
                item.start.get(),
                item.duration.get(),
                item.source_layer.get(),
            )
        });
        clamp_move_delta(origins, frame_delta, layer_delta)
    }
}

impl Timeline {
    pub(in crate::ui::panels::timeline) fn resize_item_from_pointer(
        &mut self,
        drag: &ResizeTimelineItem,
        pointer_x: f32,
        snap_disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id() {
            return;
        }

        let mut pointer = self.pointer_frame(pointer_x, window, cx);
        if !snap_disabled {
            pointer = self.snap_frame(pointer, &drag.origins, cx);
        }
        self.editor.update_if_changed(cx, |editor| {
            editor.resize_items(
                drag.origins.as_ref(),
                drag.anchor_id,
                drag.edge,
                pointer,
                drag.mode.get(),
            )
        });
    }

    fn prepare_item_move(
        &mut self,
        item_id: ItemId,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.cursor_layer = self.editor.read(cx).item_layer(item_id);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        if !self.editor.read(cx).is_item_selected(item_id) {
            self.editor
                .update_if_changed(cx, |editor| editor.select(item_id));
        }
        let items = {
            let editor = self.editor.read(cx);
            if editor.item(item_id).is_none() || editor.item_layer(item_id).is_none() {
                return;
            }
            let mut items = editor
                .selected_item_ids()
                .filter_map(|selected_id| {
                    let item = editor.item(selected_id)?;
                    let source_layer = editor.item_layer(selected_id)?;
                    Some(MovingItemOrigin {
                        item_id: selected_id,
                        source_layer,
                        start: item.start(),
                        duration: item.duration(editor.frame_rate()),
                    })
                })
                .collect::<Vec<_>>();
            items.sort_unstable_by_key(|item| item.item_id.get());
            items
        };

        self.item_move_origin = Some(ItemMoveOrigin {
            snap_playhead: self.editor.read(cx).playhead(),
            item_id,
            items,
            pointer_x: f32::from(event.position.x),
            pointer_y: f32::from(event.position.y),
        });
        cx.stop_propagation();
    }

    pub(in crate::ui::panels::timeline) fn begin_item_interaction(
        &mut self,
        item_id: ItemId,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.cursor_layer = self.editor.read(cx).item_layer(item_id);
        if event.modifiers.shift {
            self.item_move_origin = None;
            self.editor.update(cx, |editor, cx| {
                editor.finish_history_group();
                if editor.toggle_item_selection(item_id) {
                    cx.notify();
                }
            });
            cx.stop_propagation();
            return;
        }
        self.prepare_item_move(item_id, event, cx);
    }

    pub(in crate::ui::panels::timeline) fn begin_item_resize(
        &mut self,
        item_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        self.cursor_layer = self.editor.read(cx).item_layer(item_id);
        self.editor.update(cx, |editor, cx| {
            editor.finish_history_group();
            let changed = if editor.is_item_selected(item_id) {
                false
            } else {
                editor.select(item_id)
            };
            if changed {
                cx.notify();
            }
        });
        cx.stop_propagation();
    }

    pub(in crate::ui::panels::timeline) fn move_item_from_pointer(
        &mut self,
        drag: &MoveTimelineItem,
        pointer_x: f32,
        pointer_y: f32,
        snap_disabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id() {
            return;
        }
        let Some(origin) = self
            .item_move_origin
            .as_ref()
            .filter(|origin| origin.item_id == drag.item_id)
        else {
            return;
        };

        let frame_rate = self.editor.read(cx).frame_rate();
        let (mut frame_delta, layer_delta) = origin.delta_at(
            pointer_x,
            pointer_y,
            self.viewport.pixels_per_second(),
            frame_rate,
            self.viewport.layer_height,
        );
        if !snap_disabled {
            frame_delta = self.snap_item_move_delta(origin, frame_delta, layer_delta, cx);
        }
        let origins = origin
            .items
            .iter()
            .map(|item| (item.item_id, item.start, item.source_layer))
            .collect::<Vec<_>>();
        self.editor.update_if_changed(cx, |editor| {
            editor.move_items_from(&origins, frame_delta, layer_delta)
        });
    }

    fn snap_item_move_delta(
        &self,
        origin: &ItemMoveOrigin,
        frame_delta: i64,
        layer_delta: i64,
        cx: &Context<Self>,
    ) -> i64 {
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let moving_times = origin
            .items
            .iter()
            .flat_map(|item| {
                let start = item
                    .start
                    .get()
                    .checked_add_signed(frame_delta)
                    .unwrap_or(0);
                let end = start.saturating_add(item.duration.get());
                [
                    frame_rate.frame_to_seconds(Frame::new(start)),
                    frame_rate.frame_to_seconds(Frame::new(end)),
                ]
            })
            .collect::<Vec<_>>();
        let offset = time_grid::snap_offset_seconds(
            &moving_times,
            editor,
            origin.snap_playhead,
            origin.items.iter().map(|item| item.item_id),
            self.viewport.pixels_per_second(),
        );
        let snapped = frame_delta.saturating_add(frame_rate.seconds_delta_to_frames(offset));
        origin.clamp_delta(snapped, layer_delta).0
    }

    pub(in crate::ui::panels::timeline) fn snap_frame(
        &self,
        frame: Frame,
        excluded_items: &[TimelineItem],
        cx: &Context<Self>,
    ) -> Frame {
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let start_seconds = frame_rate.frame_to_seconds(frame);
        let offset = time_grid::snap_offset_seconds(
            &[start_seconds],
            editor,
            editor.playhead(),
            excluded_items.iter().map(|item| item.id),
            self.viewport.pixels_per_second(),
        );
        frame_rate.seconds_to_frame((start_seconds + offset).max(0.))
    }

    pub(in crate::ui::panels::timeline) fn finish_item_move(&mut self, cx: &mut Context<Self>) {
        self.item_move_origin.take();
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }

    pub(in crate::ui::panels::timeline) fn remove_item(
        &mut self,
        item_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        self.stop_playback(cx);
        self.editor
            .update_if_changed(cx, |editor| editor.remove_item(item_id));
    }

    pub(crate) fn remove_selected_item(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.editor
            .update_if_changed(cx, TimelineEditor::remove_selected_item);
    }
}

fn clamp_move_delta(
    origins: impl Iterator<Item = (u64, u64, u64)>,
    frame_delta: i64,
    layer_delta: i64,
) -> (i64, i64) {
    let (min_start, max_end, min_layer, max_layer) = origins
        .map(|(start, duration, layer)| (start, start.saturating_add(duration), layer, layer))
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1), a.2.min(b.2), a.3.max(b.3)))
        .unwrap_or((0, 0, 0, 0));
    (
        frame_delta.clamp(
            negative_bound(min_start),
            positive_bound(u64::MAX.saturating_sub(max_end)),
        ),
        layer_delta.clamp(
            negative_bound(min_layer),
            positive_bound(u64::MAX.saturating_sub(max_layer)),
        ),
    )
}

fn negative_bound(value: u64) -> i64 {
    if value > i64::MAX as u64 {
        i64::MIN
    } else {
        -(value as i64)
    }
}

fn positive_bound(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}
