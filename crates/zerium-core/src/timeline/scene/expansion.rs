//! Scene expansion preserves independent placement, animation, and media clocks.

use crate::property::PropertyValue;

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
            point: Frame::new(point.get().min(source_duration.get() - 1)),
            extra: instance_duration
                .get()
                .saturating_sub(source_duration.get()),
            frame_rate,
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
        if self.extra == 0 {
            return;
        }
        if !item.contains(self.point) {
            if item.start > self.point {
                item.start = Frame::new(item.start.get().saturating_add(self.extra));
            }
            return;
        }
        let duration =
            FrameDuration::new_saturating(item.duration.get().saturating_add(self.extra));
        let local_frame = Frame::new(self.point.get().saturating_sub(item.start.get()));
        for animations in std::iter::once(&mut item.animations)
            .chain(item.effects.iter_mut().map(|effect| &mut effect.animations))
        {
            animations.extend_at(local_frame, duration);
        }
        // Extend the declared source interval, preserving playback speed
        // and each input's independently referenced loop/EOF settings.
        if let Some(mapping) = item.timeline_mapping() {
            let mapping = mapping.with_timeline_duration(duration, self.frame_rate);
            let property = item
                .schema()
                .and_then(|schema| schema.property(schema.timeline()?.source_duration))
                .expect("source duration is declared");
            let value = property
                .constrained_value(&PropertyValue::F32(mapping.source_span() as f32))
                .expect("finite source duration preserves its property type");
            let span = value.as_f32().expect("source duration is f32");
            item.store_timeline_mapping(mapping.with_span(f64::from(span)))
                .expect("constrained source duration preserves the source clock");
        }
        item.duration = duration;
    }
}
