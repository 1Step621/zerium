//! Scene expansion preserves independent placement, animation, and media clocks.

use crate::timeline::{Frame, FrameDuration, FrameRate, TimelineItem, TimelineTime};

pub(super) struct SceneExpansionPlan {
    point: Frame,
    extra: u64,
    frame_rate: FrameRate,
}

impl SceneExpansionPlan {
    pub(super) fn new(
        source_duration: FrameDuration,
        instance_duration: FrameDuration,
        point: Frame,
        frame_rate: FrameRate,
    ) -> Self {
        Self {
            frame_rate,
            point: Frame::new(point.get().min(source_duration.get() - 1)),
            extra: instance_duration
                .get()
                .saturating_sub(source_duration.get()),
        }
    }

    /// Inverse lookup for occupied source intervals only. This is deliberately
    /// not the clock used to sample animation or media inside an expanded item.
    pub(super) fn source_time(&self, time: TimelineTime) -> TimelineTime {
        let frame = time.frames();
        TimelineTime::from_frames(if frame > self.point.get() as f64 {
            (frame - self.extra as f64).max(self.point.get() as f64)
        } else {
            frame
        })
    }

    pub(super) fn apply(&self, item: &mut TimelineItem) {
        item.expand_at(self.point, self.extra, self.frame_rate);
    }
}
