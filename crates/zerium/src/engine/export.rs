use std::{
    collections::HashMap,
    fmt,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
    time::Duration,
};

use thiserror::Error;

use crate::engine::{
    frame::RgbaFrame,
    media::{
        AtomicFileTransaction, AudioFormat, AudioGainEvaluation, AudioTimelineGraph,
        FfmpegFileEncoder, MediaInputId, MediaReaderRegistry, VideoColorSpec, VideoDecodeSize,
        VideoEncoderSettings, VideoOutputSpec, VisualDecoderSession, item_readings, refresh_files,
        sample_boundary,
    },
    rendering::{
        ExportFramePipeline, FrameRenderer, MediaFrameRequest, RenderQuality, RenderScene,
        RenderSize, TextFrameCache,
    },
};
use zerium_core::{
    media::{MediaAsset, VideoFrameRate},
    timeline::{Frame, FrameRate, TimelineSnapshot, TimelineTime, TimelineView},
};

#[derive(Clone, Debug)]
pub(crate) struct ExportSettings {
    pub output: PathBuf,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum ExportError {
    #[error("{0}")]
    InvalidTimeline(String),
    #[error(transparent)]
    Media(#[from] crate::engine::media::MediaError),
    #[error(transparent)]
    Render(#[from] crate::engine::rendering::RenderError),
    #[error("{0}")]
    Encoding(String),
}

impl ExportError {
    pub(crate) fn encoding(message: impl fmt::Display) -> Self {
        Self::Encoding(message.to_string())
    }
}

struct ExportDecoder {
    asset: MediaAsset,
    decoder: VisualDecoderSession,
}

const EXPORT_AUDIO_FORMAT: AudioFormat = AudioFormat {
    sample_rate: 48_000,
    channels: 2,
};
const EXPORT_PIPELINE_DEPTH: usize = 3;
const DECODE_PREFETCH_FRAMES: usize = 3;

pub(crate) enum ExportProgress {
    Frame(u64),
    Finished(Result<(), ExportError>),
}

enum EncoderMessage {
    Frame { index: u64, yuv: Vec<u8> },
    Complete,
}

pub(crate) fn export_timeline(
    timeline: TimelineSnapshot,
    renderer: Arc<FrameRenderer>,
    media_readers: Arc<MediaReaderRegistry>,
    settings: ExportSettings,
    progress: futures::channel::mpsc::UnboundedSender<ExportProgress>,
) -> Result<(), ExportError> {
    let frame_count = timeline.end_frame_exclusive().get();
    if frame_count == 0 {
        return Err(ExportError::InvalidTimeline(
            "There are no items to export on the timeline".to_owned(),
        ));
    }
    let (files, errors) = refresh_files(
        &mut item_readings(&timeline.visible_items()),
        timeline.media_cache(),
        &media_readers,
    );
    if let Some(error) = errors.into_iter().next() {
        return Err(error.into());
    }
    let mut cache = timeline.media_cache().clone();
    for file in files {
        cache.record(&file);
    }
    let timeline = timeline.with_media_cache(cache);
    let frame_rate = timeline.frame_rate();
    let size = RenderSize::from(timeline.resolution());
    let output_spec = VideoOutputSpec {
        width: size.width,
        height: size.height,
        color: VideoColorSpec::BT709_LIMITED,
    }
    .validate()
    .map_err(ExportError::encoding)?;
    let audio_graph = AudioTimelineGraph::new(
        &timeline.visible_items(),
        timeline.media_cache(),
        frame_rate,
        EXPORT_AUDIO_FORMAT,
        &media_readers,
        AudioGainEvaluation::TimelineAnimation,
    )
    .map_err(ExportError::encoding)?;
    let video_frame_rate = VideoFrameRate::new(frame_rate.numerator(), frame_rate.denominator())
        .ok_or_else(|| ExportError::encoding("Invalid output frame rate"))?;
    let mut readbacks = ExportFramePipeline::new(renderer, size, EXPORT_PIPELINE_DEPTH)?;
    thread::scope(|scope| {
        let output = settings.output;
        let (frames_to_encode, rendered_frames) = mpsc::sync_channel(EXPORT_PIPELINE_DEPTH);
        let encoder_worker = thread::Builder::new()
            .name("zerium-export-encoder".to_owned())
            .spawn_scoped(scope, move || {
                encode_frames(
                    output,
                    video_frame_rate,
                    rendered_frames,
                    audio_graph,
                    frame_rate,
                    output_spec,
                )
            })
            .map_err(|error| {
                ExportError::encoding(format!("Failed to start video encoding thread: {error}"))
            })?;
        // Scene evaluation, video decoding, and text rasterization run ahead on a
        // worker so the render thread only waits on the GPU, never on the CPU.
        let (decoded_scenes_tx, decoded_scenes_rx) = mpsc::sync_channel(DECODE_PREFETCH_FRAMES);
        let decode_worker = thread::Builder::new()
            .name("zerium-export-decode".to_owned())
            .spawn_scoped(scope, move || {
                decode_scenes(
                    timeline,
                    media_readers,
                    size,
                    frame_count,
                    decoded_scenes_tx,
                )
            })
            .map_err(|error| {
                ExportError::encoding(format!("Failed to start video decoding thread: {error}"))
            })?;

        let render_result: Result<(), ExportError> = (|| {
            for _ in 0..frame_count {
                let (frame_index, scene) = decoded_scenes_rx.recv().map_err(|_| {
                    ExportError::encoding("Video decoding thread terminated unexpectedly")
                })??;
                if let Some(frame) = readbacks.submit(frame_index, &scene)? {
                    send_frame(&frames_to_encode, frame)?;
                }
                let _ =
                    progress.unbounded_send(ExportProgress::Frame(frame_index.saturating_add(1)));
            }
            while let Some(frame) = readbacks.finish_next()? {
                send_frame(&frames_to_encode, frame)?;
            }
            frames_to_encode
                .send(EncoderMessage::Complete)
                .map_err(|_| ExportError::encoding("Video encoder terminated unexpectedly"))?;
            Ok(())
        })();
        drop(decoded_scenes_rx);
        let decode_result = decode_worker.join();
        drop(frames_to_encode);
        let encoding_result = encoder_worker.join().unwrap_or_else(|_| {
            Err(ExportError::encoding(
                "Video encoding thread terminated unexpectedly",
            ))
        });
        render_result?;
        decode_result
            .map_err(|_| ExportError::encoding("Video decoding thread terminated unexpectedly"))?;
        encoding_result
    })
}

fn decode_scenes(
    timeline: TimelineSnapshot,
    media_readers: Arc<MediaReaderRegistry>,
    size: RenderSize,
    frame_count: u64,
    scenes: mpsc::SyncSender<Result<(u64, RenderScene), ExportError>>,
) {
    let composition_size = RenderSize::from(timeline.resolution());
    let mut decoders = HashMap::<MediaInputId, ExportDecoder>::new();
    let mut text_frames = TextFrameCache::new();
    let cancelled = AtomicBool::new(false);
    for frame_index in 0..frame_count {
        let frame = Frame::new(frame_index);
        let render_time = TimelineTime::from_frame(frame);
        let active_items = timeline.active_items_at(frame);
        text_frames.retain_active(active_items.iter().map(|(_, item)| item));
        let result = RenderScene::from_timeline(
            &timeline,
            render_time,
            size,
            RenderQuality::Full,
            |request| {
                decode_texture_frame(
                    &timeline,
                    request,
                    &mut decoders,
                    &media_readers,
                    &cancelled,
                )
            },
            |request| text_frames.frame_for(request, composition_size),
        )
        .map(|scene| (frame_index, scene));
        let failed = result.is_err();
        if scenes.send(result).is_err() || failed {
            break;
        }
    }
}

fn send_frame(
    sender: &mpsc::SyncSender<EncoderMessage>,
    (index, yuv): (u64, Vec<u8>),
) -> Result<(), ExportError> {
    sender
        .send(EncoderMessage::Frame { index, yuv })
        .map_err(|_| ExportError::encoding("Video encoder terminated unexpectedly"))
}

fn encode_frames(
    output: PathBuf,
    video_frame_rate: VideoFrameRate,
    frames: mpsc::Receiver<EncoderMessage>,
    mut audio_graph: AudioTimelineGraph,
    frame_rate: FrameRate,
    output_spec: VideoOutputSpec,
) -> Result<(), ExportError> {
    let transaction = AtomicFileTransaction::new(&output).map_err(|error| {
        ExportError::encoding(format!(
            "Failed to create output temporary file '{}': {error}",
            output.display()
        ))
    })?;
    let has_audio = !audio_graph.is_empty();
    let mut encoder = FfmpegFileEncoder::create(
        transaction.temporary_path(),
        VideoEncoderSettings {
            output: output_spec,
            frame_rate: video_frame_rate,
            preset: "medium",
            crf: 18,
            gop: frame_rate
                .frames_per_second()
                .round()
                .clamp(1., f64::from(u32::MAX)) as u32,
            audio: has_audio.then_some(EXPORT_AUDIO_FORMAT),
            container: Some("mp4"),
            fast_start: true,
        },
    )
    .map_err(ExportError::encoding)?;
    for message in frames {
        let EncoderMessage::Frame { index, yuv } = message else {
            encoder.finish().map_err(ExportError::encoding)?;
            transaction.commit().map_err(|error| {
                ExportError::encoding(format!(
                    "Failed to finalize output '{}': {error}",
                    output.display()
                ))
            })?;
            return Ok(());
        };
        encoder
            .encode_yuv420p(&yuv, index)
            .map_err(ExportError::encoding)?;
        if has_audio {
            let start = sample_boundary(index, frame_rate, EXPORT_AUDIO_FORMAT.sample_rate);
            let end = sample_boundary(
                index.saturating_add(1),
                frame_rate,
                EXPORT_AUDIO_FORMAT.sample_rate,
            );
            let frame_count = usize::try_from(end.saturating_sub(start))
                .map_err(|_| ExportError::encoding("Export audio range is too large"))?;
            let audio = audio_graph
                .render(start, frame_count)
                .map_err(ExportError::encoding)?;
            encoder
                .encode_audio(&audio)
                .map_err(ExportError::encoding)?;
        }
    }
    Err(ExportError::encoding(
        "Export input closed before rendering completed",
    ))
}

fn decode_texture_frame(
    timeline: &dyn TimelineView,
    request: MediaFrameRequest<'_>,
    decoders: &mut HashMap<MediaInputId, ExportDecoder>,
    media_readers: &MediaReaderRegistry,
    cancelled: &AtomicBool,
) -> Result<Option<Arc<RgbaFrame>>, ExportError> {
    let MediaFrameRequest {
        item_id,
        effect_id,
        input_id,
        time,
        target_size,
    } = request;
    let Some(item) = timeline
        .active_items_at_time(time)
        .into_iter()
        .find_map(|(_, item)| (item.id == item_id).then_some(item))
    else {
        return Ok(None);
    };
    let Some((asset, playback)) = item.media_input(effect_id, input_id, timeline.media_cache())
    else {
        return Ok(None);
    };
    let local_seconds = item.local_seconds(time, timeline.frame_rate());
    let presentation_time = if asset.kind.is_temporal() {
        let Some(sample) = playback.sample(local_seconds, asset.duration) else {
            return Ok(None);
        };
        sample.time
    } else {
        Duration::ZERO
    };
    let id = MediaInputId {
        item_id,
        effect_id,
        input_id: input_id.to_owned(),
    };
    let decoder = match decoders.entry(id) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if entry.get().asset != asset {
                entry.insert(ExportDecoder {
                    asset: asset.clone(),
                    decoder: media_readers.open_visual_decoder(&asset)?,
                });
            }
            entry.into_mut()
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            let decoder = media_readers.open_visual_decoder(&asset)?;
            entry.insert(ExportDecoder {
                asset: asset.clone(),
                decoder,
            })
        }
    };
    let decode_size = RenderScene::media_raster_size_for_input(
        &item,
        effect_id,
        input_id,
        target_size,
        RenderSize::from(timeline.resolution()),
    );
    let decoded = decoder.decoder.decode_at(
        presentation_time,
        VideoDecodeSize {
            max_width: decode_size.width,
            max_height: decode_size.height,
        },
        cancelled,
    )?;
    Ok(Some(Arc::new(decoded.frame)))
}
