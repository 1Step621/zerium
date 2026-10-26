use gstreamer::{self as gst, prelude::ObjectExt as _};
use zerium_core::timeline::{
    AudioCodec, ExportBitDepth, ExportColorRange, ExportColorSettings, ExportColorSpace, VideoCodec,
};

use crate::engine::output::{EncodingError, EncodingSettings, VideoEncoderInput, VideoPixelFormat};

pub(super) fn video_chain(
    settings: EncodingSettings,
    input: VideoEncoderInput,
) -> Result<(gst::Caps, Vec<gst::Element>), EncodingError> {
    let mut elements = Vec::new();
    let mut caps = match input {
        VideoEncoderInput::Encoded => gst::Caps::builder(match settings.video_codec {
            VideoCodec::H264 => "video/x-h264",
            VideoCodec::H265 => "video/x-h265",
            _ => return Err(EncodingError::Invalid("Unsupported native video codec")),
        })
        .field("stream-format", "byte-stream")
        .field("alignment", "au")
        .field("width", settings.output.width as i32)
        .field("height", settings.output.height as i32)
        .build(),
        VideoEncoderInput::Raw(format) => {
            let (input_format, encoder_format) = match format {
                VideoPixelFormat::Nv12 => (gstreamer_video::VideoFormat::Nv12, "I420"),
                VideoPixelFormat::P010 => (gstreamer_video::VideoFormat::P01010le, "I420_10LE"),
            };
            let info = gstreamer_video::VideoInfo::builder(
                input_format,
                settings.output.width,
                settings.output.height,
            )
            .colorimetry(&colorimetry(settings.color))
            .build()
            .map_err(EncodingError::backend)?;
            let caps = gst::Caps::builder("video/x-raw")
                .field("format", encoder_format)
                .build();
            // Preserve the rendered precision through software format negotiation.
            elements.extend([
                gst::ElementFactory::make("videoconvert")
                    .build()
                    .map_err(EncodingError::backend)?,
                gst::ElementFactory::make("capsfilter")
                    .property("caps", caps)
                    .build()
                    .map_err(EncodingError::backend)?,
                video_encoder(settings)?,
            ]);
            info.to_caps().map_err(EncodingError::backend)?
        }
    };
    caps.make_mut().set(
        "framerate",
        gst::Fraction::new(
            i32::try_from(settings.frame_rate.numerator())
                .map_err(|_| EncodingError::Invalid("Frame rate is too large"))?,
            i32::try_from(settings.frame_rate.denominator())
                .map_err(|_| EncodingError::Invalid("Frame rate is too large"))?,
        ),
    );
    elements.push(video_parser(settings.video_codec)?);
    Ok((caps, elements))
}

fn colorimetry(color: ExportColorSettings) -> gstreamer_video::VideoColorimetry {
    use gstreamer_video::{
        VideoColorMatrix, VideoColorPrimaries, VideoColorRange, VideoTransferFunction,
    };
    gstreamer_video::VideoColorimetry::new(
        match color.range {
            ExportColorRange::Full => VideoColorRange::Range0_255,
            ExportColorRange::Limited => VideoColorRange::Range16_235,
        },
        if color.space == ExportColorSpace::Bt709 {
            VideoColorMatrix::Bt709
        } else {
            VideoColorMatrix::Bt2020
        },
        match color.space {
            ExportColorSpace::Bt709 => VideoTransferFunction::Bt709,
            ExportColorSpace::Bt2020 => VideoTransferFunction::Bt202010,
            ExportColorSpace::Bt2100Pq => VideoTransferFunction::Smpte2084,
            ExportColorSpace::Bt2100Hlg => VideoTransferFunction::AribStdB67,
        },
        if color.space == ExportColorSpace::Bt709 {
            VideoColorPrimaries::Bt709
        } else {
            VideoColorPrimaries::Bt2020
        },
    )
}

fn video_encoder(settings: EncodingSettings) -> Result<gst::Element, EncodingError> {
    let bitrate = settings.video_bitrate_kbps;
    let builder = match settings.video_codec {
        VideoCodec::H264 => {
            let builder = gst::ElementFactory::make("x264enc")
                .property_from_str("speed-preset", "medium")
                .property_from_str("tune", "zerolatency")
                .property("key-int-max", settings.gop);
            if let Some(bitrate) = bitrate {
                builder
                    .property_from_str("pass", "cbr")
                    .property("bitrate", bitrate)
            } else {
                builder
                    .property_from_str("pass", "qual")
                    .property("quantizer", 18u32)
            }
        }
        VideoCodec::H265 => {
            let builder = gst::ElementFactory::make("x265enc")
                .property_from_str("speed-preset", "medium")
                .property_from_str("tune", "zerolatency")
                .property("key-int-max", settings.gop as i32);
            if let Some(bitrate) = bitrate {
                builder.property("bitrate", bitrate)
            } else {
                builder.property("option-string", "crf=20")
            }
        }
        VideoCodec::Vp9 => {
            // No lookahead: audio and video are fed together through bounded queues.
            let builder = gst::ElementFactory::make("vp9enc")
                .property("lag-in-frames", 0i32)
                .property("cpu-used", 2i32)
                .property("keyframe-max-dist", settings.gop as i32);
            if let Some(bitrate) = bitrate {
                builder
                    .property_from_str("end-usage", "vbr")
                    .property("target-bitrate", (bitrate * 1000) as i32)
            } else {
                builder
                    .property_from_str("end-usage", "cq")
                    .property("target-bitrate", 0i32)
                    .property("cq-level", 24i32)
            }
        }
        VideoCodec::Av1 if settings.color.depth == ExportBitDepth::Ten => {
            if settings.output.width < 64 || settings.output.height < 64 {
                return Err(EncodingError::Invalid(
                    "10-bit AV1 requires dimensions of at least 64 × 64",
                ));
            }
            // Keep lookahead and temporal filtering off so alternating audio/video
            // submission needs only a small, bounded group of encoded frames.
            let builder = gst::ElementFactory::make("svtav1enc")
                .property("preset", 8u32)
                .property(
                    "parameters-string",
                    "lookahead=0:enable-tf=0:hierarchical-levels=2",
                )
                .property("intra-period-length", settings.gop as i32 - 1);
            let builder = if let Some(bitrate) = bitrate {
                builder
                    .property("crf", -1i32)
                    .property("target-bitrate", bitrate)
            } else {
                builder.property("crf", 30i32)
            };
            let encoder = builder.build().map_err(EncodingError::backend)?;
            // SVT-AV1 3.0 replaced the logical CPU count with a parallelism level.
            let parallelism = if encoder.find_property("level-of-parallelism").is_some() {
                "level-of-parallelism"
            } else {
                "logical-processors"
            };
            encoder.set_property(parallelism, 1u32);
            return Ok(encoder);
        }
        VideoCodec::Av1 => {
            let builder = gst::ElementFactory::make("av1enc")
                .property("lag-in-frames", 0u32)
                .property("cpu-used", 6i32)
                .property("keyframe-max-dist", settings.gop as i32)
                .property("max-quantizer", 63u32);
            if let Some(bitrate) = bitrate {
                builder
                    .property_from_str("end-usage", "vbr")
                    .property("target-bitrate", bitrate)
            } else {
                builder
                    .property_from_str("end-usage", "q")
                    .property("min-quantizer", 30u32)
                    .property("max-quantizer", 30u32)
            }
        }
    };
    builder.build().map_err(EncodingError::backend)
}

fn video_parser(codec: VideoCodec) -> Result<gst::Element, EncodingError> {
    gst::ElementFactory::make(match codec {
        VideoCodec::H264 => "h264parse",
        VideoCodec::H265 => "h265parse",
        VideoCodec::Vp9 => "vp9parse",
        VideoCodec::Av1 => "av1parse",
    })
    .build()
    .map_err(EncodingError::backend)
}

pub(super) fn audio_encoder(settings: EncodingSettings) -> Result<gst::Element, EncodingError> {
    let bitrate = (settings.audio_bitrate_kbps * 1000) as i32;
    let builder = match settings.audio_codec {
        AudioCodec::Aac => gst::ElementFactory::make("avenc_aac").property("bitrate", bitrate),
        AudioCodec::Opus => gst::ElementFactory::make("opusenc").property("bitrate", bitrate),
        AudioCodec::Flac => gst::ElementFactory::make("flacenc"),
    };
    builder.build().map_err(EncodingError::backend)
}

pub(super) fn audio_parser(codec: AudioCodec) -> Result<gst::Element, EncodingError> {
    gst::ElementFactory::make(match codec {
        AudioCodec::Aac => "aacparse",
        AudioCodec::Opus => "opusparse",
        AudioCodec::Flac => "flacparse",
    })
    .build()
    .map_err(EncodingError::backend)
}
