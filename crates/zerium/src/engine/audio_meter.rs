use std::sync::Arc;

use crate::engine::media::{
    AudioCalculationPolicy, AudioFormat, AudioTimelineGraph, MediaReaderRegistry, sample_boundary,
};
use zerium_core::timeline::{AudioTimelineSnapshot, Frame, TimelineEditor};

const SAMPLE_FRAMES: usize = 2_048;
const FORMAT: AudioFormat = AudioFormat {
    sample_rate: 48_000,
    channels: 2,
};

pub(crate) struct AudioLevelSampler {
    media_readers: Arc<MediaReaderRegistry>,
    timeline: Option<AudioTimelineSnapshot>,
    graph: Option<AudioTimelineGraph>,
    cached: Option<(Frame, [f32; 2])>,
}

impl AudioLevelSampler {
    pub(crate) fn new(media_readers: Arc<MediaReaderRegistry>) -> Self {
        Self {
            media_readers,
            timeline: None,
            graph: None,
            cached: None,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.timeline = None;
        self.graph = None;
        self.cached = None;
    }

    pub(crate) fn levels_at(&mut self, editor: &TimelineEditor) -> [f32; 2] {
        let timeline = editor.audio_snapshot();
        let frame = editor.playhead();
        let frame_rate = timeline.frame_rate();
        let unchanged = self
            .timeline
            .as_ref()
            .is_some_and(|previous| previous.same_content(&timeline));
        if unchanged
            && let Some((cached_frame, levels)) = self.cached
            && cached_frame == frame
        {
            return levels;
        }
        if !unchanged {
            if let Some(graph) = &mut self.graph {
                if graph
                    .update_timeline(timeline.clone(), &self.media_readers)
                    .is_err()
                {
                    self.graph = None;
                }
            } else {
                self.graph = AudioTimelineGraph::new(
                    timeline.clone(),
                    FORMAT,
                    &self.media_readers,
                    AudioCalculationPolicy::Fallback,
                )
                .ok();
            }
            self.timeline = Some(timeline);
            self.cached = None;
        }
        let levels = self
            .graph
            .as_mut()
            .and_then(|graph| {
                graph
                    .render(
                        sample_boundary(frame.get(), frame_rate, FORMAT.sample_rate),
                        SAMPLE_FRAMES,
                    )
                    .ok()
            })
            .map_or([0.; 2], |samples| {
                peak_levels(&samples, usize::from(FORMAT.channels))
            });

        self.cached = Some((frame, levels));
        levels
    }
}

fn peak_levels(samples: &[f32], channels: usize) -> [f32; 2] {
    let channels = channels.max(1);
    let mut peak = [0_f32; 2];
    for (index, sample) in samples.iter().enumerate() {
        let channel = index % channels;
        if channel < peak.len() {
            peak[channel] = peak[channel].max(sample.abs());
        }
    }
    if channels == 1 {
        peak[1] = peak[0];
    }
    peak
}
