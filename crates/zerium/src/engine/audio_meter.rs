use std::sync::Arc;

use crate::{
    domain::timeline::{Frame, FrameRate, TimelineItem},
    engine::media::{
        AudioFormat, AudioGainEvaluation, AudioTimelineGraph, MediaReaderRegistry, sample_boundary,
    },
};

const SAMPLE_FRAMES: usize = 2_048;
const FORMAT: AudioFormat = AudioFormat {
    sample_rate: 48_000,
    channels: 2,
};

pub(crate) struct AudioLevelSampler {
    media_readers: Arc<MediaReaderRegistry>,
    items: Vec<TimelineItem>,
    frame_rate: Option<FrameRate>,
    graph: Option<AudioTimelineGraph>,
    cached: Option<(Frame, [f32; 2])>,
}

impl AudioLevelSampler {
    pub(crate) fn new(media_readers: Arc<MediaReaderRegistry>) -> Self {
        Self {
            media_readers,
            items: Vec::new(),
            frame_rate: None,
            graph: None,
            cached: None,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.items.clear();
        self.frame_rate = None;
        self.graph = None;
        self.cached = None;
    }

    pub(crate) fn levels_at(
        &mut self,
        items: Vec<TimelineItem>,
        frame: Frame,
        frame_rate: FrameRate,
    ) -> [f32; 2] {
        if self.frame_rate != Some(frame_rate) || self.items != items {
            self.graph = AudioTimelineGraph::new(
                items.clone(),
                frame_rate,
                FORMAT,
                &self.media_readers,
                AudioGainEvaluation::TimelineAnimation,
            )
            .ok();
            self.items = items;
            self.frame_rate = Some(frame_rate);
            self.cached = None;
        }
        if let Some((cached_frame, levels)) = self.cached
            && cached_frame == frame
        {
            return levels;
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
