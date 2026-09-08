use crate::domain::timeline::{Frame, FrameRate};

use super::super::time_grid;

pub(super) const INITIAL_LAYER_HEIGHT: f32 = 30.;
const MIN_LAYER_HEIGHT: f32 = 28.;
const MAX_LAYER_HEIGHT: f32 = 96.;
const BASE_PIXELS_PER_SECOND: f64 = 1_400. / 60.;
const MIN_HORIZONTAL_ZOOM: f64 = 0.25;
const MAX_HORIZONTAL_ZOOM: f64 = 16.;

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineViewport {
    pub layer_height: f32,
    horizontal_offset_seconds: f64,
    horizontal_zoom: f64,
}

impl Default for TimelineViewport {
    fn default() -> Self {
        Self {
            layer_height: INITIAL_LAYER_HEIGHT,
            horizontal_offset_seconds: 0.,
            horizontal_zoom: 1.,
        }
    }
}

impl TimelineViewport {
    pub(super) fn pixels_per_second(self) -> f64 {
        BASE_PIXELS_PER_SECOND * self.horizontal_zoom
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

    pub(super) fn zoom_horizontal(&mut self, factor: f32, cursor_x: f32) -> bool {
        let old_pixels_per_second = self.pixels_per_second();
        let zoom =
            (self.horizontal_zoom * factor as f64).clamp(MIN_HORIZONTAL_ZOOM, MAX_HORIZONTAL_ZOOM);
        if zoom == self.horizontal_zoom {
            return false;
        }

        let anchor_seconds =
            self.horizontal_offset_seconds + cursor_x as f64 / old_pixels_per_second;
        self.horizontal_zoom = zoom;
        self.horizontal_offset_seconds =
            (anchor_seconds - cursor_x as f64 / self.pixels_per_second()).max(0.);
        true
    }

    pub(super) fn zoom_vertical(&mut self, factor: f32) -> Option<f32> {
        let old_height = self.layer_height;
        let height = (old_height * factor).clamp(MIN_LAYER_HEIGHT, MAX_LAYER_HEIGHT);
        if height == old_height {
            return None;
        }
        self.layer_height = height;
        Some(height / old_height)
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

    pub(super) fn ruler_step(self) -> f64 {
        time_grid::ruler_step(self.pixels_per_second())
    }

    pub(super) fn frame_grid_step(self, frame_rate: FrameRate) -> u64 {
        time_grid::frame_grid_step(self.pixels_per_second(), frame_rate)
    }
}
