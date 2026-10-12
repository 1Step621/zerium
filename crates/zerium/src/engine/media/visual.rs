//! Shared frame requests and persistent decoder state for preview and export.
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use zerium_core::{
    media::{MediaAsset, MediaKind, MediaMetadataCache},
    timeline::TimelineItem,
};

use super::reader::{
    DecodedVideoFrame, ImageDecoderSession, MediaError, MediaInputId, MediaReaderRegistry,
    VideoDecodeSize, VideoDecoderSession,
};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct MediaFrameRequest {
    pub(crate) input: MediaInputId,
    pub(crate) asset: MediaAsset,
    pub(crate) presentation_time: Duration,
    pub(crate) size: VideoDecodeSize,
}

impl MediaFrameRequest {
    pub(crate) fn from_input(
        item: &TimelineItem,
        input: MediaInputId,
        local_seconds: f64,
        cache: &MediaMetadataCache,
        size: VideoDecodeSize,
    ) -> Option<Self> {
        let (asset, playback) = item.media_input(input.effect_id, &input.input_id, cache)?;
        let presentation_time = if asset.kind.is_temporal() {
            playback.sample(local_seconds, asset.duration)?.time
        } else {
            Duration::ZERO
        };
        let size = match asset.kind {
            MediaKind::Video { width, height, .. } => VideoDecodeSize {
                max_width: width,
                max_height: height,
            },
            _ => size,
        };
        Some(Self {
            input,
            asset,
            presentation_time,
            size,
        })
    }
}

enum Decoder {
    Video(Box<dyn VideoDecoderSession>),
    Image(Box<dyn ImageDecoderSession>),
}

#[derive(Default)]
pub(crate) struct VisualDecoder {
    session: Option<(MediaAsset, Decoder)>,
}

impl VisualDecoder {
    pub(crate) fn decode(
        &mut self,
        readers: &MediaReaderRegistry,
        request: &MediaFrameRequest,
        device: Option<&wgpu::Device>,
        cancelled: &AtomicBool,
    ) -> Result<DecodedVideoFrame, MediaError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(MediaError::Cancelled);
        }
        if self
            .session
            .as_ref()
            .is_none_or(|(asset, _)| *asset != request.asset)
        {
            self.session = None;
            let asset = &request.asset;
            let reader = readers.reader_for(&asset.reader_id)?;
            let decoder = match asset.kind {
                MediaKind::Video { .. } => {
                    Decoder::Video(reader.open_video_decoder(asset, device)?)
                }
                MediaKind::Image { .. } => Decoder::Image(reader.open_image_decoder(asset)?),
                MediaKind::Audio { .. } => {
                    return Err(MediaError::unsupported(
                        "Cannot create a video decoder from audio media",
                    ));
                }
            };
            self.session = Some((asset.clone(), decoder));
        }
        let (asset, decoder) = self.session.as_mut().expect("decoder was opened above");
        match decoder {
            Decoder::Video(decoder) => decoder.decode_at(request.presentation_time, cancelled),
            Decoder::Image(decoder) => Ok(DecodedVideoFrame {
                presentation_time: Duration::ZERO,
                duration: asset.duration,
                frame: decoder.render(request.size, cancelled)?,
            }),
        }
    }
}
