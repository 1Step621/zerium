use crate::domain::timeline::{Frame, FrameRate};

const TARGET_RULER_TICK_SPACING: f64 = 90.;
const TARGET_FRAME_GRID_SPACING: f64 = 10.;

pub(crate) fn format_timestamp(seconds: f64) -> String {
    let total_seconds = seconds.round().clamp(0., u64::MAX as f64) as u64;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

pub(crate) fn ruler_step(pixels_per_second: f64) -> f64 {
    const STEPS: [f64; 11] = [1., 2., 5., 10., 20., 30., 60., 120., 300., 600., 1_800.];

    let desired_step = TARGET_RULER_TICK_SPACING / pixels_per_second.max(f64::EPSILON);
    STEPS
        .into_iter()
        .find(|step| *step >= desired_step)
        .unwrap_or(3_600.)
}

pub(crate) fn frame_grid_step(pixels_per_second: f64, frame_rate: FrameRate) -> u64 {
    const STEPS: [u64; 13] = [1, 2, 3, 5, 10, 15, 30, 60, 120, 300, 600, 1_800, 3_600];

    let pixels_per_frame = pixels_per_second * frame_rate.frame_duration().as_secs_f64();
    STEPS
        .into_iter()
        .find(|frames| *frames as f64 * pixels_per_frame >= TARGET_FRAME_GRID_SPACING)
        .unwrap_or(3_600)
}

pub(crate) fn visible_seconds(start: f64, end: f64, step: f64) -> Vec<f64> {
    let first = (start / step).floor() * step;
    let count = ((end - first) / step).ceil().max(0.) as usize + 1;
    (0..count)
        .map(|index| first + index as f64 * step)
        .collect()
}

pub(crate) fn visible_frames(
    start: f64,
    end: f64,
    frame_rate: FrameRate,
    step_frames: u64,
) -> Vec<Frame> {
    let first_visible = frame_rate.seconds_to_frame(start).get();
    let last_visible = frame_rate.seconds_to_frame(end).get();
    let first = first_visible.saturating_sub(step_frames) / step_frames * step_frames;
    let count = last_visible.saturating_sub(first) / step_frames + 2;

    (0..count)
        .map(|index| Frame::new(first.saturating_add(index.saturating_mul(step_frames))))
        .collect()
}
