use std::{collections::VecDeque, num::NonZeroU32, sync::mpsc};

use gpu_video::{
    VideoDeviceExt,
    parameters::{
        ColorRange, ColorSpace, EncoderParametersH264, EncoderParametersH265, RateControl,
        VideoParameters,
    },
};

use zerium_core::timeline::{ExportColorRange, VideoCodec};

use crate::engine::{
    output::visual::{ColorConversion, EncodingError, EncodingSettings, VideoEncoderFrame},
    processing::visual::{RenderScene, RenderSession},
};

pub(in crate::engine::output::visual) struct Encoder {
    // Callback delivery must never block the library's shared GPU waiter thread.
    // The outer pipeline limits outstanding frames to `depth`.
    receiver: mpsc::Receiver<gpu_video::EncodedOutputChunk<Vec<u8>>>,
    encoder: NativeEncoder,
    headers: Vec<u8>,
    pending: VecDeque<u64>,
}

enum NativeEncoder {
    H264(gpu_video::WgpuTexturesEncoderH264),
    H265(gpu_video::WgpuTexturesEncoderH265),
}

impl NativeEncoder {
    fn input_texture(&mut self) -> Result<gpu_video::EncodeTexture, gpu_video::VideoEncoderError> {
        match self {
            Self::H264(encoder) => encoder.input_texture(),
            Self::H265(encoder) => encoder.input_texture(),
        }
    }

    fn encode(
        &mut self,
        frame: gpu_video::InputFrame<gpu_video::EncodeTexture>,
    ) -> Result<(), gpu_video::VideoEncoderError> {
        match self {
            Self::H264(encoder) => encoder.encode(frame, false),
            Self::H265(encoder) => encoder.encode(frame, false),
        }
    }

    fn flush(&mut self) -> Result<(), gpu_video::VideoEncoderError> {
        match self {
            Self::H264(encoder) => encoder.flush(),
            Self::H265(encoder) => encoder.flush(),
        }
    }
}

impl Encoder {
    pub(in crate::engine::output::visual) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        settings: EncodingSettings,
        depth: usize,
    ) -> Result<Option<Self>, EncodingError> {
        let Ok(video) = device.video() else {
            return Ok(None);
        };
        let capabilities = video.encode_capabilities();
        let profile = match settings.video_codec {
            VideoCodec::H264 => capabilities.h264.and_then(|caps| caps.high_profile),
            VideoCodec::H265 => capabilities.h265.and_then(|caps| caps.main_profile),
            VideoCodec::Vp9 | VideoCodec::Av1 => None,
        };
        let Some(profile) = profile else {
            return Ok(None);
        };
        let output = settings.output;
        if !(profile.min_width..=profile.max_width).contains(&output.width)
            || !(profile.min_height..=profile.max_height).contains(&output.height)
        {
            return Ok(None);
        }
        let rate = settings.frame_rate;
        // Allocate 0.2 bits per pixel per frame, with room for complex frames.
        // This is a bitrate target, not an equivalent of x264's CRF value.
        let automatic_bitrate =
            (u64::from(output.width) * u64::from(output.height) * u64::from(rate.numerator())
                / u64::from(rate.denominator())
                / 5)
            .max(1_000_000);
        let bitrate = settings
            .video_bitrate_kbps
            .map_or(automatic_bitrate, |value| u64::from(value) * 1000);
        let control = if profile.rate_control.vbr_supported {
            RateControl::VariableBitrate {
                average_bitrate: bitrate,
                max_bitrate: bitrate.saturating_mul(2),
                virtual_buffer_size: std::time::Duration::from_secs(1),
            }
        } else if profile.rate_control.cbr_supported {
            RateControl::ConstantBitrate {
                bitrate,
                virtual_buffer_size: std::time::Duration::from_secs(1),
            }
        } else {
            if settings.video_bitrate_kbps.is_some() {
                return Ok(None);
            }
            RateControl::Disabled
        };
        let input_parameters = VideoParameters {
            width: NonZeroU32::new(output.width)
                .ok_or(EncodingError::Invalid("Invalid video width"))?,
            height: NonZeroU32::new(output.height)
                .ok_or(EncodingError::Invalid("Invalid video height"))?,
            target_framerate: gpu_video::parameters::Rational {
                numerator: rate.numerator(),
                denominator: NonZeroU32::new(rate.denominator())
                    .ok_or(EncodingError::Invalid("Invalid frame rate"))?,
            },
        };
        let (sender, receiver) = mpsc::channel();
        // A cancelled export drops the receiver before pending callbacks.
        let callback = move |chunk| {
            let _ = sender.send(chunk);
        };
        let color_range = match settings.color.range {
            ExportColorRange::Limited => ColorRange::Limited,
            ExportColorRange::Full => ColorRange::Full,
        };
        let (encoder, headers) = match settings.video_codec {
            VideoCodec::H264 => {
                let mut parameters = video
                    .encoder_output_parameters_h264_high_quality(control)
                    .map_err(EncodingError::backend)?;
                parameters.idr_period = NonZeroU32::new(settings.gop);
                parameters.color_space = Some(ColorSpace::BT709);
                parameters.color_range = Some(color_range);
                let encoder = video
                    .create_wgpu_textures_encoder_h264(
                        queue,
                        EncoderParametersH264 {
                            input_parameters,
                            output_parameters: parameters,
                            max_in_flight_submissions: Some(depth as u32),
                        },
                        callback,
                    )
                    .map_err(EncodingError::backend)?;
                let mut headers = encoder.sps().map_err(EncodingError::backend)?;
                headers.extend(encoder.pps().map_err(EncodingError::backend)?);
                (NativeEncoder::H264(encoder), headers)
            }
            VideoCodec::H265 => {
                let mut parameters = video
                    .encoder_output_parameters_h265_high_quality(control)
                    .map_err(EncodingError::backend)?;
                parameters.idr_period = NonZeroU32::new(settings.gop);
                parameters.color_space = Some(ColorSpace::BT709);
                parameters.color_range = Some(color_range);
                let encoder = video
                    .create_wgpu_textures_encoder_h265(
                        queue,
                        EncoderParametersH265 {
                            input_parameters,
                            output_parameters: parameters,
                            max_in_flight_submissions: Some(depth as u32),
                        },
                        callback,
                    )
                    .map_err(EncodingError::backend)?;
                let mut headers = encoder.vps().map_err(EncodingError::backend)?;
                headers.extend(encoder.sps().map_err(EncodingError::backend)?);
                headers.extend(encoder.pps().map_err(EncodingError::backend)?);
                (NativeEncoder::H265(encoder), headers)
            }
            VideoCodec::Vp9 | VideoCodec::Av1 => return Ok(None),
        };
        Ok(Some(Self {
            receiver,
            encoder,
            headers,
            pending: VecDeque::with_capacity(depth),
        }))
    }

    pub(in crate::engine::output::visual) fn submit(
        &mut self,
        index: u64,
        renderer: &RenderSession,
        conversion: &ColorConversion,
        scene: &RenderScene,
    ) -> Result<(), EncodingError> {
        let texture = self
            .encoder
            .input_texture()
            .map_err(EncodingError::backend)?;
        let views: Vec<_> = texture
            .texture()
            .planes()
            .iter()
            .map(|plane| {
                plane.create_view_with_usage(None, Some(wgpu::TextureUsages::RENDER_ATTACHMENT))
            })
            .collect();
        let commands = conversion.render(renderer, scene, &views)?;
        renderer.gpu().queue.submit([commands.finish()]);
        self.encoder
            .encode(gpu_video::InputFrame {
                data: texture,
                pts: Some(index),
            })
            .map_err(EncodingError::backend)?;
        self.pending.push_back(index);
        Ok(())
    }

    pub(in crate::engine::output::visual) fn pending_frames(&self) -> usize {
        self.pending.len()
    }

    pub(in crate::engine::output::visual) fn finish_next(
        &mut self,
        cancelled: crate::engine::output::visual::Cancellation<'_>,
    ) -> Result<Option<VideoEncoderFrame>, EncodingError> {
        let Some(index) = self.pending.pop_front() else {
            return Ok(None);
        };
        self.receive(index, cancelled).map(Some)
    }

    fn receive(
        &mut self,
        index: u64,
        cancelled: crate::engine::output::visual::Cancellation<'_>,
    ) -> Result<VideoEncoderFrame, EncodingError> {
        let deadline = std::time::Instant::now() + crate::engine::output::visual::WAIT_TIMEOUT;
        loop {
            cancelled.check()?;
            match self
                .receiver
                .recv_timeout(std::time::Duration::from_millis(10))
            {
                Ok(chunk) => {
                    if chunk.pts != Some(index) {
                        return Err(EncodingError::Invalid(
                            "GPU encoder returned an unexpected frame timestamp",
                        ));
                    }
                    let data = if self.headers.is_empty() {
                        chunk.data
                    } else {
                        let mut data = std::mem::take(&mut self.headers);
                        data.extend(chunk.data);
                        data
                    };
                    return Ok(VideoEncoderFrame {
                        index,
                        data,
                        keyframe: chunk.is_keyframe,
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) if std::time::Instant::now() < deadline => {}
                Err(mpsc::RecvTimeoutError::Timeout) => return Err(EncodingError::Timeout),
                Err(error) => return Err(EncodingError::backend(error)),
            }
        }
    }

    pub(in crate::engine::output::visual) fn flush(&mut self) -> Result<(), EncodingError> {
        self.encoder.flush().map_err(EncodingError::backend)
    }
}
