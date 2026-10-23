//! Shared frame requests and persistent decoder state for preview and export.
use crate::engine::gpu::GpuContext;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use zerium_core::media::{MediaAsset, MediaKind, MediaPlayback};

use super::reader::{
    DecodeStreamKey, DecodedFrame, ImageDecoder, MediaError, MediaReaderRegistry, RasterSize,
    VideoDecoder,
};

/// Planning identifies a source by `MediaInputId`; demand traversal attaches
/// consumer affinity as `DecodeStreamKey` before preview/export resolves it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MediaFrameRequest<S = DecodeStreamKey> {
    pub(crate) stream: S,
    pub(crate) asset: MediaAsset,
    pub(crate) presentation_time: Duration,
    pub(crate) size: RasterSize,
}

impl<S> MediaFrameRequest<S> {
    pub(crate) fn new(
        stream: S,
        asset: MediaAsset,
        playback: MediaPlayback,
        local_seconds: f64,
        size: RasterSize,
    ) -> Option<Self> {
        let presentation_time = if asset.kind.is_temporal() {
            playback.sample(local_seconds, asset.duration)?.time
        } else {
            Duration::ZERO
        };
        let size = match asset.kind {
            MediaKind::Video { width, height, .. } => RasterSize {
                max_width: width,
                max_height: height,
            },
            _ => size,
        };
        Some(Self {
            stream,
            asset,
            presentation_time,
            size,
        })
    }
}

enum Decoder {
    Video(Box<dyn VideoDecoder>),
    Image(Box<dyn ImageDecoder>),
}

#[derive(Default)]
pub(crate) struct VisualDecodeSession {
    session: Option<(MediaAsset, Decoder)>,
}

impl VisualDecodeSession {
    pub(crate) fn decode(
        &mut self,
        readers: &MediaReaderRegistry,
        request: &MediaFrameRequest,
        gpu: Option<&GpuContext>,
        cancelled: &AtomicBool,
    ) -> Result<DecodedFrame, MediaError> {
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
                MediaKind::Video { .. } => Decoder::Video(reader.open_video_decoder(asset, gpu)?),
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
            Decoder::Image(decoder) => Ok(DecodedFrame {
                presentation_time: Duration::ZERO,
                duration: asset.duration,
                frame: decoder.render(request.size, cancelled)?,
            }),
        }
    }
}
