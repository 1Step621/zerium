use std::path::{Path, PathBuf};

use ffmpeg_next as ffmpeg;
use thiserror::Error;

use crate::{domain::media::VideoFrameRate, engine::frame::RgbaFrame};

use super::reader::AudioFormat;

const VIDEO_PIXEL_FORMAT: ffmpeg::format::Pixel = ffmpeg::format::Pixel::YUV420P;
const INPUT_AUDIO_FORMAT: ffmpeg::format::Sample =
    ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VideoColorSpec {
    pub(crate) space: ffmpeg::util::color::Space,
    pub(crate) range: ffmpeg::util::color::Range,
    pub(crate) primaries: ffmpeg::util::color::Primaries,
    pub(crate) transfer: ffmpeg::util::color::TransferCharacteristic,
}

impl VideoColorSpec {
    pub(crate) const BT709_LIMITED: Self = Self {
        space: ffmpeg::util::color::Space::BT709,
        range: ffmpeg::util::color::Range::MPEG,
        primaries: ffmpeg::util::color::Primaries::BT709,
        transfer: ffmpeg::util::color::TransferCharacteristic::BT709,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VideoOutputSpec {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) color: VideoColorSpec,
}

impl VideoOutputSpec {
    pub(crate) fn validate(self) -> Result<Self, FfmpegEncoderError> {
        if self.width == 0 || self.height == 0 {
            return Err(FfmpegEncoderError::NonPositiveDimensions);
        }
        if !self.width.is_multiple_of(2) || !self.height.is_multiple_of(2) {
            return Err(FfmpegEncoderError::OddOutputDimensions {
                width: self.width,
                height: self.height,
            });
        }
        Ok(self)
    }
}

pub(crate) struct VideoEncoderSettings<'a> {
    pub(crate) output: VideoOutputSpec,
    pub(crate) frame_rate: VideoFrameRate,
    pub(crate) preset: &'a str,
    pub(crate) crf: u8,
    pub(crate) gop: u32,
    pub(crate) audio: Option<AudioFormat>,
    pub(crate) container: Option<&'a str>,
    pub(crate) fast_start: bool,
}

#[derive(Debug, Error)]
pub(crate) enum FfmpegEncoderError {
    #[error(transparent)]
    Initialize(#[from] super::reader::MediaError),
    #[error("出力ファイル'{path}'を開けません: {source}")]
    OpenOutput {
        path: PathBuf,
        source: ffmpeg::Error,
    },
    #[error("H.264エンコーダーが見つかりません")]
    MissingVideoCodec,
    #[error("映像のタイムベースが大きすぎます")]
    VideoTimeBaseTooLarge,
    #[error("H.264エンコーダーを構成できません: {0}")]
    ConfigureVideo(ffmpeg::Error),
    #[error("H.264エンコーダーを開けません: {0}")]
    OpenVideo(ffmpeg::Error),
    #[error("映像ストリームを作成できません: {0}")]
    CreateVideoStream(ffmpeg::Error),
    #[error("出力ヘッダーを書き込めません: {0}")]
    WriteHeader(ffmpeg::Error),
    #[error("映像エンコード用スケーラーを構成できません: {0}")]
    CreateScaler(ffmpeg::Error),
    #[error("未対応のRGB→YUV行列です: {0:?}")]
    UnsupportedColorMatrix(ffmpeg::util::color::Space),
    #[error("FFmpegから色変換係数を取得できません")]
    MissingColorCoefficients,
    #[error("RGB→YUV色変換を構成できません: FFmpeg error {code}")]
    ConfigureColorSpace { code: i32 },
    #[error("エンコードする映像サイズは正である必要があります")]
    NonPositiveDimensions,
    #[error(
        "YUV420P出力には偶数解像度が必要です（指定: {width}x{height}）。出力設定でpad/cropするか偶数解像度を指定してください"
    )]
    OddOutputDimensions { width: u32, height: u32 },
    #[error("音声エンコード設定が不正です")]
    InvalidAudioConfig,
    #[error("AACエンコーダーが見つかりません")]
    MissingAudioCodec,
    #[error("AACエンコーダーを取得できません: {0}")]
    AcquireAudioCodec(ffmpeg::Error),
    #[error("AACエンコーダーのサンプル形式を取得できません")]
    MissingAudioSampleFormat,
    #[error("音声サンプルレートが大きすぎます")]
    AudioSampleRateTooLarge,
    #[error("AACエンコーダーを構成できません: {0}")]
    ConfigureAudio(ffmpeg::Error),
    #[error("AACエンコーダーを開けません: {0}")]
    OpenAudio(ffmpeg::Error),
    #[error("音声ストリームを作成できません: {0}")]
    CreateAudioStream(ffmpeg::Error),
    #[error("音声エンコード用リサンプラーを構成できません: {0}")]
    CreateResampler(ffmpeg::Error),
    #[error(
        "映像フレームのサイズ{frame_width}x{frame_height}が出力サイズ{output_width}x{output_height}と一致しません"
    )]
    FrameSizeMismatch {
        frame_width: u32,
        frame_height: u32,
        output_width: u32,
        output_height: u32,
    },
    #[error("映像フレームの幅が大きすぎます")]
    FrameTooWide,
    #[error("映像フレームの高さが大きすぎます")]
    FrameTooTall,
    #[error("映像フレームが大きすぎます")]
    FrameTooLarge,
    #[error("映像フレームのデータ長が不正です")]
    InvalidFrameData,
    #[error("映像フレームのストライドが不正です")]
    InvalidFrameStride,
    #[error("YUV420P入力には偶数解像度が必要です")]
    OddYuvDimensions,
    #[error("映像PTSが大きすぎます")]
    VideoPtsTooLarge,
    #[error("映像フレームをYUVへ変換できません: {0}")]
    ConvertToYuv(ffmpeg::Error),
    #[error("映像フレームをエンコーダーへ送れません: {0}")]
    SendVideoFrame(ffmpeg::Error),
    #[error("音声ストリームのない出力へ音声が渡されました")]
    AudioWithoutStream,
    #[error("音声サンプル数がチャンネル数で割り切れません")]
    InvalidSampleCount,
    #[error("音声サンプル数が大きすぎます")]
    TooManySamples,
    #[error("音声PTSが大きすぎます")]
    AudioPtsTooLarge,
    #[error("音声エンコーダーフレームが大きすぎます")]
    AudioFrameTooLarge,
    #[error("音声サンプルを変換できません: {0}")]
    ResampleAudio(ffmpeg::Error),
    #[error("音声フレームをエンコーダーへ送れません: {0}")]
    SendAudioFrame(ffmpeg::Error),
    #[error("出力映像ストリームが失われました")]
    MissingVideoStream,
    #[error("映像パケットを書き込めません: {0}")]
    WriteVideoPacket(ffmpeg::Error),
    #[error("映像パケットを取得できません: {0}")]
    ReceiveVideoPacket(ffmpeg::Error),
    #[error("出力音声ストリームが失われました")]
    MissingAudioStream,
    #[error("音声パケットを書き込めません: {0}")]
    WriteAudioPacket(ffmpeg::Error),
    #[error("音声パケットを取得できません: {0}")]
    ReceiveAudioPacket(ffmpeg::Error),
    #[error("映像エンコーダーを完了できません: {0}")]
    FinishVideo(ffmpeg::Error),
    #[error("音声エンコーダーを完了できません: {0}")]
    FinishAudio(ffmpeg::Error),
    #[error("出力トレーラーを書き込めません: {0}")]
    WriteTrailer(ffmpeg::Error),
}

pub(crate) struct FfmpegFileEncoder {
    output: ffmpeg::format::context::Output,
    video: ffmpeg::encoder::Video,
    video_stream: usize,
    video_time_base: ffmpeg::Rational,
    scaler: ffmpeg::software::scaling::Context,
    audio: Option<AudioEncoder>,
    width: u32,
    height: u32,
    color: VideoColorSpec,
}

fn configure_scaler_color(
    scaler: &mut ffmpeg::software::scaling::Context,
    color: VideoColorSpec,
) -> Result<(), FfmpegEncoderError> {
    let matrix = match color.space {
        ffmpeg::util::color::Space::BT709 => ffmpeg::ffi::SWS_CS_ITU709,
        _ => {
            return Err(FfmpegEncoderError::UnsupportedColorMatrix(color.space));
        }
    };
    let full_range = i32::from(color.range == ffmpeg::util::color::Range::JPEG);
    let coefficients = unsafe { ffmpeg::ffi::sws_getCoefficients(matrix) };
    if coefficients.is_null() {
        return Err(FfmpegEncoderError::MissingColorCoefficients);
    }
    let result = unsafe {
        ffmpeg::ffi::sws_setColorspaceDetails(
            scaler.as_mut_ptr(),
            coefficients,
            1,
            coefficients,
            full_range,
            0,
            1 << 16,
            1 << 16,
        )
    };
    if result < 0 {
        return Err(FfmpegEncoderError::ConfigureColorSpace { code: result });
    }
    Ok(())
}

fn copy_plane_to_frame(
    frame: &mut ffmpeg::frame::Video,
    plane: usize,
    source: &[u8],
    width: usize,
    height: usize,
) -> Result<(), FfmpegEncoderError> {
    let stride = frame.stride(plane);
    let expected = width
        .checked_mul(height)
        .ok_or(FfmpegEncoderError::FrameTooLarge)?;
    if source.len() != expected {
        return Err(FfmpegEncoderError::InvalidFrameData);
    }
    if stride < width {
        return Err(FfmpegEncoderError::InvalidFrameStride);
    }
    let target = frame.data_mut(plane);
    for row in 0..height {
        target[row * stride..row * stride + width]
            .copy_from_slice(&source[row * width..(row + 1) * width]);
    }
    Ok(())
}

struct AudioEncoder {
    encoder: ffmpeg::encoder::Audio,
    stream: usize,
    time_base: ffmpeg::Rational,
    resampler: ffmpeg::software::resampling::Context,
    format: AudioFormat,
    frame_size: usize,
    pending: Vec<f32>,
    pending_start: usize,
    next_pts: i64,
    input_sample_frames: i64,
}

impl FfmpegFileEncoder {
    pub(crate) fn create(
        path: &Path,
        settings: VideoEncoderSettings<'_>,
    ) -> Result<Self, FfmpegEncoderError> {
        super::ffmpeg_next::initialize_ffmpeg()?;
        let output_spec = settings.output.validate()?;
        let mut output = match settings.container {
            Some(container) => ffmpeg::format::output_as(path, container),
            None => ffmpeg::format::output(path),
        }
        .map_err(|source| FfmpegEncoderError::OpenOutput {
            path: path.to_path_buf(),
            source,
        })?;
        let global_header = output
            .format()
            .flags()
            .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
        let video_codec = ffmpeg::encoder::find(ffmpeg::codec::Id::H264)
            .ok_or(FfmpegEncoderError::MissingVideoCodec)?;
        let video_time_base = ffmpeg::Rational(
            i32::try_from(settings.frame_rate.denominator())
                .map_err(|_| FfmpegEncoderError::VideoTimeBaseTooLarge)?,
            i32::try_from(settings.frame_rate.numerator())
                .map_err(|_| FfmpegEncoderError::VideoTimeBaseTooLarge)?,
        );
        let mut video = ffmpeg::codec::context::Context::new_with_codec(video_codec)
            .encoder()
            .video()
            .map_err(FfmpegEncoderError::ConfigureVideo)?;
        video.set_width(output_spec.width);
        video.set_height(output_spec.height);
        video.set_format(VIDEO_PIXEL_FORMAT);
        video.set_time_base(video_time_base);
        video.set_frame_rate(Some((
            settings.frame_rate.numerator() as i32,
            settings.frame_rate.denominator() as i32,
        )));
        video.set_gop(settings.gop.max(1));
        video.set_colorspace(output_spec.color.space);
        video.set_color_range(output_spec.color.range);
        video.set_color_primaries(output_spec.color.primaries);
        video.set_color_transfer_characteristic(output_spec.color.transfer);
        if global_header {
            video.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
        }
        let mut options = ffmpeg::Dictionary::new();
        options.set("preset", settings.preset);
        options.set("crf", &settings.crf.to_string());
        let video = video
            .open_as_with(video_codec, options)
            .map_err(FfmpegEncoderError::OpenVideo)?;
        let video_stream = {
            let mut stream = output
                .add_stream(video_codec)
                .map_err(FfmpegEncoderError::CreateVideoStream)?;
            stream.set_time_base(video_time_base);
            stream.set_rate((
                settings.frame_rate.numerator() as i32,
                settings.frame_rate.denominator() as i32,
            ));
            stream.set_parameters(&video);
            stream.index()
        };

        let audio = settings
            .audio
            .map(|format| Self::create_audio_encoder(&mut output, format, global_header))
            .transpose()?;
        if settings.fast_start {
            let mut options = ffmpeg::Dictionary::new();
            options.set("movflags", "+faststart");
            output
                .write_header_with(options)
                .map_err(FfmpegEncoderError::WriteHeader)?;
        } else {
            output
                .write_header()
                .map_err(FfmpegEncoderError::WriteHeader)?;
        }
        let mut scaler = ffmpeg::software::scaling::Context::get(
            ffmpeg::format::Pixel::RGBA,
            output_spec.width,
            output_spec.height,
            VIDEO_PIXEL_FORMAT,
            output_spec.width,
            output_spec.height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )
        .map_err(FfmpegEncoderError::CreateScaler)?;
        configure_scaler_color(&mut scaler, output_spec.color)?;
        Ok(Self {
            output,
            video,
            video_stream,
            video_time_base,
            scaler,
            audio,
            width: output_spec.width,
            height: output_spec.height,
            color: output_spec.color,
        })
    }

    fn create_audio_encoder(
        output: &mut ffmpeg::format::context::Output,
        format: AudioFormat,
        global_header: bool,
    ) -> Result<AudioEncoder, FfmpegEncoderError> {
        if format.sample_rate == 0 || format.channels == 0 {
            return Err(FfmpegEncoderError::InvalidAudioConfig);
        }
        let codec = ffmpeg::encoder::find(ffmpeg::codec::Id::AAC)
            .ok_or(FfmpegEncoderError::MissingAudioCodec)?;
        let layout = ffmpeg::ChannelLayout::default(i32::from(format.channels));
        let sample_format = codec
            .audio()
            .map_err(FfmpegEncoderError::AcquireAudioCodec)?
            .formats()
            .and_then(|mut formats| formats.next())
            .ok_or(FfmpegEncoderError::MissingAudioSampleFormat)?;
        let time_base = ffmpeg::Rational(
            1,
            i32::try_from(format.sample_rate)
                .map_err(|_| FfmpegEncoderError::AudioSampleRateTooLarge)?,
        );
        let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
            .encoder()
            .audio()
            .map_err(FfmpegEncoderError::ConfigureAudio)?;
        encoder.set_rate(format.sample_rate as i32);
        encoder.set_channel_layout(layout);
        encoder.set_format(sample_format);
        encoder.set_bit_rate(192_000);
        encoder.set_time_base(time_base);
        if global_header {
            encoder.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
        }
        let encoder = encoder
            .open_as(codec)
            .map_err(FfmpegEncoderError::OpenAudio)?;
        let stream = {
            let mut stream = output
                .add_stream(codec)
                .map_err(FfmpegEncoderError::CreateAudioStream)?;
            stream.set_time_base(time_base);
            stream.set_parameters(&encoder);
            stream.index()
        };
        let resampler = ffmpeg::software::resampling::Context::get(
            INPUT_AUDIO_FORMAT,
            layout,
            format.sample_rate,
            sample_format,
            layout,
            format.sample_rate,
        )
        .map_err(FfmpegEncoderError::CreateResampler)?;
        let frame_size = usize::try_from(encoder.frame_size()).unwrap_or(0).max(1);
        Ok(AudioEncoder {
            encoder,
            stream,
            time_base,
            resampler,
            format,
            frame_size,
            pending: Vec::new(),
            pending_start: 0,
            next_pts: 0,
            input_sample_frames: 0,
        })
    }

    pub(crate) fn encode_video(
        &mut self,
        frame: &RgbaFrame,
        pts: u64,
    ) -> Result<(), FfmpegEncoderError> {
        if frame.width != self.width || frame.height != self.height {
            return Err(FfmpegEncoderError::FrameSizeMismatch {
                frame_width: frame.width,
                frame_height: frame.height,
                output_width: self.width,
                output_height: self.height,
            });
        }
        let row_bytes = usize::try_from(self.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or(FfmpegEncoderError::FrameTooWide)?;
        let expected = row_bytes
            .checked_mul(self.height as usize)
            .ok_or(FfmpegEncoderError::FrameTooLarge)?;
        if frame.rgba.len() != expected {
            return Err(FfmpegEncoderError::InvalidFrameData);
        }
        let mut rgba =
            ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, self.width, self.height);
        let stride = rgba.stride(0);
        for row in 0..self.height as usize {
            let source = &frame.rgba[row * row_bytes..(row + 1) * row_bytes];
            let target = &mut rgba.data_mut(0)[row * stride..row * stride + row_bytes];
            target.copy_from_slice(source);
        }
        rgba.set_pts(Some(
            i64::try_from(pts).map_err(|_| FfmpegEncoderError::VideoPtsTooLarge)?,
        ));
        rgba.set_color_space(ffmpeg::util::color::Space::RGB);
        rgba.set_color_range(ffmpeg::util::color::Range::JPEG);
        rgba.set_color_primaries(self.color.primaries);
        rgba.set_color_transfer_characteristic(self.color.transfer);
        let mut yuv = ffmpeg::frame::Video::empty();
        self.scaler
            .run(&rgba, &mut yuv)
            .map_err(FfmpegEncoderError::ConvertToYuv)?;
        yuv.set_pts(rgba.pts());
        self.submit_yuv_frame(&mut yuv)
    }

    /// Encodes one tightly packed YUV420P frame (Y, then U, then V) produced by
    /// the GPU export pipeline. Unlike [`Self::encode_video`], this skips the
    /// software RGBA to YUV conversion.
    pub(crate) fn encode_yuv420p(
        &mut self,
        yuv: &[u8],
        pts: u64,
    ) -> Result<(), FfmpegEncoderError> {
        let width = usize::try_from(self.width).map_err(|_| FfmpegEncoderError::FrameTooWide)?;
        let height = usize::try_from(self.height).map_err(|_| FfmpegEncoderError::FrameTooTall)?;
        let y_len = width
            .checked_mul(height)
            .ok_or(FfmpegEncoderError::FrameTooLarge)?;
        if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(FfmpegEncoderError::OddYuvDimensions);
        }
        let uv_len = y_len
            .checked_div(4)
            .ok_or(FfmpegEncoderError::FrameTooLarge)?;
        let expected = y_len
            .checked_add(uv_len)
            .and_then(|size| size.checked_add(uv_len))
            .ok_or(FfmpegEncoderError::FrameTooLarge)?;
        if yuv.len() != expected {
            return Err(FfmpegEncoderError::InvalidFrameData);
        }
        let mut frame = ffmpeg::frame::Video::new(VIDEO_PIXEL_FORMAT, self.width, self.height);
        let (y, uv) = yuv.split_at(y_len);
        let (u, v) = uv.split_at(uv_len);
        copy_plane_to_frame(&mut frame, 0, y, width, height)?;
        copy_plane_to_frame(&mut frame, 1, u, width / 2, height / 2)?;
        copy_plane_to_frame(&mut frame, 2, v, width / 2, height / 2)?;
        frame.set_pts(Some(
            i64::try_from(pts).map_err(|_| FfmpegEncoderError::VideoPtsTooLarge)?,
        ));
        self.submit_yuv_frame(&mut frame)
    }

    fn submit_yuv_frame(
        &mut self,
        yuv: &mut ffmpeg::frame::Video,
    ) -> Result<(), FfmpegEncoderError> {
        yuv.set_color_space(self.color.space);
        yuv.set_color_range(self.color.range);
        yuv.set_color_primaries(self.color.primaries);
        yuv.set_color_transfer_characteristic(self.color.transfer);
        self.video
            .send_frame(yuv)
            .map_err(FfmpegEncoderError::SendVideoFrame)?;
        self.write_video_packets()
    }

    pub(crate) fn encode_audio(&mut self, samples: &[f32]) -> Result<(), FfmpegEncoderError> {
        let Some(audio) = &mut self.audio else {
            return if samples.is_empty() {
                Ok(())
            } else {
                Err(FfmpegEncoderError::AudioWithoutStream)
            };
        };
        if !samples
            .len()
            .is_multiple_of(usize::from(audio.format.channels))
        {
            return Err(FfmpegEncoderError::InvalidSampleCount);
        }
        let sample_frames = samples.len() / usize::from(audio.format.channels);
        audio.input_sample_frames = audio
            .input_sample_frames
            .checked_add(
                i64::try_from(sample_frames).map_err(|_| FfmpegEncoderError::TooManySamples)?,
            )
            .ok_or(FfmpegEncoderError::AudioPtsTooLarge)?;
        audio.pending.extend_from_slice(samples);
        Self::write_complete_audio_frames(&mut self.output, audio)
    }

    fn write_complete_audio_frames(
        output: &mut ffmpeg::format::context::Output,
        audio: &mut AudioEncoder,
    ) -> Result<(), FfmpegEncoderError> {
        let frame_samples = audio
            .frame_size
            .checked_mul(usize::from(audio.format.channels))
            .ok_or(FfmpegEncoderError::AudioFrameTooLarge)?;
        while audio.pending.len().saturating_sub(audio.pending_start) >= frame_samples {
            let end = audio.pending_start + frame_samples;
            let frame = audio.pending[audio.pending_start..end].to_vec();
            audio.pending_start = end;
            Self::write_audio_frame(output, audio, &frame, None)?;
        }
        if audio.pending_start > 16_384 && audio.pending_start * 2 >= audio.pending.len() {
            audio.pending.drain(..audio.pending_start);
            audio.pending_start = 0;
        }
        Ok(())
    }

    fn write_audio_frame(
        output: &mut ffmpeg::format::context::Output,
        audio: &mut AudioEncoder,
        samples: &[f32],
        presentation_end: Option<i64>,
    ) -> Result<(), FfmpegEncoderError> {
        let channels = usize::from(audio.format.channels);
        let sample_frames = samples.len() / channels;
        let layout = ffmpeg::ChannelLayout::default(i32::from(audio.format.channels));
        let mut input = ffmpeg::frame::Audio::new(INPUT_AUDIO_FORMAT, sample_frames, layout);
        input.set_rate(audio.format.sample_rate);
        input.set_pts(Some(audio.next_pts));
        input.plane_mut::<f32>(0).copy_from_slice(samples);
        let mut converted = ffmpeg::frame::Audio::empty();
        audio
            .resampler
            .run(&input, &mut converted)
            .map_err(FfmpegEncoderError::ResampleAudio)?;
        converted.set_pts(Some(audio.next_pts));
        audio.next_pts = audio.next_pts.saturating_add(sample_frames as i64);
        audio
            .encoder
            .send_frame(&converted)
            .map_err(FfmpegEncoderError::SendAudioFrame)?;
        Self::write_audio_packets(output, audio, presentation_end)
    }

    fn write_video_packets(&mut self) -> Result<(), FfmpegEncoderError> {
        let output_time_base = self
            .output
            .stream(self.video_stream)
            .ok_or(FfmpegEncoderError::MissingVideoStream)?
            .time_base();
        let mut packet = ffmpeg::Packet::empty();
        loop {
            match self.video.receive_packet(&mut packet) {
                Ok(()) => {
                    packet.set_stream(self.video_stream);
                    packet.rescale_ts(self.video_time_base, output_time_base);
                    packet.set_position(-1);
                    packet
                        .write_interleaved(&mut self.output)
                        .map_err(FfmpegEncoderError::WriteVideoPacket)?;
                }
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => break,
                Err(ffmpeg::Error::Eof) => break,
                Err(error) => {
                    return Err(FfmpegEncoderError::ReceiveVideoPacket(error));
                }
            }
        }
        Ok(())
    }

    fn write_audio_packets(
        output: &mut ffmpeg::format::context::Output,
        audio: &mut AudioEncoder,
        presentation_end: Option<i64>,
    ) -> Result<(), FfmpegEncoderError> {
        let output_time_base = output
            .stream(audio.stream)
            .ok_or(FfmpegEncoderError::MissingAudioStream)?
            .time_base();
        let mut packet = ffmpeg::Packet::empty();
        loop {
            match audio.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    if let Some(end) = presentation_end {
                        let pts = packet.pts().unwrap_or(0);
                        if pts >= end {
                            continue;
                        }
                        let remaining = end.saturating_sub(pts);
                        let encoded_frame_duration = i64::try_from(audio.frame_size)
                            .unwrap_or(i64::MAX)
                            .min(remaining);
                        let duration = packet.duration();
                        packet.set_duration(if duration > 0 {
                            duration.min(remaining)
                        } else {
                            encoded_frame_duration
                        });
                    }
                    packet.set_stream(audio.stream);
                    packet.rescale_ts(audio.time_base, output_time_base);
                    packet.set_position(-1);
                    packet
                        .write_interleaved(output)
                        .map_err(FfmpegEncoderError::WriteAudioPacket)?;
                }
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => break,
                Err(ffmpeg::Error::Eof) => break,
                Err(error) => {
                    return Err(FfmpegEncoderError::ReceiveAudioPacket(error));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<(), FfmpegEncoderError> {
        if let Some(audio) = &mut self.audio {
            let channels = usize::from(audio.format.channels);
            let frame_samples = audio.frame_size.saturating_mul(channels);
            let remaining = audio.pending.len().saturating_sub(audio.pending_start);
            if remaining > 0 {
                let mut final_frame = audio.pending[audio.pending_start..].to_vec();
                final_frame.resize(frame_samples, 0.);
                let presentation_end = audio.input_sample_frames;
                Self::write_audio_frame(
                    &mut self.output,
                    audio,
                    &final_frame,
                    Some(presentation_end),
                )?;
            }
        }
        self.video
            .send_eof()
            .map_err(FfmpegEncoderError::FinishVideo)?;
        self.write_video_packets()?;
        if let Some(audio) = &mut self.audio {
            audio
                .encoder
                .send_eof()
                .map_err(FfmpegEncoderError::FinishAudio)?;
            let presentation_end = audio.input_sample_frames;
            Self::write_audio_packets(&mut self.output, audio, Some(presentation_end))?;
        }
        self.output
            .write_trailer()
            .map_err(FfmpegEncoderError::WriteTrailer)?;
        Ok(())
    }
}
