//! Frozen timeline export across engine input, processing, and output stages.
use crate::{
    app::storage::AtomicFileTransaction,
    engine::{
        GpuContext,
        input::{
            AudioFormat, DecodeStreamKey, VisualDecodeSession, dependency_readings, refresh_files,
        },
        output::{
            EncodingError, EncodingSettings, VideoDimensions, VideoEncoderFrame, VideoEncoderInput,
            visual::{MediaWriter, VideoOutput, render_png},
        },
        processing::{
            CalculationPolicy,
            audio::AudioTimelineGraph,
            visual::{RenderScene, RenderSession, RenderSize, SceneEvaluator},
        },
    },
};

use std::{
    collections::HashMap,
    fmt,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use thiserror::Error;

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
    Media(#[from] crate::engine::input::MediaError),
    #[error(transparent)]
    Render(#[from] crate::engine::processing::visual::RenderError),
    #[error(transparent)]
    Audio(#[from] crate::engine::processing::audio::AudioTimelineError),
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

const EXPORT_PIPELINE_DEPTH: usize = 3;
const DECODE_PREFETCH_FRAMES: usize = 3;

pub(crate) enum ExportProgress {
    Frame(u64),
    Finished(Result<(), ExportError>),
}

pub(crate) enum ExportedFrame {
    Saved,
    Png(Vec<u8>),
}

enum WriterMessage {
    Frame(VideoEncoderFrame),
    Complete,
}

/// Export the captured scope at its playhead to an atomic file or PNG bytes.
pub(crate) fn export_frame(
    timeline: TimelineSnapshot,
    renderer: RenderSession,
    output: Option<PathBuf>,
) -> Result<ExportedFrame, ExportError> {
    let timeline = refresh_media(timeline, false)?;
    let mut decoders = HashMap::<DecodeStreamKey, VisualDecodeSession>::new();
    let cancelled = AtomicBool::new(false);
    let mut scene = SceneEvaluator::new(CalculationPolicy::Report).evaluate(
        &timeline,
        TimelineTime::from_frame(timeline.playhead()),
        RenderSize::from(timeline.resolution()),
        |request| {
            let decoded = decoders.entry(request.stream.clone()).or_default().decode(
                &request,
                Some(renderer.gpu()),
                &cancelled,
            )?;
            Ok::<_, ExportError>(Some(decoded.frame))
        },
    )?;
    scene.background = [0.; 4];
    let png = render_png(&renderer, &scene)?;
    if let Some(path) = output {
        let transaction = AtomicFileTransaction::new(path).map_err(ExportError::encoding)?;
        transaction.write_all(&png).map_err(ExportError::encoding)?;
        transaction.commit().map_err(ExportError::encoding)?;
        Ok(ExportedFrame::Saved)
    } else {
        Ok(ExportedFrame::Png(png))
    }
}

fn refresh_media(
    timeline: TimelineSnapshot,
    include_audio: bool,
) -> Result<TimelineSnapshot, ExportError> {
    let refresh = refresh_files(
        &mut dependency_readings(timeline.media_dependencies().into_iter().filter(|source| {
            include_audio || source.target != zerium_core::media::MediaTarget::Audio
        })),
        timeline.media_cache(),
    );
    if let Some((_, error)) = refresh.errors.into_iter().next() {
        return Err(error.into());
    }
    let mut cache = timeline.media_cache().clone();
    for file in refresh.files {
        cache.record(&file);
    }
    Ok(timeline.with_media_cache(cache))
}

pub(crate) fn export_timeline(
    timeline: TimelineSnapshot,
    renderer: RenderSession,
    output: PathBuf,
    cancelled: &AtomicBool,
    progress: futures::channel::mpsc::UnboundedSender<ExportProgress>,
) -> Result<(), ExportError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    let settings = timeline.export_settings().clone();
    settings
        .validate()
        .map_err(|error| ExportError::InvalidTimeline(error.to_string()))?;
    let frame_count = timeline.end_frame_exclusive().get();
    if frame_count == 0 {
        return Err(ExportError::InvalidTimeline(
            "There are no items to export on the timeline".to_owned(),
        ));
    }
    let timeline = refresh_media(timeline, settings.exports_audio())?;
    let frame_rate = timeline.frame_rate();
    let size = RenderSize::from(timeline.resolution());
    let output_spec = VideoDimensions {
        width: size.width,
        height: size.height,
    }
    .validate()
    .map_err(ExportError::encoding)?;
    let audio_format = AudioFormat {
        sample_rate: settings.audio_sample_rate,
        channels: settings.audio_channels,
    };
    let audio_graph = if settings.exports_audio() {
        Some(AudioTimelineGraph::new(
            timeline.audio_snapshot(),
            audio_format,
            CalculationPolicy::Report,
        )?)
        .filter(|graph| !graph.is_empty())
    } else {
        None
    };
    let video_frame_rate = VideoFrameRate::new(frame_rate.numerator(), frame_rate.denominator())
        .ok_or_else(|| ExportError::encoding("Invalid output frame rate"))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    let video_settings = EncodingSettings {
        color: settings.color,
        container: settings.container,
        gif_loop: settings.gif_loop,
        video_codec: settings.video_codec,
        audio_codec: settings.audio_codec,
        video_bitrate_kbps: settings.video_bitrate_kbps,
        audio_bitrate_kbps: settings.audio_bitrate_kbps,
        output: output_spec,
        frame_rate: video_frame_rate,
        gop: frame_rate
            .frames_per_second()
            .round()
            .clamp(1., f64::from(u32::MAX)) as u32,
        audio: audio_graph.as_ref().map(|_| audio_format),
    };
    let decode_gpu = renderer.gpu().clone();
    let (mut frames, encoding) = VideoOutput::new(renderer, EXPORT_PIPELINE_DEPTH, video_settings)?;
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
    gpu: &GpuContext,
    size: RenderSize,
    frame_count: u64,
    scenes: mpsc::SyncSender<Result<(u64, RenderScene), ExportError>>,
    cancelled: &AtomicBool,
) {
    let mut decoders = HashMap::<DecodeStreamKey, VisualDecodeSession>::new();
    let mut evaluator = SceneEvaluator::new(CalculationPolicy::Report);
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
                let decoded = decoder.decode(&request, Some(gpu), cancelled)?;
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
    mut audio_graph: Option<AudioTimelineGraph>,
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
    let mut encoder = MediaWriter::create(
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
        if let Some(audio_graph) = &mut audio_graph {
            let format = video_settings.audio.expect("audio graph format");
            let start = frame_rate.sample_boundary(Frame::new(index), format.sample_rate);
            let end =
                frame_rate.sample_boundary(Frame::new(index.saturating_add(1)), format.sample_rate);
            let frame_count = usize::try_from(end.saturating_sub(start)).map_err(|_| {
                ExportError::Audio(
                    crate::engine::processing::audio::AudioTimelineError::RangeTooLarge,
                )
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
