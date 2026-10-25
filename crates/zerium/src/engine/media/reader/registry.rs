//! Fixed reader lookup and validation of bundled implementations.
use std::path::Path;

use zerium_core::media::{MediaAsset, MediaMetadata, MediaTarget};

use super::super::{
    audio::{AudioReader, READER_ID as AUDIO_READER_ID},
    image::{
        IMAGE_READER_ID, ImageReader, ImageSequenceReader, SEQUENCE_READER_ID, SVG_READER_ID,
        SvgMediaReader,
    },
    video::{READER_ID as VIDEO_READER_ID, VideoReader},
};
use super::{AudioDecoder, MediaError, MediaReader};

pub(crate) struct MediaReaderRegistry;

impl MediaReaderRegistry {
    pub(in crate::engine::media) fn probe(
        &self,
        path: &Path,
        reader: &str,
        target: MediaTarget,
    ) -> Result<Option<MediaMetadata>, MediaError> {
        let metadata = self.reader_for(reader)?.probe(path, target)?;
        if metadata
            .as_ref()
            .is_some_and(|metadata| !metadata.accepts(target))
        {
            return Err(MediaError::invalid_input(format!(
                "Media reader '{reader}' returned invalid metadata"
            )));
        }
        Ok(metadata)
    }

    pub(in crate::engine::media) fn reader_for(
        &self,
        reader_id: &str,
    ) -> Result<&dyn MediaReader, MediaError> {
        match reader_id {
            IMAGE_READER_ID => Ok(&ImageReader),
            SEQUENCE_READER_ID => Ok(&ImageSequenceReader),
            VIDEO_READER_ID => Ok(&VideoReader),
            AUDIO_READER_ID => Ok(&AudioReader),
            SVG_READER_ID => Ok(&SvgMediaReader),
            _ => Err(MediaError::ReaderUnavailable(format!(
                "Media reader '{reader_id}' is not registered"
            ))),
        }
    }

    pub(crate) fn open_audio_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn AudioDecoder>, MediaError> {
        self.reader_for(&asset.reader_id)?.open_audio_decoder(asset)
    }
}
