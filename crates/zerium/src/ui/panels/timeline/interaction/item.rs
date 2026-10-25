use std::{cell::Cell, rc::Rc};

use gpui::{Context, Empty, EntityId, Modifiers, MouseDownEvent, Render, Window, prelude::*};

use zerium_core::timeline::{
    Frame, FrameRate, ItemId, ItemMoveEdit, ResizeEdge, ResizeMode, TimelineEditor, TimelineItem,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveAxis {
    Time,
    Layer,
}

pub(in crate::ui::panels::timeline) struct ItemMoveDrag {
    snap_playhead: Frame,
    item_id: ItemId,
    edit: ItemMoveEdit,
    pointer_x: f32,
    pointer_y: f32,
}

impl ItemMoveDrag {
    fn delta_at(
        &self,
        pointer_x: f32,
        pointer_y: f32,
        pixels_per_second: f64,
        frame_rate: FrameRate,
        layer_height: f32,
        constrain_axis: bool,
    ) -> (i64, i64, Option<MoveAxis>) {
        let delta_x = pointer_x - self.pointer_x;
        let delta_y = pointer_y - self.pointer_y;
        let axis = constrain_axis.then(|| {
            if delta_x.abs() >= delta_y.abs() {
                MoveAxis::Time
            } else {
                MoveAxis::Layer
            }
        });
        let delta_frames = if axis == Some(MoveAxis::Layer) {
            0
        } else {
            frame_rate.seconds_delta_to_frames(delta_x as f64 / pixels_per_second)
        };
        let layer_delta = if axis == Some(MoveAxis::Time) {
            0
        } else {
            (delta_y / layer_height).round() as i64
        };
        let (frame_delta, layer_delta) =
            self.edit.clamp_delta(delta_frames, layer_delta, frame_rate);
        (frame_delta, layer_delta, axis)
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

    pub(in crate::ui::panels::timeline) fn begin_item_interaction(
        &mut self,
        item_id: ItemId,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window, cx);
        self.cursor_layer = self.editor.read(cx).item_layer(item_id);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        if !event.modifiers.shift && !self.editor.read(cx).is_item_selected(item_id) {
            self.editor
                .update_if_changed(cx, |editor| editor.select(item_id));
        }
        let Some(edit) = self.editor.read(cx).begin_item_move(item_id) else {
            return;
        };

        self.item_drag = Some(ItemMoveDrag {
            snap_playhead: self.editor.read(cx).playhead(),
            item_id,
            edit,
            pointer_x: f32::from(event.position.x),
            pointer_y: f32::from(event.position.y),
        });
        cx.stop_propagation();
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

    pub(in crate::ui::panels::timeline) fn refresh_item_move(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !cx.has_active_drag() || window.pressed_mouse_button() != Some(gpui::MouseButton::Left) {
            return;
        }
        let pointer = window.mouse_position();
        self.update_item_move(
            f32::from(pointer.x),
            f32::from(pointer.y),
            window.modifiers(),
            cx,
        );
    }

    pub(in crate::ui::panels::timeline) fn move_item_from_pointer(
        &mut self,
        drag: &MoveTimelineItem,
        pointer_x: f32,
        pointer_y: f32,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id()
            || self
                .item_drag
                .as_ref()
                .is_none_or(|state| state.item_id != drag.item_id)
        {
            return;
        }
        self.update_item_move(pointer_x, pointer_y, modifiers, cx);
    }

    fn update_item_move(
        &mut self,
        pointer_x: f32,
        pointer_y: f32,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(mut state) = self.item_drag.take() else {
            return;
        };
        if !state.edit.is_current(self.editor.read(cx)) {
            return;
        }

        let frame_rate = self.editor.read(cx).frame_rate();
        let (mut frame_delta, layer_delta, axis) = state.delta_at(
            pointer_x,
            pointer_y,
            self.viewport.pixels_per_second(),
            frame_rate,
            self.viewport.layer_height,
            modifiers.shift,
        );
        if !modifiers.alt && axis != Some(MoveAxis::Layer) {
            frame_delta =
                self.snap_item_move_delta(&state, frame_delta, layer_delta, modifiers.control, cx);
        }
        self.editor.update_if_changed(cx, |editor| {
            editor.move_items(&mut state.edit, frame_delta, layer_delta, modifiers.control)
        });
        self.item_drag = Some(state);
    }

    fn snap_item_move_delta(
        &self,
        state: &ItemMoveDrag,
        frame_delta: i64,
        layer_delta: i64,
        duplicate: bool,
        cx: &Context<Self>,
    ) -> i64 {
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let moving_times = state
            .edit
            .origins()
            .iter()
            .flat_map(|(_, item)| {
                let start = item
                    .start()
                    .get()
                    .checked_add_signed(frame_delta)
                    .unwrap_or(0);
                let end = start.saturating_add(item.duration(frame_rate).get());
                [
                    frame_rate.frame_to_seconds(Frame::new(start)),
                    frame_rate.frame_to_seconds(Frame::new(end)),
                ]
            })
            .collect::<Vec<_>>();
        let offset = time_grid::snap_offset_seconds(
            &moving_times,
            editor,
            state.snap_playhead,
            state.edit.snap_excluded_items(duplicate),
            self.viewport.pixels_per_second(),
        );
        let snapped = frame_delta.saturating_add(frame_rate.seconds_delta_to_frames(offset));
        state.edit.clamp_delta(snapped, layer_delta, frame_rate).0
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
        self.item_drag.take();
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
