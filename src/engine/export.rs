use std::{
    collections::HashMap,
    error::Error,
    fmt,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use crate::{
    domain::{
        media::{MediaAsset, MediaKind, VideoFrameRate},
        timeline::{Frame, FrameRate, ItemId, TimelineSnapshot, TimelineTime, TimelineView},
    },
    engine::{
        frame::RgbaFrame,
        media::{
            AtomicFileTransaction, AudioFormat, AudioGainEvaluation, AudioTimelineGraph,
            FfmpegFileEncoder, MediaReaderRegistry, VideoColorSpec, VideoDecodeSize,
            VideoDecoderSession, VideoEncoderSettings, VideoOutputSpec, sample_boundary,
        },
        rendering::{ExportFramePipeline, FrameRenderer, RenderScene, RenderSize, TextFrameCache},
    },
};

#[derive(Clone, Debug)]
pub(crate) struct ExportSettings {
    pub output: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExportError {
    InvalidTimeline(String),
    Media(crate::engine::media::MediaError),
    Render(crate::engine::rendering::RenderError),
    Encoding(String),
}

impl ExportError {
    fn encoding(message: impl Into<String>) -> Self {
        Self::Encoding(message.into())
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTimeline(message) | Self::Encoding(message) => {
                formatter.write_str(message)
            }
            Self::Media(error) => error.fmt(formatter),
            Self::Render(error) => error.fmt(formatter),
        }
    }
}

impl Error for ExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Media(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::InvalidTimeline(_) | Self::Encoding(_) => None,
        }
    }
}

impl From<crate::engine::media::MediaError> for ExportError {
    fn from(error: crate::engine::media::MediaError) -> Self {
        Self::Media(error)
    }
}

impl From<crate::engine::rendering::RenderError> for ExportError {
    fn from(error: crate::engine::rendering::RenderError) -> Self {
        Self::Render(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TextureInputId {
    item_id: ItemId,
    input_id: String,
}

struct ExportDecoder {
    asset: MediaAsset,
    decoder: Box<dyn VideoDecoderSession>,
}

const EXPORT_AUDIO_FORMAT: AudioFormat = AudioFormat {
    sample_rate: 48_000,
    channels: 2,
};
const EXPORT_PIPELINE_DEPTH: usize = 2;

struct RenderedFrame {
    index: u64,
    rgba: Vec<u8>,
}

enum EncoderMessage {
    Frame(RenderedFrame),
    Complete,
}

enum EncoderWorkerError {
    Export(ExportError),
    IncompleteInput,
}

impl From<ExportError> for EncoderWorkerError {
    fn from(error: ExportError) -> Self {
        Self::Export(error)
    }
}

pub(crate) fn export_timeline(
    timeline: TimelineSnapshot,
    renderer: Arc<FrameRenderer>,
    media_readers: Arc<MediaReaderRegistry>,
    settings: ExportSettings,
) -> Result<(), ExportError> {
    let frame_count = timeline.end_frame_exclusive().get();
    if frame_count == 0 {
        return Err(ExportError::InvalidTimeline(
            "タイムラインに書き出せるアイテムがありません".to_owned(),
        ));
    }
    let frame_rate = timeline.frame_rate();
    let size = RenderSize::from(timeline.resolution());
    let composition_size = RenderSize::from(timeline.resolution());
    let output_spec = VideoOutputSpec {
        width: size.width,
        height: size.height,
        color: VideoColorSpec::BT709_LIMITED,
    }
    .validate()
    .map_err(ExportError::encoding)?;
    let audio_graph = AudioTimelineGraph::new(
        timeline.visible_items(),
        frame_rate,
        EXPORT_AUDIO_FORMAT,
        &media_readers,
        AudioGainEvaluation::TimelineAnimation,
    )
    .map_err(|error| ExportError::encoding(error.to_string()))?;
    let video_frame_rate = VideoFrameRate::new(frame_rate.numerator(), frame_rate.denominator())
        .ok_or_else(|| ExportError::encoding("出力フレームレートが不正です"))?;
    let mut readbacks = ExportFramePipeline::new(renderer, size, EXPORT_PIPELINE_DEPTH)?;
    let output = settings.output;
    let (frames_to_encode, rendered_frames) = mpsc::sync_channel(EXPORT_PIPELINE_DEPTH);
    let encoder_worker = thread::Builder::new()
        .name("zerium-export-encoder".to_owned())
        .spawn(move || {
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
            ExportError::encoding(format!("映像エンコードスレッドを開始できません: {error}"))
        })?;
    let mut decoders = HashMap::<TextureInputId, ExportDecoder>::new();
    let mut text_frames = TextFrameCache::new();

    let render_result = (|| {
        for frame_index in 0..frame_count {
            let frame = Frame::new(frame_index);
            let render_time = TimelineTime::from_frame(frame);
            let active_items = timeline.active_items_at(frame);
            text_frames.retain_active(active_items.iter().map(|(_, item)| item.id));
            let effect_size =
                RenderScene::effect_render_size_for_timeline(&timeline, render_time, size)?;
            let scene = RenderScene::from_timeline(
                &timeline,
                render_time,
                size,
                |request| {
                    decode_texture_frame(
                        &timeline,
                        request.item_id,
                        request.input_id,
                        request.time,
                        effect_size,
                        &mut decoders,
                        &media_readers,
                    )
                },
                |item, schema, target_size| {
                    text_frames.frame_for(item, schema, target_size, composition_size)
                },
            )?;
            if let Some(frame) = readbacks.submit(frame_index, &scene)? {
                send_frame(&frames_to_encode, frame)?;
            }
        }
        while let Some(frame) = readbacks.finish_next()? {
            send_frame(&frames_to_encode, frame)?;
        }
        frames_to_encode
            .send(EncoderMessage::Complete)
            .map_err(|_| ExportError::encoding("映像エンコーダーが予期せず終了しました"))?;
        Ok(())
    })();
    drop(frames_to_encode);
    let encoding_result = encoder_worker
        .join()
        .map_err(|_| ExportError::encoding("映像エンコードスレッドが予期せず終了しました"))?;
    match (render_result, encoding_result) {
        (Err(error), Err(EncoderWorkerError::IncompleteInput)) => Err(error),
        (_, Err(EncoderWorkerError::Export(error))) => Err(error),
        (Ok(()), Err(EncoderWorkerError::IncompleteInput)) => Err(ExportError::encoding(
            "レンダリングが完了する前に書き出し入力が閉じられました",
        )),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn send_frame(
    sender: &mpsc::SyncSender<EncoderMessage>,
    (index, rgba): (u64, Vec<u8>),
) -> Result<(), ExportError> {
    sender
        .send(EncoderMessage::Frame(RenderedFrame { index, rgba }))
        .map_err(|_| ExportError::encoding("映像エンコーダーが予期せず終了しました"))
}

fn encode_frames(
    output: PathBuf,
    video_frame_rate: VideoFrameRate,
    frames: mpsc::Receiver<EncoderMessage>,
    mut audio_graph: AudioTimelineGraph,
    frame_rate: FrameRate,
    output_spec: VideoOutputSpec,
) -> Result<(), EncoderWorkerError> {
    let transaction = AtomicFileTransaction::new(&output).map_err(|error| {
        ExportError::encoding(format!(
            "出力一時ファイル'{}'を作成できません: {error}",
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
        let EncoderMessage::Frame(frame) = message else {
            encoder.finish().map_err(ExportError::encoding)?;
            transaction.commit().map_err(|error| {
                ExportError::encoding(format!(
                    "出力'{}'を確定できません: {error}",
                    output.display()
                ))
            })?;
            return Ok(());
        };
        encoder
            .encode_video(
                &RgbaFrame {
                    width: output_spec.width,
                    height: output_spec.height,
                    rgba: frame.rgba.into(),
                },
                frame.index,
            )
            .map_err(ExportError::encoding)?;
        if has_audio {
            let start = sample_boundary(frame.index, frame_rate, EXPORT_AUDIO_FORMAT.sample_rate);
            let end = sample_boundary(
                frame.index.saturating_add(1),
                frame_rate,
                EXPORT_AUDIO_FORMAT.sample_rate,
            );
            let frame_count = usize::try_from(end.saturating_sub(start))
                .map_err(|_| ExportError::encoding("書き出し音声範囲が大きすぎます"))?;
            let audio = audio_graph
                .render(start, frame_count)
                .map_err(|error| ExportError::encoding(error.to_string()))?;
            encoder
                .encode_audio(&audio)
                .map_err(ExportError::encoding)?;
        }
    }
    Err(EncoderWorkerError::IncompleteInput)
}

fn decode_texture_frame(
    timeline: &dyn TimelineView,
    item_id: ItemId,
    input_id: &str,
    time: TimelineTime,
    size: RenderSize,
    decoders: &mut HashMap<TextureInputId, ExportDecoder>,
    media_readers: &MediaReaderRegistry,
) -> Result<Option<Arc<RgbaFrame>>, ExportError> {
    let Some(item) = timeline
        .active_items_at_time(time)
        .into_iter()
        .find_map(|(_, item)| (item.id == item_id).then_some(item))
    else {
        return Ok(None);
    };
    let Some(asset) = item.assets.get(input_id) else {
        return Ok(None);
    };
    if matches!(asset.kind, MediaKind::Audio { .. }) {
        return Ok(None);
    }
    let id = TextureInputId {
        item_id,
        input_id: input_id.to_owned(),
    };
    let decoder = match decoders.entry(id) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if entry.get().asset != *asset {
                entry.insert(ExportDecoder {
                    asset: asset.clone(),
                    decoder: media_readers.open_video_decoder(asset)?,
                });
            }
            entry.into_mut()
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            let decoder = media_readers.open_video_decoder(asset)?;
            entry.insert(ExportDecoder {
                asset: asset.clone(),
                decoder,
            })
        }
    };
    let local_frames = (time.frames() - item.start.get() as f64).max(0.);
    let local_seconds = local_frames / timeline.frame_rate().frames_per_second();
    let presentation_time = match asset.kind {
        MediaKind::Video { .. } => {
            looped_presentation_time(local_seconds, decoder.decoder.stream_duration())?
        }
        MediaKind::Image { .. } => Duration::ZERO,
        MediaKind::Audio { .. } => unreachable!("audio inputs were skipped above"),
    };
    let decoded = decoder.decoder.decode_at(
        presentation_time,
        VideoDecodeSize {
            max_width: size.width,
            max_height: size.height,
        },
    )?;
    Ok(Some(Arc::new(decoded.frame)))
}

fn looped_presentation_time(
    local_seconds: f64,
    stream_duration: Duration,
) -> Result<Duration, ExportError> {
    let duration = stream_duration.as_secs_f64();
    if !local_seconds.is_finite() || local_seconds < 0. || !duration.is_finite() || duration <= 0. {
        return Err(ExportError::encoding("映像ストリームの再生時刻が不正です"));
    }
    Duration::try_from_secs_f64(local_seconds.rem_euclid(duration))
        .map_err(|_| ExportError::encoding("映像ストリームの再生時刻が不正です"))
}
