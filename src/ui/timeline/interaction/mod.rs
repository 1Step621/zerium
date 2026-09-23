mod context_menu;
mod drop;

use super::*;

impl Timeline {
    pub(super) fn begin_marquee_selection(
        &mut self,
        layer_index: usize,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        let additive = event.modifiers.shift;
        let baseline = if additive {
            self.editor.read(cx).selected_item_ids().collect()
        } else {
            HashSet::new()
        };
        let position = [f32::from(event.position.x), f32::from(event.position.y)];
        self.marquee_selection = Some(MarqueeSelection {
            layer_index,
            button: event.button,
            origin: position,
            current: position,
            baseline,
            active: false,
        });
        cx.notify();
        cx.stop_propagation();
    }

    pub(super) fn begin_primary_track_interaction(
        &mut self,
        layer_index: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor_layer = Some(LayerId::new(layer_index as u64));
        if is_marquee_pointer_down(event) {
            self.begin_marquee_selection(layer_index, event, cx);
        } else {
            self.begin_playhead_scrub(event, window, cx);
        }
    }

    pub(super) fn update_marquee_selection(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !is_marquee_pointer_drag(event) || self.marquee_selection.is_none() {
            return;
        }
        self.update_marquee_selection_at(event.position, cx);
    }

    pub(super) fn update_marquee_selection_at(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let was_active = self
            .marquee_selection
            .as_ref()
            .is_some_and(|marquee| marquee.active);
        self.marquee_selection
            .as_mut()
            .expect("marquee selection was checked above")
            .update(position);
        let is_active = self
            .marquee_selection
            .as_ref()
            .is_some_and(|marquee| marquee.active);
        if is_active {
            self.apply_marquee_selection(cx);
        }
        if is_active || was_active != is_active {
            cx.notify();
        }
    }

    pub(super) fn apply_marquee_selection(&mut self, cx: &mut Context<Self>) {
        let Some(marquee) = self
            .marquee_selection
            .as_ref()
            .filter(|marquee| marquee.active)
        else {
            return;
        };
        let selection_bounds = marquee.bounds();
        let list_bounds = layer_scroll_base(&self.layer_scroll).bounds();
        let track_bounds = Bounds {
            origin: point(
                list_bounds.origin.x + px(LAYER_HEADER_WIDTH),
                list_bounds.origin.y,
            ),
            size: size(
                (list_bounds.size.width - px(LAYER_HEADER_WIDTH)).max(px(0.)),
                list_bounds.size.height,
            ),
        };
        let layer_offset = f32::from(layer_scroll_base(&self.layer_scroll).offset().y);
        let mut selected = marquee.baseline.clone();
        let editor = self.editor.read(cx);
        for (item_id, layer, start, end) in editor.item_layouts() {
            let left = f32::from(track_bounds.origin.x)
                + self
                    .viewport
                    .x_at_seconds(editor.frame_rate().frame_to_seconds(start));
            let right = f32::from(track_bounds.origin.x)
                + self
                    .viewport
                    .x_at_seconds(editor.frame_rate().frame_to_seconds(end));
            let top = f32::from(list_bounds.origin.y)
                + layer.get() as f32 * self.viewport.layer_height
                + layer_offset;
            let item_bounds = Bounds {
                origin: point(px(left), px(top)),
                size: size(px((right - left).max(1.)), px(self.viewport.layer_height)),
            };
            if item_bounds.intersects(&track_bounds) && item_bounds.intersects(&selection_bounds) {
                selected.insert(item_id);
            }
        }
        self.editor
            .update_if_changed(cx, |editor| editor.select_items(selected));
    }

    pub(super) fn finish_marquee_selection(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut marquee) = self.marquee_selection.take() else {
            return;
        };
        marquee.update(event.position);
        if marquee.active {
            self.marquee_selection = Some(marquee);
            self.apply_marquee_selection(cx);
            self.marquee_selection = None;
        } else if marquee.button == MouseButton::Right {
            self.open_context_menu(
                marquee.layer_index,
                px(marquee.origin[0]),
                point(px(marquee.origin[0]), px(marquee.origin[1])),
                window,
                cx,
            );
        }
        cx.notify();
    }

    pub(super) fn marquee_overlay(&self, colors: ThemeColor) -> Option<Div> {
        let marquee = self
            .marquee_selection
            .as_ref()
            .filter(|marquee| marquee.active)?;
        let list_bounds = layer_scroll_base(&self.layer_scroll).bounds();
        let bounds = marquee.bounds();
        let left =
            f32::from(bounds.origin.x).max(f32::from(list_bounds.origin.x) + LAYER_HEADER_WIDTH);
        let right = f32::from(bounds.origin.x + bounds.size.width)
            .min(f32::from(list_bounds.origin.x + list_bounds.size.width));
        let top = f32::from(bounds.origin.y).max(f32::from(list_bounds.origin.y));
        let bottom = f32::from(bounds.origin.y + bounds.size.height)
            .min(f32::from(list_bounds.origin.y + list_bounds.size.height));
        if right <= left || bottom <= top {
            return None;
        }

        Some(
            div()
                .absolute()
                .left(px(left - f32::from(list_bounds.origin.x)))
                .top(px(
                    top - f32::from(list_bounds.origin.y) + PANE_HEADER_HEIGHT
                ))
                .w(px(right - left))
                .h(px(bottom - top))
                .border_1()
                .border_color(colors.primary)
                .bg(colors.primary.opacity(0.12)),
        )
    }

    pub(super) fn resize_item_from_pointer(
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
            pointer = self.snap_frame(pointer, None, &drag.origins, cx);
        }
        self.editor.update_if_changed(cx, |editor| {
            editor.resize_items(drag.origins.as_ref(), drag.anchor_id, drag.edge, pointer)
        });
    }

    pub(super) fn prepare_item_move(
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
                        start: item.start,
                        duration: item.duration,
                    })
                })
                .collect::<Vec<_>>();
            items.sort_unstable_by_key(|item| item.item_id.get());
            items
        };

        self.item_move_origin = Some(ItemMoveOrigin {
            item_id,
            items,
            pointer_x: f32::from(event.position.x),
            pointer_y: f32::from(event.position.y),
        });
        cx.stop_propagation();
    }

    pub(super) fn begin_item_interaction(
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

    pub(super) fn select_item(
        &mut self,
        item_id: ItemId,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.cursor_layer = self.editor.read(cx).item_layer(item_id);
        self.editor.update(cx, |editor, cx| {
            editor.finish_history_group();
            let changed = if event.modifiers.shift {
                editor.toggle_item_selection(item_id)
            } else if editor.is_item_selected(item_id) {
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

    pub(super) fn moved_items_delta(
        origin: &ItemMoveOrigin,
        pointer_x: f32,
        pointer_y: f32,
        pixels_per_second: f64,
        frame_rate: FrameRate,
        layer_height: f32,
    ) -> (i64, i64) {
        let delta_seconds = (pointer_x - origin.pointer_x) as f64 / pixels_per_second;
        let delta_frames = frame_rate.seconds_delta_to_frames(delta_seconds);
        let layer_delta = ((pointer_y - origin.pointer_y) / layer_height).round() as i64;
        Self::clamp_item_move_delta(origin, delta_frames, layer_delta)
    }

    pub(super) fn clamp_item_move_delta(
        origin: &ItemMoveOrigin,
        frame_delta: i64,
        layer_delta: i64,
    ) -> (i64, i64) {
        let origins = origin
            .items
            .iter()
            .map(|item| model::MoveOrigin {
                start: item.start.get(),
                duration: item.duration.get(),
                layer: item.source_layer.get(),
            })
            .collect::<Vec<_>>();
        model::clamp_move_delta(&origins, frame_delta, layer_delta)
    }

    pub(super) fn move_item_from_pointer(
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
        let (mut frame_delta, layer_delta) = Self::moved_items_delta(
            origin,
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

    pub(super) fn snap_item_move_delta(
        &self,
        origin: &ItemMoveOrigin,
        frame_delta: i64,
        layer_delta: i64,
        cx: &Context<Self>,
    ) -> i64 {
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let moving_ids = origin
            .items
            .iter()
            .map(|item| item.item_id)
            .collect::<HashSet<_>>();
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
        let mut targets = vec![editor.playhead_seconds()];
        for (_, start, end) in editor
            .item_time_ranges()
            .filter(|(item_id, _, _)| !moving_ids.contains(item_id))
        {
            targets.push(frame_rate.frame_to_seconds(start));
            targets.push(frame_rate.frame_to_seconds(end));
        }
        let offset = model::snap_offset_seconds(
            &moving_times,
            &targets,
            self.viewport.ruler_step(),
            self.viewport.pixels_per_second(),
        );
        let snapped = frame_delta.saturating_add(frame_rate.seconds_delta_to_frames(offset));
        Self::clamp_item_move_delta(origin, snapped, layer_delta).0
    }

    pub(super) fn snap_frame(
        &self,
        frame: Frame,
        moving_duration: Option<FrameDuration>,
        excluded_items: &[TimelineItem],
        cx: &Context<Self>,
    ) -> Frame {
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let start_seconds = frame_rate.frame_to_seconds(frame);
        let mut moving_times = vec![start_seconds];
        if let Some(duration) = moving_duration {
            moving_times.push(
                frame_rate.frame_to_seconds(Frame::new(frame.get().saturating_add(duration.get()))),
            );
        }

        let mut targets = vec![editor.playhead_seconds()];
        for (_, start, end) in editor
            .item_time_ranges()
            .filter(|(item_id, _, _)| !excluded_items.iter().any(|item| item.id == *item_id))
        {
            targets.push(frame_rate.frame_to_seconds(start));
            targets.push(frame_rate.frame_to_seconds(end));
        }
        let offset = model::snap_offset_seconds(
            &moving_times,
            &targets,
            self.viewport.ruler_step(),
            self.viewport.pixels_per_second(),
        );
        frame_rate.seconds_to_frame((start_seconds + offset).max(0.))
    }

    pub(super) fn move_animation_stop_from_pointer(
        &mut self,
        drag: &MoveAnimationStop,
        pointer_x: f32,
        snap_disabled: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id()
            || self.animation_selection.read(cx).target() != Some(&drag.target)
        {
            return;
        }

        let mut frame = self.pointer_frame(pointer_x, window, cx);
        let progress = {
            let editor = self.editor.read(cx);
            let Some(item) = editor
                .selected_item()
                .filter(|item| item.id == drag.target.item_id)
            else {
                return;
            };
            let Some(track) = item.animation_track(
                drag.target.effect_id,
                &drag.target.property_id,
                drag.target.element_id,
                drag.target.scalar_index,
            ) else {
                return;
            };
            let Some(previous) = drag
                .stop
                .checked_sub(1)
                .and_then(|index| track.stops().get(index))
            else {
                return;
            };
            let Some(next) = track.stops().get(drag.stop + 1) else {
                return;
            };
            let frame_at = |position| {
                Frame::new(item.animation_timeline_frame(position).round().max(0.) as u64)
            };
            let minimum = Frame::new(frame_at(previous.position()).get().saturating_add(1));
            let maximum = Frame::new(frame_at(next.position()).get().saturating_sub(1));
            if minimum > maximum {
                return;
            }
            frame = frame.clamp(minimum, maximum);
            if !snap_disabled {
                let playhead_x = LAYER_HEADER_WIDTH
                    + self
                        .viewport
                        .x_at_seconds(editor.frame_rate().frame_to_seconds(drag.snap_frame));
                if (minimum..=maximum).contains(&drag.snap_frame)
                    && (pointer_x - playhead_x).abs() <= ANIMATION_STOP_SNAP_DISTANCE
                {
                    frame = drag.snap_frame;
                }
            }
            item.animation_progress_at_time(TimelineTime::from_frame(frame))
        };

        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor.move_selected_animation_stop(
                drag.target.effect_id,
                drag.target.property_id.clone(),
                drag.target.element_id,
                drag.target.scalar_index,
                drag.stop,
                progress,
            );
            if changed {
                cx.notify();
            }
            changed
        });
        if changed && let Some(segment) = drag.follow_focus {
            self.animation_selection.read(cx).focus_segment(segment);
            self.transport
                .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        }
    }

    pub(super) fn finish_item_move(&mut self, cx: &mut Context<Self>) {
        self.item_move_origin.take();
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }
}
