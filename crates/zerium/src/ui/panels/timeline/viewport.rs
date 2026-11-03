use crate::ui::panels::timeline::LAYER_HEIGHT;
use gpui::{Bounds, Context, Pixels, Point, Window, point, px};

use zerium_core::timeline::{Frame, FrameRate, LayerId};

use super::{LAYER_HEADER_WIDTH, Timeline, TimelineRow, layer_scroll_base};

const DEFAULT_VISIBLE_SECONDS: f64 = 10.;
const MAX_PIXELS_PER_FRAME: f64 = 20.;

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineViewport {
    horizontal_offset_seconds: f64,
    pixels_per_second: f64,
    needs_initial_fit: bool,
    followed_playhead: Option<Frame>,
}

impl Default for TimelineViewport {
    fn default() -> Self {
        Self {
            horizontal_offset_seconds: 0.,
            pixels_per_second: 1.,
            needs_initial_fit: true,
            followed_playhead: None,
        }
    }
}

impl TimelineViewport {
    pub(super) fn fit_initial(&mut self, width: f32, duration: f64, playhead: Frame) -> bool {
        if !self.needs_initial_fit || width <= 0. {
            return false;
        }
        let seconds = if duration > 0. {
            duration
        } else {
            DEFAULT_VISIBLE_SECONDS
        };
        self.pixels_per_second = ((width - 32.).max(1.) as f64 / seconds).max(f64::EPSILON);
        self.horizontal_offset_seconds = 0.;
        self.followed_playhead = Some(playhead);
        self.needs_initial_fit = false;
        true
    }

    pub(super) fn pixels_per_second(self) -> f64 {
        self.pixels_per_second
    }

    pub(super) fn visible_time_range(self, viewport_width: f32) -> (f64, f64) {
        let start = self.horizontal_offset_seconds;
        let end = start + viewport_width.max(1.) as f64 / self.pixels_per_second();
        (start, end)
    }

    pub(super) fn scroll_horizontal(&mut self, delta_pixels: f32) -> bool {
        let offset = (self.horizontal_offset_seconds
            - delta_pixels as f64 / self.pixels_per_second())
        .max(0.);
        if offset == self.horizontal_offset_seconds {
            return false;
        }
        self.horizontal_offset_seconds = offset;
        true
    }

    pub(super) fn zoom_horizontal(
        &mut self,
        factor: f32,
        cursor_x: f32,
        frame_rate: FrameRate,
    ) -> bool {
        let old_pixels_per_second = self.pixels_per_second();
        let scale = (old_pixels_per_second * factor as f64).clamp(
            f64::EPSILON,
            (frame_rate.frames_per_second() * MAX_PIXELS_PER_FRAME).max(old_pixels_per_second),
        );
        if scale == old_pixels_per_second {
            return false;
        }

        let anchor_seconds =
            self.horizontal_offset_seconds + cursor_x as f64 / old_pixels_per_second;
        self.pixels_per_second = scale;
        self.horizontal_offset_seconds =
            (anchor_seconds - cursor_x as f64 / self.pixels_per_second()).max(0.);
        true
    }

    pub(super) fn frame_at_x(
        self,
        position_x: f32,
        header_width: f32,
        viewport_width: f32,
        frame_rate: FrameRate,
    ) -> Frame {
        let viewport_x = (position_x - header_width).clamp(0., viewport_width);
        let seconds = self.horizontal_offset_seconds + viewport_x as f64 / self.pixels_per_second();
        frame_rate.seconds_to_frame(seconds)
    }

    pub(super) fn x_at_seconds(self, seconds: f64) -> f32 {
        ((seconds - self.horizontal_offset_seconds) * self.pixels_per_second()) as f32
    }

    pub(super) fn scrub_playhead(&mut self, frame: Frame) {
        // Following a pointer seek changes the time under that same pointer on its next move.
        self.followed_playhead = Some(frame);
    }

    pub(super) fn follow_playhead(
        &mut self,
        frame: Frame,
        viewport_width: f32,
        frame_rate: FrameRate,
    ) {
        if self.followed_playhead == Some(frame) {
            return;
        }
        self.followed_playhead = Some(frame);
        let playhead_seconds = frame_rate.frame_to_seconds(frame);
        let playhead_x = self.x_at_seconds(playhead_seconds);
        let margin = 32.;
        if playhead_x >= margin && playhead_x <= viewport_width - margin {
            return;
        }

        let visible_seconds = viewport_width.max(1.) as f64 / self.pixels_per_second();
        self.horizontal_offset_seconds = (playhead_seconds - visible_seconds / 2.).max(0.);
    }
}

impl Timeline {
    pub(super) fn fit_initial_viewport(&mut self, width: f32, cx: &mut Context<Self>) {
        if !self.viewport.needs_initial_fit {
            return;
        }
        let editor = self.editor.read(cx);
        let duration = if editor.active_scene().is_empty() {
            0.
        } else {
            editor
                .frame_rate()
                .frame_to_seconds(editor.end_frame_exclusive())
        };
        if self
            .viewport
            .fit_initial(width - LAYER_HEADER_WIDTH, duration, editor.playhead())
        {
            cx.notify();
        }
    }

    pub(super) fn row_at_position(&self, position: Point<Pixels>) -> Option<TimelineRow<'_>> {
        let scroll = layer_scroll_base(&self.layer_scroll);
        if !scroll.bounds().contains(&position) {
            return None;
        }
        let content_y = f32::from(position.y - scroll.bounds().origin.y - scroll.offset().y);
        Some(
            self.rows
                .row((content_y / LAYER_HEIGHT).floor().max(0.) as usize),
        )
    }

    pub(super) fn layer_at_content_y(&self, y: f32) -> LayerId {
        self.rows
            .row((y / LAYER_HEIGHT).floor().max(0.) as usize)
            .layer()
    }

    pub(super) fn track_bounds(&self) -> Bounds<Pixels> {
        let bounds = layer_scroll_base(&self.layer_scroll).bounds();
        Bounds::from_corners(
            bounds.origin + point(px(LAYER_HEADER_WIDTH), px(0.)),
            bounds.bottom_right(),
        )
    }

    pub(super) fn row_count(&self, window: &Window, cx: &Context<Self>) -> usize {
        let occupied_end = self
            .editor
            .read(cx)
            .item_layouts()
            .map(|(_, layer, _, _)| layer.get())
            .max()
            .map_or(0, |layer| {
                self.rows.layer_row(LayerId::new(layer.saturating_add(1)))
            });
        let scroll = layer_scroll_base(&self.layer_scroll);
        let measured_height = f32::from(scroll.bounds().size.height);
        let viewport_height = if measured_height > 0. {
            measured_height
        } else {
            f32::from(window.viewport_size().height)
        };
        let scroll_top = (-f32::from(scroll.offset().y)).max(0.);
        let viewport_end = ((scroll_top + viewport_height) / LAYER_HEIGHT).ceil() as usize;
        occupied_end.max(viewport_end).saturating_add(1)
    }

    pub(super) fn reset_viewport(&mut self) {
        self.viewport = TimelineViewport::default();
        layer_scroll_base(&self.layer_scroll).set_offset(point(px(0.), px(0.)));
    }

    pub(super) fn track_viewport_width(&self, window: &Window) -> f32 {
        let measured_width = layer_scroll_base(&self.layer_scroll).bounds().size.width;
        let width = if measured_width > px(0.) {
            measured_width
        } else {
            window.viewport_size().width
        };
        (f32::from(width) - LAYER_HEADER_WIDTH).max(1.)
    }
}
