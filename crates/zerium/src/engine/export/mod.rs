use crate::engine::gpu::GpuContext;
pub(crate) mod encoding;
mod video;

use std::{
    collections::HashMap,
    fmt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use thiserror::Error;

use crate::engine::{
    atomic_file::AtomicFileTransaction,
    gstreamer::encoding::Mp4Writer,
    media::{
        AudioCalculationPolicy, AudioFormat, AudioTimelineGraph, DecodeStreamKey,
        MediaReaderRegistry, VisualDecodeSession, dependency_readings, refresh_files,
        sample_boundary,
    },
    rendering::{RenderScene, RenderSession, RenderSize, SceneEvaluator},
};
use encoding::{
    EncodingError, EncodingSettings, VideoDimensions, VideoEncoderFrame, VideoEncoderInput,
};
use video::ExportVideoPipeline;

use zerium_core::{
    media::VideoFrameRate,
    timeline::{Frame, FrameRate, TimelineSnapshot, TimelineTime, TimelineView},
};

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum ExportError {
    #[error("Export cancelled")]
    Cancelled,
    #[error("Export input closed before rendering completed")]
    InputClosed,
    #[error("{0}")]
    InvalidTimeline(String),
    #[error(transparent)]
    Media(#[from] crate::engine::media::MediaError),
    #[error(transparent)]
    Render(#[from] crate::engine::rendering::RenderError),
    #[error(transparent)]
    Audio(#[from] crate::engine::media::AudioTimelineError),
    #[error("{0}")]
    Encoding(String),
}

impl ExportError {
    pub(crate) fn encoding(message: impl fmt::Display) -> Self {
        Self::Encoding(message.to_string())
    }
}

impl From<EncodingError> for ExportError {
    fn from(error: EncodingError) -> Self {
        match error {
            EncodingError::Cancelled => Self::Cancelled,
            error => Self::encoding(error),
        }
    }
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

enum WriterMessage {
    Frame(VideoEncoderFrame),
    Complete,
}

pub(crate) fn export_timeline(
    timeline: TimelineSnapshot,
    renderer: RenderSession,
    media_readers: Arc<MediaReaderRegistry>,
    output: PathBuf,
    cancelled: &AtomicBool,
    progress: futures::channel::mpsc::UnboundedSender<ExportProgress>,
) -> Result<(), ExportError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    let frame_count = timeline.end_frame_exclusive().get();
    if frame_count == 0 {
        return Err(ExportError::InvalidTimeline(
            "There are no items to export on the timeline".to_owned(),
        ));
    }
    let refresh = refresh_files(
        &mut dependency_readings(timeline.media_dependencies()),
        timeline.media_cache(),
        &media_readers,
    );
    if let Some((_, error)) = refresh.errors.into_iter().next() {
        return Err(error.into());
    }
    let mut cache = timeline.media_cache().clone();
    for file in refresh.files {
        cache.record(&file);
    }
    let timeline = timeline.with_media_cache(cache);
    let frame_rate = timeline.frame_rate();
    let size = RenderSize::from(timeline.resolution());
    let output_spec = VideoDimensions {
        width: size.width,
        height: size.height,
    }
    .validate()
    .map_err(ExportError::encoding)?;
    let audio_graph = AudioTimelineGraph::new(
        timeline.audio_snapshot(),
        EXPORT_AUDIO_FORMAT,
        &media_readers,
        AudioCalculationPolicy::Report,
    )?;
    let video_frame_rate = VideoFrameRate::new(frame_rate.numerator(), frame_rate.denominator())
        .ok_or_else(|| ExportError::encoding("Invalid output frame rate"))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    let video_settings = EncodingSettings {
        output: output_spec,
        frame_rate: video_frame_rate,
        gop: frame_rate
            .frames_per_second()
            .round()
            .clamp(1., f64::from(u32::MAX)) as u32,
        audio: (!audio_graph.is_empty()).then_some(EXPORT_AUDIO_FORMAT),
    };
    let decode_gpu = renderer.gpu().clone();
    let (mut frames, encoding) =
        ExportVideoPipeline::new(renderer, EXPORT_PIPELINE_DEPTH, video_settings)?;
    thread::scope(|scope| {
        let (frames_to_write, video_frames) = mpsc::sync_channel(EXPORT_PIPELINE_DEPTH);
        let writer_failed = frames.failure_signal();
        let writer_worker = thread::Builder::new()
            .name("zerium-export-encoder".to_owned())
            .spawn_scoped(scope, move || {
                let result = write_output(
                    output,
                    video_settings,
                    video_frames,
                    audio_graph,
                    frame_rate,
                    encoding,
                    cancelled,
                );
                if result.is_err() {
                    writer_failed.store(true, Ordering::Release);
                }
                result
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
                    &decode_gpu,
                    size,
                    frame_count,
                    decoded_scenes_tx,
                    cancelled,
                )
            })
            .map_err(|error| {
                ExportError::encoding(format!("Failed to start video decoding thread: {error}"))
            })?;

        let render_result: Result<(), ExportError> = (|| {
            for _ in 0..frame_count {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(ExportError::Cancelled);
                }
                let (frame_index, scene) = decoded_scenes_rx.recv().map_err(|_| {
                    ExportError::encoding("Video decoding thread terminated unexpectedly")
                })??;
                if frames.is_full()
                    && let Some(frame) = frames.finish_next(cancelled)?
                {
                    send_frame(&frames_to_write, frame)?;
                }
                frames.submit(frame_index, &scene, cancelled)?;
                let _ =
                    progress.unbounded_send(ExportProgress::Frame(frame_index.saturating_add(1)));
            }
            while let Some(frame) = frames.finish_next(cancelled)? {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(ExportError::Cancelled);
                }
                send_frame(&frames_to_write, frame)?;
            }
            frames.finish()?;
            frames_to_write
                .send(WriterMessage::Complete)
                .map_err(|_| ExportError::encoding("Video encoder terminated unexpectedly"))?;
            Ok(())
        })();
        drop(decoded_scenes_rx);
        let decode_result = decode_worker.join();
        drop(frames_to_write);
        let encoding_result = writer_worker.join().unwrap_or_else(|_| {
            Err(ExportError::encoding(
                "Video encoding thread terminated unexpectedly",
            ))
        });
        match encoding_result {
            // A closed input can follow an upstream failure. Preserve that
            // failure, while writer errors and cancellation take precedence.
            Ok(()) | Err(ExportError::InputClosed) => {
                render_result?;
                decode_result.map_err(|_| {
                    ExportError::encoding("Video decoding thread terminated unexpectedly")
                })?;
                encoding_result
            }
            Err(error) => Err(error),
        }
    })
}

fn decode_scenes(
    timeline: TimelineSnapshot,
    media_readers: Arc<MediaReaderRegistry>,
    gpu: &GpuContext,
    size: RenderSize,
    frame_count: u64,
    scenes: mpsc::SyncSender<Result<(u64, RenderScene), ExportError>>,
    cancelled: &AtomicBool,
) {
    let mut decoders = HashMap::<DecodeStreamKey, VisualDecodeSession>::new();
    let mut evaluator = SceneEvaluator::for_export();
    for frame_index in 0..frame_count {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let frame = Frame::new(frame_index);
        let render_time = TimelineTime::from_frame(frame);
        let mut active_streams = std::collections::HashSet::new();
        let result = evaluator
            .evaluate(&timeline, render_time, size, |request| {
                active_streams.insert(request.stream.clone());
                let decoder = decoders.entry(request.stream.clone()).or_default();
                let decoded = decoder.decode(&media_readers, &request, Some(gpu), cancelled)?;
                Ok::<_, ExportError>(Some(decoded.frame))
            })
            .map(|scene| (frame_index, scene));
        decoders.retain(|stream, _| active_streams.contains(stream));
        let failed = result.is_err();
        if scenes.send(result).is_err() || failed {
            break;
        }
    }
}

fn send_frame(
    sender: &mpsc::SyncSender<WriterMessage>,
    frame: VideoEncoderFrame,
) -> Result<(), ExportError> {
    sender
        .send(WriterMessage::Frame(frame))
        .map_err(|_| ExportError::encoding("Video encoder terminated unexpectedly"))
}

fn write_output(
    output: PathBuf,
    video_settings: EncodingSettings,
    frames: mpsc::Receiver<WriterMessage>,
    mut audio_graph: AudioTimelineGraph,
    frame_rate: FrameRate,
    encoding: VideoEncoderInput,
    cancelled: &AtomicBool,
) -> Result<(), ExportError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    let transaction = AtomicFileTransaction::new(&output).map_err(|error| {
        ExportError::encoding(format!(
            "Failed to create output temporary file '{}': {error}",
            output.display()
        ))
    })?;
    let has_audio = !audio_graph.is_empty();
    let mut encoder = Mp4Writer::create(
        transaction.temporary_path(),
        video_settings,
        encoding,
        cancelled,
    )
    .map_err(ExportError::from)?;
    for message in frames {
        if cancelled.load(Ordering::Relaxed) {
            return Err(ExportError::Cancelled);
        }
        let WriterMessage::Frame(frame) = message else {
            encoder.finish().map_err(ExportError::from)?;
            if cancelled.load(Ordering::Relaxed) {
                return Err(ExportError::Cancelled);
            }
            transaction.commit().map_err(|error| {
                ExportError::encoding(format!(
                    "Failed to finalize output '{}': {error}",
                    output.display()
                ))
            })?;
            return Ok(());
        };
        let index = frame.index;
        encoder.write_video(frame).map_err(ExportError::from)?;
        if has_audio {
            let start = sample_boundary(index, frame_rate, EXPORT_AUDIO_FORMAT.sample_rate);
            let end = sample_boundary(
                index.saturating_add(1),
                frame_rate,
                EXPORT_AUDIO_FORMAT.sample_rate,
            );
            let frame_count = usize::try_from(end.saturating_sub(start)).map_err(|_| {
                ExportError::Audio(crate::engine::media::AudioTimelineError::RangeTooLarge)
            })?;
            let audio = audio_graph.render(start, frame_count)?;
            encoder.write_audio(&audio).map_err(ExportError::from)?;
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    Err(ExportError::InputClosed)
}
