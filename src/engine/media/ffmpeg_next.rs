use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ffmpeg_next as ffmpeg;

use crate::{
    domain::media::{MediaAsset, MediaKind, VideoFrameRate},
    domain::plugin::MediaType,
    engine::frame::RgbaFrame,
};

use super::{
    ffmpeg::fit_dimensions,
    reader::{
        AudioDecoderSession, AudioFormat, DecodedAudioBlock, DecodedVideoFrame, MediaError,
        MediaProbe, MediaStreamDurations, VideoDecodeSize, VideoDecoderSession,
    },
};

fn interactive_decode_threads() -> usize {
    std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(4)
        .clamp(2, 8)
}

struct VideoScaler(ffmpeg::software::scaling::Context);

// SAFETY: `SwsContext` has no thread affinity. The context is owned by one
// decoder session and is only accessed through `&mut self`, so it cannot be
// used concurrently when the session is moved between worker threads.
unsafe impl Send for VideoScaler {}

pub(super) struct FfmpegVideoDecoder {
    asset: MediaAsset,
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    video_stream_index: usize,
    stream_time_base: ffmpeg::Rational,
    stream_start_time: i64,
    stream_duration: Duration,
    scaler: Option<VideoScaler>,
    output_size: Option<(u32, u32)>,
    still_image: Option<ffmpeg::frame::Video>,
    pending_decoded: Option<ffmpeg::frame::Video>,
    last_frame: Option<DecodedVideoFrame>,
    trail: VecDeque<TrailFrame>,
    trail_bytes: usize,
    fallback_time: Duration,
    draining: bool,
}

struct TrailFrame {
    decoded: ffmpeg::frame::Video,
    presentation_time: Duration,
    duration: Duration,
    bytes: usize,
}

const TRAIL_FRAMES: usize = 8;
const TRAIL_BYTES: usize = 32 * 1024 * 1024;

enum SeekCandidate {
    Ready(DecodedVideoFrame),
    Raw {
        decoded: ffmpeg::frame::Video,
        presentation_time: Duration,
        duration: Duration,
    },
}

impl FfmpegVideoDecoder {
    fn cache_candidate(&mut self, candidate: Option<SeekCandidate>, width: u32, height: u32) {
        self.last_frame = match candidate {
            Some(SeekCandidate::Ready(frame)) => Some(frame),
            Some(SeekCandidate::Raw {
                decoded,
                presentation_time,
                duration,
            }) => self
                .convert_candidate(&decoded, presentation_time, duration, width, height)
                .ok(),
            None => None,
        };
    }

    fn convert_candidate(
        &mut self,
        decoded: &ffmpeg::frame::Video,
        presentation_time: Duration,
        duration: Duration,
        width: u32,
        height: u32,
    ) -> Result<DecodedVideoFrame, MediaError> {
        Ok(DecodedVideoFrame {
            presentation_time,
            duration,
            frame: self.rgba_frame(decoded, width, height)?,
        })
    }

    fn push_trail(
        &mut self,
        decoded: ffmpeg::frame::Video,
        presentation_time: Duration,
        duration: Duration,
    ) {
        let bytes = decoded.width() as usize * decoded.height() as usize * 3 / 2;
        self.trail_bytes = self.trail_bytes.saturating_add(bytes);
        self.trail.push_back(TrailFrame {
            decoded,
            presentation_time,
            duration,
            bytes,
        });
        while self.trail.len() > TRAIL_FRAMES || self.trail_bytes > TRAIL_BYTES {
            let Some(old) = self.trail.pop_front() else {
                break;
            };
            self.trail_bytes = self.trail_bytes.saturating_sub(old.bytes);
        }
    }

    fn trail_frame(
        &mut self,
        presentation_time: Duration,
        width: u32,
        height: u32,
    ) -> Option<DecodedVideoFrame> {
        let index = self
            .trail
            .iter()
            .enumerate()
            .filter(|(_, frame)| {
                frame.presentation_time <= presentation_time
                    && presentation_time
                        < frame
                            .presentation_time
                            .checked_add(frame.duration)
                            .unwrap_or(Duration::MAX)
            })
            .max_by_key(|(_, frame)| frame.presentation_time)
            .map(|(index, _)| index)?;
        let hit = self.trail.remove(index)?;
        let frame = self.rgba_frame(&hit.decoded, width, height).ok()?;
        let frame = DecodedVideoFrame {
            presentation_time: hit.presentation_time,
            duration: hit.duration,
            frame,
        };
        self.last_frame = Some(frame.clone());
        self.trail_bytes = self.trail_bytes.saturating_sub(hit.bytes);
        Some(frame)
    }
}

const KEYFRAME_SCAN_PACKETS: usize = 3000;

pub(crate) fn estimate_max_keyframe_gap(path: &Path) -> Option<u64> {
    let mut input = open_input(path).ok()?;
    let video_stream_index = input.streams().best(ffmpeg::media::Type::Video)?.index();
    let mut since_key = 0u64;
    let mut max_gap = 0u64;
    let mut keyframes = 0u64;
    let mut scanned = 0usize;
    let mut exhausted = true;
    for (stream, packet) in input.packets() {
        if stream.index() != video_stream_index {
            continue;
        }
        if scanned >= KEYFRAME_SCAN_PACKETS {
            exhausted = false;
            break;
        }
        scanned += 1;
        if packet.is_key() {
            keyframes += 1;
            max_gap = max_gap.max(since_key);
            since_key = 0;
        } else {
            since_key += 1;
        }
    }
    if keyframes == 0 {
        return None;
    }
    if exhausted {
        max_gap = max_gap.max(since_key);
    }
    Some(max_gap)
}

pub(super) fn probe(path: &Path, media_type: MediaType) -> Result<MediaProbe, MediaError> {
    initialize_ffmpeg()?;
    let input = ffmpeg::format::input(path).map_err(|error| {
        MediaError::external(format!("'{}'を解析できません: {error}", path.display()))
    })?;
    let video = input.streams().best(ffmpeg::media::Type::Video);
    let audio = input.streams().best(ffmpeg::media::Type::Audio);
    if media_type == MediaType::Image {
        let stream = video.ok_or_else(|| MediaError::external("画像ストリームが見つかりません"))?;
        let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .and_then(|context| context.decoder().video())
            .map_err(|error| MediaError::external(format!("画像情報を取得できません: {error}")))?;
        if decoder.width() == 0 || decoder.height() == 0 {
            return Err(MediaError::external("画像サイズを取得できません"));
        }
        return Ok(MediaProbe {
            duration: Duration::from_secs(5),
            kind: MediaKind::Image {
                width: decoder.width(),
                height: decoder.height(),
            },
            streams: MediaStreamDurations {
                video: Some(Duration::from_secs(5)),
            },
        });
    }

    let duration = media_duration(&input)
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .ok_or_else(|| MediaError::external("メディアの再生時間を取得できません"))?;
    let stream_durations = MediaStreamDurations {
        video: video.as_ref().and_then(stream_duration),
    };
    let kind = if let Some(stream) = video {
        let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .and_then(|context| context.decoder().video())
            .map_err(|error| MediaError::external(format!("映像情報を取得できません: {error}")))?;
        let frame_rate = video_frame_rate(stream.avg_frame_rate())
            .or_else(|| video_frame_rate(stream.rate()))
            .ok_or_else(|| MediaError::external("動画のフレームレートを取得できません"))?;
        let frame_count = u64::try_from(stream.frames())
            .ok()
            .filter(|frames| *frames > 0)
            .unwrap_or_else(|| {
                (duration.as_secs_f64() * frame_rate.frames_per_second())
                    .ceil()
                    .clamp(1., u64::MAX as f64) as u64
            });
        MediaKind::Video {
            width: decoder.width(),
            height: decoder.height(),
            frame_rate,
            frame_count,
            has_audio: audio.is_some(),
        }
    } else if let Some(stream) = audio {
        let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .and_then(|context| context.decoder().audio())
            .map_err(|error| MediaError::external(format!("音声情報を取得できません: {error}")))?;
        MediaKind::Audio {
            channels: Some(u32::from(decoder.channels())).filter(|channels| *channels > 0),
            sample_rate: Some(decoder.rate()).filter(|rate| *rate > 0),
        }
    } else {
        return Err(MediaError::external(
            "動画または音声ストリームが見つかりません",
        ));
    };
    Ok(MediaProbe {
        duration,
        kind,
        streams: stream_durations,
    })
}

fn stream_duration(stream: &ffmpeg::Stream<'_>) -> Option<Duration> {
    let value = stream.duration();
    if value <= 0 || value == ffmpeg::ffi::AV_NOPTS_VALUE {
        return None;
    }
    Duration::try_from_secs_f64(value as f64 * f64::from(stream.time_base())).ok()
}

fn stream_start_seconds(start_time: i64, time_base: ffmpeg::Rational) -> f64 {
    if start_time == ffmpeg::ffi::AV_NOPTS_VALUE {
        0.
    } else {
        start_time as f64 * f64::from(time_base)
    }
}

fn seek_timestamp(start_time: i64, time_base: ffmpeg::Rational, seconds: f64) -> i64 {
    ((seconds + stream_start_seconds(start_time, time_base)) * f64::from(ffmpeg::ffi::AV_TIME_BASE))
        .round()
        .clamp(0., i64::MAX as f64) as i64
}

fn stream_seconds(timestamp: i64, start_time: i64, time_base: ffmpeg::Rational) -> f64 {
    (timestamp.saturating_sub(start_time) as f64 * f64::from(time_base)).max(0.)
}

fn open_input(path: &Path) -> Result<ffmpeg::format::context::Input, MediaError> {
    initialize_ffmpeg()?;
    ffmpeg::format::input(path)
        .map_err(|error| MediaError::external(format!("'{}'を開けません: {error}", path.display())))
}

fn media_duration(input: &ffmpeg::format::context::Input) -> Option<f64> {
    let container_duration = input.duration();
    if container_duration > 0 && container_duration != ffmpeg::ffi::AV_NOPTS_VALUE {
        return Some(container_duration as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE));
    }
    input
        .streams()
        .filter_map(|stream| {
            (stream.duration() > 0)
                .then(|| stream.duration() as f64 * f64::from(stream.time_base()))
        })
        .filter(|duration| duration.is_finite() && *duration > 0.)
        .max_by(f64::total_cmp)
}

fn video_frame_rate(rate: ffmpeg::Rational) -> Option<VideoFrameRate> {
    let numerator = u32::try_from(rate.numerator()).ok()?;
    let denominator = u32::try_from(rate.denominator()).ok()?;
    VideoFrameRate::new(numerator, denominator)
}

pub(super) struct FfmpegAudioDecoder {
    asset: MediaAsset,
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Audio,
    stream_index: usize,
    stream_time_base: ffmpeg::Rational,
    stream_start_time: i64,
    resampler: Option<ffmpeg::software::resampling::Context>,
    output_format: Option<AudioFormat>,
    pending: VecDeque<f32>,
    next_sample_frame: Option<u64>,
    discard_before: Option<u64>,
    decoded_cursor: u64,
    draining: bool,
    stream_duration: Duration,
}

impl FfmpegAudioDecoder {
    pub(super) fn open(asset: MediaAsset) -> Result<Self, MediaError> {
        let input = open_input(&asset.path)?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .ok_or_else(|| MediaError::external("音声ストリームが見つかりません"))?;
        let stream_index = stream.index();
        let stream_time_base = stream.time_base();
        let stream_start_time = stream.start_time();
        let stream_duration = stream_duration(&stream).unwrap_or(asset.duration);
        let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .and_then(|context| context.decoder().audio())
            .map_err(|error| {
                MediaError::external(format!("音声デコーダーを開けません: {error}"))
            })?;
        Ok(Self {
            asset,
            input,
            decoder,
            stream_index,
            stream_time_base,
            stream_start_time,
            resampler: None,
            output_format: None,
            pending: VecDeque::new(),
            next_sample_frame: Some(0),
            discard_before: None,
            decoded_cursor: 0,
            draining: false,
            stream_duration,
        })
    }

    fn seek(&mut self, sample_frame: u64, format: AudioFormat) -> Result<(), MediaError> {
        let seconds = sample_frame as f64 / f64::from(format.sample_rate);
        let timestamp = seek_timestamp(self.stream_start_time, self.stream_time_base, seconds);
        self.input.seek(timestamp, ..timestamp).map_err(|error| {
            MediaError::external(format!(
                "'{}'の音声をシークできません: {error}",
                self.asset.path.display()
            ))
        })?;
        self.decoder.flush();
        self.pending.clear();
        self.next_sample_frame = Some(sample_frame);
        self.discard_before = Some(sample_frame);
        self.decoded_cursor = sample_frame;
        self.draining = false;
        Ok(())
    }

    fn next_decoded_frame(&mut self) -> Result<Option<ffmpeg::frame::Audio>, MediaError> {
        loop {
            let mut frame = ffmpeg::frame::Audio::empty();
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => return Ok(Some(frame)),
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {}
                Err(error) => {
                    return Err(MediaError::external(format!(
                        "'{}'の音声をデコードできません: {error}",
                        self.asset.path.display()
                    )));
                }
            }
            if let Some(packet) = self
                .input
                .packets()
                .find(|(stream, _)| stream.index() == self.stream_index)
                .map(|(_, packet)| packet)
            {
                self.decoder.send_packet(&packet).map_err(|error| {
                    MediaError::external(format!("音声パケットを送信できません: {error}"))
                })?;
            } else if self.draining {
                return Ok(None);
            } else {
                self.decoder.send_eof().map_err(|error| {
                    MediaError::external(format!("音声デコーダーを完了できません: {error}"))
                })?;
                self.draining = true;
            }
        }
    }

    fn append_frame(
        &mut self,
        decoded: &ffmpeg::frame::Audio,
        format: AudioFormat,
    ) -> Result<(), MediaError> {
        let source_layout = if decoded.channel_layout().is_empty() {
            ffmpeg::ChannelLayout::default(i32::from(decoded.channels()))
        } else {
            decoded.channel_layout()
        };
        let target_layout = ffmpeg::ChannelLayout::default(i32::from(format.channels));
        let input_changed = self.resampler.as_ref().is_none_or(|resampler| {
            let input = resampler.input();
            input.format != decoded.format()
                || input.channel_layout != source_layout
                || input.rate != decoded.rate()
                || self.output_format != Some(format)
        });
        if input_changed {
            self.resampler = Some(
                ffmpeg::software::resampling::Context::get(
                    decoded.format(),
                    source_layout,
                    decoded.rate(),
                    ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
                    target_layout,
                    format.sample_rate,
                )
                .map_err(|error| {
                    MediaError::external(format!("音声リサンプラーを構成できません: {error}"))
                })?,
            );
            self.output_format = Some(format);
        }
        let mut converted = ffmpeg::frame::Audio::empty();
        self.resampler
            .as_mut()
            .expect("the resampler was created above")
            .run(decoded, &mut converted)
            .map_err(|error| MediaError::external(format!("音声を変換できません: {error}")))?;
        let frame_start = decoded
            .timestamp()
            .map_or(self.decoded_cursor, |timestamp| {
                let start_time = if self.stream_start_time == ffmpeg::ffi::AV_NOPTS_VALUE {
                    0
                } else {
                    self.stream_start_time
                };
                (stream_seconds(timestamp, start_time, self.stream_time_base)
                    * f64::from(format.sample_rate))
                .round()
                .clamp(0., u64::MAX as f64) as u64
            });
        let sample_frames = converted.samples();
        self.decoded_cursor = frame_start.saturating_add(sample_frames as u64);
        let skip_frames = self
            .discard_before
            .map(|target| target.saturating_sub(frame_start) as usize)
            .unwrap_or(0)
            .min(sample_frames);
        if skip_frames == sample_frames {
            return Ok(());
        }
        self.discard_before = None;
        let channels = usize::from(format.channels);
        let samples = converted.plane::<f32>(0);
        self.pending
            .extend(samples[skip_frames * channels..].iter().copied());
        Ok(())
    }
}

impl AudioDecoderSession for FfmpegAudioDecoder {
    fn stream_duration(&self) -> Duration {
        self.stream_duration
    }

    fn decode_sample_frames(
        &mut self,
        start_seconds: f64,
        sample_frames: usize,
        format: AudioFormat,
    ) -> Result<DecodedAudioBlock, MediaError> {
        if !start_seconds.is_finite()
            || start_seconds < 0.
            || sample_frames == 0
            || format.sample_rate == 0
            || format.channels == 0
        {
            return Err(MediaError::external("音声サンプル要求が不正です"));
        }
        let latest_time = (self.stream_duration.as_secs_f64() - 0.000_001).max(0.);
        let start_seconds = start_seconds.clamp(0., latest_time);
        let start_sample_frame = (start_seconds * f64::from(format.sample_rate))
            .round()
            .clamp(0., u64::MAX as f64) as u64;
        if self.next_sample_frame != Some(start_sample_frame) || self.output_format != Some(format)
        {
            self.seek(start_sample_frame, format)?;
        }
        let channels = usize::from(format.channels);
        let requested_samples = sample_frames
            .checked_mul(channels)
            .ok_or_else(|| MediaError::external("音声ブロックが大きすぎます"))?;
        while self.pending.len() < requested_samples {
            let Some(decoded) = self.next_decoded_frame()? else {
                break;
            };
            self.append_frame(&decoded, format)?;
        }
        let returned_samples = requested_samples.min(self.pending.len());
        let returned_samples = returned_samples - returned_samples % channels;
        if returned_samples == 0 {
            return Err(MediaError::external(format!(
                "'{}'の音声を取得できません",
                self.asset.path.display()
            )));
        }
        let samples = self.pending.drain(..returned_samples).collect::<Vec<_>>();
        let returned_frames = returned_samples / channels;
        self.next_sample_frame = Some(start_sample_frame.saturating_add(returned_frames as u64));
        Ok(DecodedAudioBlock {
            format,
            samples: samples.into(),
        })
    }
}

impl FfmpegVideoDecoder {
    pub(super) fn open(asset: MediaAsset) -> Result<Self, MediaError> {
        let input = open_input(&asset.path)?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or_else(|| MediaError::external("映像ストリームが見つかりません"))?;
        let video_stream_index = stream.index();
        let stream_time_base = stream.time_base();
        let stream_start_time = stream.start_time();
        let stream_duration = stream_duration(&stream).unwrap_or(asset.duration);
        let mut context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|error| {
                MediaError::external(format!("映像デコーダーを構成できません: {error}"))
            })?;
        context.set_threading(ffmpeg::codec::threading::Config {
            kind: ffmpeg::codec::threading::Type::Frame,
            count: interactive_decode_threads(),
        });
        let decoder = context.decoder().video().map_err(|error| {
            MediaError::external(format!("映像デコーダーを開けません: {error}"))
        })?;

        Ok(Self {
            asset,
            input,
            decoder,
            video_stream_index,
            stream_time_base,
            stream_start_time,
            stream_duration,
            scaler: None,
            output_size: None,
            still_image: None,
            pending_decoded: None,
            last_frame: None,
            trail: VecDeque::new(),
            trail_bytes: 0,
            fallback_time: Duration::ZERO,
            draining: false,
        })
    }

    fn seek(&mut self, presentation_time: Duration) -> Result<(), MediaError> {
        let timestamp = seek_timestamp(
            self.stream_start_time,
            self.stream_time_base,
            presentation_time.as_secs_f64(),
        );
        self.input.seek(timestamp, ..timestamp).map_err(|error| {
            MediaError::external(format!(
                "'{}' の映像をシークできません: {error}",
                self.asset.path.display()
            ))
        })?;
        self.decoder.flush();
        self.pending_decoded = None;
        self.last_frame = None;
        self.fallback_time = presentation_time;
        self.draining = false;
        Ok(())
    }

    fn next_decoded_frame(&mut self) -> Result<Option<ffmpeg::frame::Video>, MediaError> {
        if let Some(frame) = self.pending_decoded.take() {
            return Ok(Some(frame));
        }
        loop {
            let mut frame = ffmpeg::frame::Video::empty();
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => return Ok(Some(frame)),
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {}
                Err(error) => {
                    return Err(MediaError::external(format!(
                        "'{}' の映像をデコードできません: {error}",
                        self.asset.path.display()
                    )));
                }
            }

            if let Some(packet) = self.next_video_packet() {
                self.decoder.send_packet(&packet).map_err(|error| {
                    MediaError::external(format!(
                        "'{}' の映像パケットをデコーダーへ送信できません: {error}",
                        self.asset.path.display()
                    ))
                })?;
            } else if self.draining {
                return Ok(None);
            } else {
                self.decoder.send_eof().map_err(|error| {
                    MediaError::external(format!(
                        "'{}' の映像デコーダーを完了できません: {error}",
                        self.asset.path.display()
                    ))
                })?;
                self.draining = true;
            }
        }
    }

    fn next_video_packet(&mut self) -> Option<ffmpeg::Packet> {
        self.input
            .packets()
            .find(|(stream, _)| stream.index() == self.video_stream_index)
            .map(|(_, packet)| packet)
    }

    fn frame_presentation_time(&self, frame: &ffmpeg::frame::Video) -> Option<Duration> {
        let timestamp = frame.timestamp()?;
        let start_time = if self.stream_start_time == ffmpeg::ffi::AV_NOPTS_VALUE {
            0
        } else {
            self.stream_start_time
        };
        let seconds = stream_seconds(timestamp, start_time, self.stream_time_base);
        Duration::try_from_secs_f64(seconds).ok()
    }

    fn default_frame_duration(&self) -> Duration {
        match self.asset.kind {
            MediaKind::Video { frame_rate, .. } => {
                Duration::try_from_secs_f64(1. / frame_rate.frames_per_second())
                    .unwrap_or(Duration::from_millis(33))
            }
            MediaKind::Image { .. } => self.asset.duration,
            MediaKind::Audio { .. } => Duration::from_millis(33),
        }
    }

    fn decoded_frame_duration(&self, frame: &ffmpeg::frame::Video) -> Duration {
        let packet_duration = frame.packet().duration;
        if packet_duration > 0 {
            Duration::try_from_secs_f64(packet_duration as f64 * f64::from(self.stream_time_base))
                .ok()
                .filter(|duration| !duration.is_zero())
                .unwrap_or_else(|| self.default_frame_duration())
        } else {
            self.default_frame_duration()
        }
    }

    fn rgba_frame(
        &mut self,
        decoded: &ffmpeg::frame::Video,
        width: u32,
        height: u32,
    ) -> Result<RgbaFrame, MediaError> {
        let input_changed = self.scaler.as_ref().is_none_or(|scaler| {
            let input = scaler.0.input();
            input.format != decoded.format()
                || input.width != decoded.width()
                || input.height != decoded.height()
        });
        if input_changed || self.output_size != Some((width, height)) {
            let scaler = ffmpeg::software::scaling::Context::get(
                decoded.format(),
                decoded.width(),
                decoded.height(),
                ffmpeg::format::Pixel::RGBA,
                width,
                height,
                ffmpeg::software::scaling::flag::Flags::BILINEAR,
            )
            .map_err(|error| {
                MediaError::external(format!("映像スケーラーを構成できません: {error}"))
            })?;
            self.scaler = Some(VideoScaler(scaler));
            self.output_size = Some((width, height));
        }

        let mut converted = ffmpeg::frame::Video::empty();
        self.scaler
            .as_mut()
            .expect("the scaler was created above")
            .0
            .run(decoded, &mut converted)
            .map_err(|error| {
                MediaError::external(format!("映像をRGBAへ変換できません: {error}"))
            })?;
        let width = usize::try_from(width)
            .map_err(|_| MediaError::external("映像フレームの幅が大きすぎます"))?;
        let height = usize::try_from(height)
            .map_err(|_| MediaError::external("映像フレームの高さが大きすぎます"))?;
        let row_bytes = width
            .checked_mul(4)
            .ok_or_else(|| MediaError::external("映像フレームの幅が大きすぎます"))?;
        let frame_bytes = row_bytes
            .checked_mul(height)
            .ok_or_else(|| MediaError::external("映像フレームのサイズが大きすぎます"))?;
        let source = converted.data(0);
        let stride = converted.stride(0);
        let mut rgba = vec![0; frame_bytes];
        for row in 0..height {
            let source_start = row
                .checked_mul(stride)
                .ok_or_else(|| MediaError::external("映像フレームが大きすぎます"))?;
            let target_start = row
                .checked_mul(row_bytes)
                .ok_or_else(|| MediaError::external("映像フレームが大きすぎます"))?;
            rgba[target_start..target_start + row_bytes]
                .copy_from_slice(&source[source_start..source_start + row_bytes]);
        }
        Ok(RgbaFrame {
            width: width as u32,
            height: height as u32,
            rgba: rgba.into(),
        })
    }
}

fn presentation_duration(
    presentation_time: Duration,
    next_presentation_time: Duration,
    fallback: Duration,
) -> Duration {
    next_presentation_time
        .checked_sub(presentation_time)
        .filter(|duration| !duration.is_zero())
        .unwrap_or(fallback)
}

impl VideoDecoderSession for FfmpegVideoDecoder {
    fn stream_duration(&self) -> Duration {
        self.stream_duration
    }

    fn decode_at(
        &mut self,
        presentation_time: Duration,
        size: VideoDecodeSize,
        cancelled: &AtomicBool,
    ) -> Result<DecodedVideoFrame, MediaError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        if size.max_width == 0 || size.max_height == 0 {
            return Err(MediaError::external("映像フレーム要求が不正です"));
        }
        let (source_width, source_height, is_image) = match self.asset.kind {
            MediaKind::Video { width, height, .. } => (width, height, false),
            MediaKind::Image { width, height } => (width, height, true),
            MediaKind::Audio { .. } => {
                return Err(MediaError::external(
                    "音声素材から映像フレームは取得できません",
                ));
            }
        };
        let (width, height) =
            fit_dimensions(source_width, source_height, size.max_width, size.max_height)
                .ok_or_else(|| MediaError::external("映像サイズが不正です"))?;

        if let Some(frame) = &self.last_frame
            && self.output_size == Some((width, height))
        {
            let end = frame
                .presentation_time
                .checked_add(frame.duration)
                .unwrap_or(Duration::MAX);
            if frame.presentation_time <= presentation_time && presentation_time < end {
                return Ok(frame.clone());
            }
        }
        if let Some(frame) = self.trail_frame(presentation_time, width, height) {
            return Ok(frame);
        }
        if is_image && let Some(decoded) = self.still_image.take() {
            let frame = DecodedVideoFrame {
                presentation_time: Duration::ZERO,
                duration: self.asset.duration,
                frame: self.rgba_frame(&decoded, width, height)?,
            };
            self.still_image = Some(decoded);
            self.last_frame = Some(frame.clone());
            return Ok(frame);
        }

        let can_continue = self.output_size == Some((width, height))
            && self
                .last_frame
                .as_ref()
                .is_some_and(|frame| frame.presentation_time <= presentation_time);
        let fresh_at_stream_start = presentation_time.is_zero()
            && self.last_frame.is_none()
            && self.pending_decoded.is_none()
            && self.fallback_time.is_zero()
            && !self.draining;
        if !can_continue && !fresh_at_stream_start {
            self.seek(presentation_time)?;
        }

        let mut candidate: Option<SeekCandidate> = self
            .last_frame
            .take()
            .filter(|frame| frame.presentation_time <= presentation_time)
            .map(SeekCandidate::Ready);
        loop {
            if cancelled.load(Ordering::Relaxed) {
                self.cache_candidate(candidate, width, height);
                return Err(MediaError::Cancelled);
            }
            let Some(decoded) = self.next_decoded_frame()? else {
                break;
            };
            if cancelled.load(Ordering::Relaxed) {
                self.pending_decoded = Some(decoded);
                self.cache_candidate(candidate, width, height);
                return Err(MediaError::Cancelled);
            }
            let frame_time = self
                .frame_presentation_time(&decoded)
                .unwrap_or(self.fallback_time);
            let fallback_duration = self.decoded_frame_duration(&decoded);
            self.fallback_time = frame_time
                .checked_add(fallback_duration)
                .unwrap_or(Duration::MAX);

            if frame_time > presentation_time {
                if let Some(candidate) = candidate {
                    let mut frame = match candidate {
                        SeekCandidate::Ready(frame) => frame,
                        SeekCandidate::Raw {
                            decoded: raw,
                            presentation_time,
                            duration,
                        } => self.convert_candidate(
                            &raw,
                            presentation_time,
                            duration,
                            width,
                            height,
                        )?,
                    };
                    frame.duration = frame_time
                        .checked_sub(frame.presentation_time)
                        .filter(|duration| !duration.is_zero())
                        .unwrap_or(frame.duration);
                    // The deferred frame has not consumed its fallback timestamp yet.
                    self.fallback_time = frame_time;
                    self.pending_decoded = Some(decoded);
                    self.last_frame = Some(frame.clone());
                    return Ok(frame);
                }
                let frame = DecodedVideoFrame {
                    presentation_time: frame_time,
                    duration: fallback_duration,
                    frame: self.rgba_frame(&decoded, width, height)?,
                };
                self.last_frame = Some(frame.clone());
                return Ok(frame);
            }

            if is_image {
                let frame = DecodedVideoFrame {
                    presentation_time: frame_time,
                    duration: fallback_duration,
                    frame: self.rgba_frame(&decoded, width, height)?,
                };
                self.still_image = Some(decoded);
                candidate = Some(SeekCandidate::Ready(frame));
                continue;
            }
            if let Some(SeekCandidate::Raw {
                decoded: prev,
                presentation_time: prev_time,
                duration: prev_duration,
            }) = candidate.replace(SeekCandidate::Raw {
                decoded,
                presentation_time: frame_time,
                duration: fallback_duration,
            }) {
                self.push_trail(prev, prev_time, prev_duration);
            }
        }

        match candidate {
            Some(SeekCandidate::Ready(frame)) => {
                self.last_frame = Some(frame.clone());
                return Ok(frame);
            }
            Some(SeekCandidate::Raw {
                decoded,
                presentation_time,
                duration,
            }) => {
                let frame =
                    self.convert_candidate(&decoded, presentation_time, duration, width, height)?;
                self.last_frame = Some(frame.clone());
                return Ok(frame);
            }
            None => {}
        }
        Err(MediaError::external(format!(
            "'{}' の時刻 {:.6} 秒の映像フレームを取得できません",
            self.asset.path.display(),
            presentation_time.as_secs_f64()
        )))
    }

    fn decode_from(
        &mut self,
        presentation_time: Duration,
        frame_count: usize,
        size: VideoDecodeSize,
        cancelled: &AtomicBool,
    ) -> Result<Vec<DecodedVideoFrame>, MediaError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        if frame_count == 0 {
            return Err(MediaError::external("映像フレーム要求が不正です"));
        }
        let first = self.decode_at(presentation_time, size, cancelled)?;
        if matches!(self.asset.kind, MediaKind::Image { .. }) || frame_count == 1 {
            return Ok(vec![first]);
        }
        let (source_width, source_height) = self
            .asset
            .kind
            .dimensions()
            .map(|[width, height]| (width, height))
            .ok_or_else(|| MediaError::external("映像サイズが不正です"))?;
        let (width, height) =
            fit_dimensions(source_width, source_height, size.max_width, size.max_height)
                .ok_or_else(|| MediaError::external("映像サイズが不正です"))?;
        let mut frames = Vec::with_capacity(frame_count);
        frames.push(first);
        while frames.len() < frame_count {
            if cancelled.load(Ordering::Relaxed) {
                self.last_frame = frames.last().cloned();
                return Err(MediaError::Cancelled);
            }
            let Some(decoded) = self.next_decoded_frame()? else {
                break;
            };
            if cancelled.load(Ordering::Relaxed) {
                self.pending_decoded = Some(decoded);
                self.last_frame = frames.last().cloned();
                return Err(MediaError::Cancelled);
            }
            let frame_time = self
                .frame_presentation_time(&decoded)
                .unwrap_or(self.fallback_time);
            if let Some(previous) = frames.last_mut() {
                previous.duration = presentation_duration(
                    previous.presentation_time,
                    frame_time,
                    previous.duration,
                );
            }
            let duration = self.decoded_frame_duration(&decoded);
            self.fallback_time = frame_time.checked_add(duration).unwrap_or(Duration::MAX);
            frames.push(DecodedVideoFrame {
                presentation_time: frame_time,
                duration,
                frame: self.rgba_frame(&decoded, width, height)?,
            });
        }

        self.last_frame = frames.last().cloned();
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        if frames.len() == frame_count {
            if let Some(next) = self.next_decoded_frame()? {
                let next_time = self
                    .frame_presentation_time(&next)
                    .unwrap_or(self.fallback_time);
                if let Some(previous) = frames.last_mut() {
                    previous.duration = presentation_duration(
                        previous.presentation_time,
                        next_time,
                        previous.duration,
                    );
                }
                self.pending_decoded = Some(next);
            }
        } else if let Some(last) = frames.last_mut() {
            last.duration = self
                .stream_duration
                .checked_sub(last.presentation_time)
                .filter(|duration| !duration.is_zero())
                .unwrap_or(last.duration);
        }
        self.last_frame = frames.last().cloned();
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        Ok(frames)
    }
}

pub(super) fn initialize_ffmpeg() -> Result<(), MediaError> {
    static INITIALIZATION: OnceLock<Result<(), String>> = OnceLock::new();
    INITIALIZATION
        .get_or_init(|| {
            ffmpeg::init().map_err(|error| error.to_string())?;
            ffmpeg::log::set_level(ffmpeg::log::Level::Error);
            Ok(())
        })
        .as_ref()
        .map(|_| ())
        .map_err(|error| MediaError::external(format!("FFmpegを初期化できません: {error}")))
}
