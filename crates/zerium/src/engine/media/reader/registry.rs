//! Reader lookup, validation, and registration of bundled implementations.
use std::{collections::HashMap, path::Path, sync::Arc};

use zerium_core::media::{MediaAsset, MediaMetadata, MediaTarget};

use super::super::{
    audio::{AudioReader, READER_ID as AUDIO_READER_ID},
    image::{IMAGE_READER_ID, ImageReader, SVG_READER_ID, SvgMediaReader},
    video::{READER_ID as VIDEO_READER_ID, VideoReader},
};
use super::{AudioDecoderSession, MediaError, MediaReader};

#[derive(Default)]
pub(crate) struct MediaReaderRegistry {
    readers: HashMap<String, Box<dyn MediaReader>>,
}

impl MediaReaderRegistry {
    fn register_reader(
        &mut self,
        id: impl Into<String>,
        reader: impl MediaReader + 'static,
    ) -> Result<(), MediaError> {
        let id = id.into();
        if id.is_empty() {
            return Err(MediaError::invalid_input("Media reader ID cannot be empty"));
        }
        if self.readers.contains_key(&id) {
            return Err(MediaError::invalid_input(format!(
                "Media reader '{id}' is already registered"
            )));
        }
        self.readers.insert(id, Box::new(reader));
        Ok(())
    }

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
        self.readers
            .get(reader_id)
            .map(|reader| reader.as_ref())
            .ok_or_else(|| {
                MediaError::ReaderUnavailable(format!(
                    "Media reader '{reader_id}' is not registered"
                ))
            })
    }

    pub(crate) fn open_audio_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn AudioDecoderSession>, MediaError> {
        self.reader_for(&asset.reader_id)?.open_audio_decoder(asset)
    }
}

pub(crate) fn bundled_media_readers() -> Result<Arc<MediaReaderRegistry>, MediaError> {
    let mut registry = MediaReaderRegistry::default();
    registry.register_reader(IMAGE_READER_ID, ImageReader)?;
    registry.register_reader(VIDEO_READER_ID, VideoReader)?;
    registry.register_reader(AUDIO_READER_ID, AudioReader)?;
    registry.register_reader(SVG_READER_ID, SvgMediaReader)?;
    Ok(Arc::new(registry))
}
