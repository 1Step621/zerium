use crate::ui::timeline::track_viewport_width;
use gpui::{Context, MouseDownEvent, MouseMoveEvent, ScrollWheelEvent, Window, point, px};

use zerium_core::timeline::Frame;

use crate::ui::transport::ScrubSource;

use crate::ui::timeline::{LAYER_HEADER_WIDTH, Timeline, layer_scroll_base};

const ZOOM_STEP: f32 = 1.05;

impl Timeline {
    pub(in crate::ui::timeline) fn zoom_horizontal(
        &mut self,
        event: &ScrollWheelEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(factor) = zoom_factor(event, window) else {
            return;
        };

        let viewport_width = track_viewport_width(window);
        let cursor_x = (f32::from(event.position.x) - LAYER_HEADER_WIDTH).clamp(0., viewport_width);
        if self
            .viewport
            .zoom_horizontal(factor, cursor_x, self.editor.read(cx).frame_rate())
        {
            cx.notify();
        }
    }

    pub(in crate::ui::timeline) fn zoom_vertical(
        &mut self,
        event: &ScrollWheelEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(factor) = zoom_factor(event, window) else {
            return;
        };

        let Some(scroll_ratio) = self.viewport.zoom_vertical(factor) else {
            return;
        };

        let old_offset = layer_scroll_base(&self.layer_scroll).offset();
        layer_scroll_base(&self.layer_scroll)
            .set_offset(point(old_offset.x, old_offset.y * scroll_ratio));
        cx.notify();
    }

    pub(in crate::ui::timeline) fn scroll_layers(
        &self,
        event: &ScrollWheelEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(window.line_height());
        let scroll = layer_scroll_base(&self.layer_scroll);
        let old_offset = scroll.offset();
        let new_y = (old_offset.y + delta.y).min(px(0.));
        if new_y != old_offset.y {
            scroll.set_offset(point(old_offset.x, new_y));
            cx.notify();
        }
    }

    pub(in crate::ui::timeline) fn on_track_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.modifiers.control {
            if event.modifiers.shift {
                self.zoom_vertical(event, window, cx);
            } else {
                self.zoom_horizontal(event, window, cx);
            }
        } else if event.modifiers.alt {
            self.scroll_layers(event, window, cx);
        } else {
            let delta = dominant_scroll_delta(event, window);
            if self.viewport.scroll_horizontal(delta) {
                cx.notify();
            }
        }

        window.prevent_default();
        cx.stop_propagation();
    }

    pub(in crate::ui::timeline) fn on_label_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.modifiers.control {
            if event.modifiers.shift {
                self.zoom_vertical(event, window, cx);
            } else {
                self.zoom_horizontal(event, window, cx);
            }
        } else {
            self.scroll_layers(event, window, cx);
        }

        window.prevent_default();
        cx.stop_propagation();
    }

    pub(in crate::ui::timeline) fn pointer_frame(
        &self,
        position_x: f32,
        window: &Window,
        cx: &Context<Self>,
    ) -> Frame {
        let viewport_width = track_viewport_width(window);
        self.viewport.frame_at_x(
            position_x,
            LAYER_HEADER_WIDTH,
            viewport_width,
            self.editor.read(cx).frame_rate(),
        )
    }

    fn seek_to_x(&mut self, position_x: f32, window: &mut Window, cx: &mut Context<Self>) {
        let frame = self.pointer_frame(position_x, window, cx);
        self.transport
            .update(cx, |transport, cx| transport.set_playhead(frame, cx));
    }

    pub(in crate::ui::timeline) fn begin_playhead_scrub(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scrubbing_playhead = true;
        self.transport.update(cx, |transport, cx| {
            transport.begin_scrub(ScrubSource::Timeline, cx);
        });
        self.seek_to_x(f32::from(event.position.x), window, cx);
    }

    pub(in crate::ui::timeline) fn update_playhead_scrub(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.scrubbing_playhead {
            return;
        }
        if !event.dragging() {
            self.finish_playhead_scrub(cx);
            return;
        }

        self.seek_to_x(f32::from(event.position.x), window, cx);
    }

    pub(in crate::ui::timeline) fn finish_playhead_scrub(&mut self, cx: &mut Context<Self>) {
        if !self.scrubbing_playhead {
            return;
        }
        self.scrubbing_playhead = false;
        self.transport.update(cx, |transport, cx| {
            transport.end_scrub(ScrubSource::Timeline, cx);
        });
    }
}

fn dominant_scroll_delta(event: &ScrollWheelEvent, window: &Window) -> f32 {
    let delta = event.delta.pixel_delta(window.line_height());
    let x = f32::from(delta.x);
    let y = f32::from(delta.y);

    if x.abs() > y.abs() { x } else { y }
}

fn zoom_factor(event: &ScrollWheelEvent, window: &Window) -> Option<f32> {
    let delta = dominant_scroll_delta(event, window);
    if delta > 0. {
        Some(ZOOM_STEP)
    } else if delta < 0. {
        Some(1. / ZOOM_STEP)
    } else {
        None
    }
}
