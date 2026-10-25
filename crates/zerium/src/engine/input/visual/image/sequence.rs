//! A directory of still images, read in natural filename order.
use std::{
    cmp::Ordering,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
    time::Duration,
};

use zerium_core::media::{
    MediaAsset, MediaKind, MediaMetadata, MediaRevision, MediaTarget, VideoFrameRate,
};

use super::raster::{decode_image, image_dimensions, rgba_frame};
use crate::engine::input::{
    DecodedFrame, FrameCache, MediaError, MediaReader, RASTER_CACHE_BYTES, VideoDecoder,
    VisualFrame,
};

pub(in crate::engine::input) const READER_ID: &str = "zerium.image_sequence";
pub(in crate::engine::input) struct ImageSequenceReader;

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"]
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(extension))
        })
}

fn image_paths(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_name().as_encoded_bytes().starts_with(b".")
            && is_image(&path)
            && entry.file_type()?.is_file()
        {
            paths.push(path);
        }
    }
    paths.sort_by(|left, right| natural_name_cmp(left, right));
    Ok(paths)
}

// Compare digit runs by magnitude, without parsing into a bounded integer.
fn natural_name_cmp(left: &Path, right: &Path) -> Ordering {
    let left = left
        .file_name()
        .expect("image file name")
        .as_encoded_bytes();
    let right = right
        .file_name()
        .expect("image file name")
        .as_encoded_bytes();
    let (mut a, mut b) = (left, right);
    let mut padding_order = Ordering::Equal;
    while let (Some(&x), Some(&y)) = (a.first(), b.first()) {
        let order = if x.is_ascii_digit() && y.is_ascii_digit() {
            let a_len = a.iter().take_while(|byte| byte.is_ascii_digit()).count();
            let b_len = b.iter().take_while(|byte| byte.is_ascii_digit()).count();
            let number_a = &a[..a_len];
            let number_b = &b[..b_len];
            let number_a = &number_a[number_a.iter().take_while(|&&byte| byte == b'0').count()..];
            let number_b = &number_b[number_b.iter().take_while(|&&byte| byte == b'0').count()..];
            let order = number_a
                .len()
                .cmp(&number_b.len())
                .then_with(|| number_a.cmp(number_b));
            padding_order = padding_order.then_with(|| a_len.cmp(&b_len));
            a = &a[a_len..];
            b = &b[b_len..];
            order
        } else {
            a = &a[1..];
            b = &b[1..];
            x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase())
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len()
        .cmp(&b.len())
        .then(padding_order)
        .then_with(|| left.cmp(right))
}

// A directory's mtime alone misses edits to existing images. Fingerprinting
// detects added, removed, or edited images and changes during discovery.
fn directory_revision(directory: &Path) -> std::io::Result<MediaRevision> {
    let mut fingerprint = DefaultHasher::new();
    fs::metadata(directory)?.modified()?.hash(&mut fingerprint);
    for path in image_paths(directory)? {
        let metadata = fs::metadata(&path)?;
        let time = metadata.modified()?;
        path.file_name().hash(&mut fingerprint);
        metadata.len().hash(&mut fingerprint);
        time.hash(&mut fingerprint);
    }
    Ok(MediaRevision::Directory {
        fingerprint: fingerprint.finish(),
    })
}

impl MediaReader for ImageSequenceReader {
    fn revision(&self, path: &Path) -> std::io::Result<MediaRevision> {
        directory_revision(path)
    }

    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError> {
        if target != MediaTarget::Visual {
            return Ok(None);
        }
        let paths =
            image_paths(path).map_err(|error| path_error(path, MediaError::external(error)))?;
        if paths.is_empty() {
            return Err(MediaError::invalid_input(format!(
                "'{}' contains no supported still images",
                path.display()
            )));
        }
        let mut dimensions = None;
        for image in &paths {
            let size = image_dimensions(image).map_err(|error| path_error(image, error))?;
            if dimensions.is_some_and(|expected| expected != size) {
                return Err(MediaError::invalid_input(format!(
                    "'{}' has different dimensions from the other images",
                    image.display()
                )));
            }
            dimensions = Some(size);
        }
        let (width, height) = dimensions.expect("nonempty image sequence");
        let frame_rate = VideoFrameRate::new(30, 1).expect("positive FPS");
        let duration = Duration::from_secs_f64(frame_rate.frame_to_seconds(paths.len() as u64));
        let frames = paths
            .into_iter()
            .map(|path| PathBuf::from(path.file_name().expect("image name")))
            .collect();
        Ok(Some(MediaMetadata {
            duration,
            kind: MediaKind::ImageSequence {
                width,
                height,
                frame_rate,
                frames,
            },
        }))
    }

    fn open_video_decoder(
        &self,
        asset: &MediaAsset,
        _: Option<&crate::engine::GpuContext>,
    ) -> Result<Box<dyn VideoDecoder>, MediaError> {
        let MediaKind::ImageSequence {
            width,
            height,
            frame_rate,
            frames,
        } = &asset.kind
        else {
            return Err(MediaError::invalid_input(
                "Image sequence reader requires image sequence media",
            ));
        };
        if *width == 0 || *height == 0 || frames.is_empty() {
            return Err(MediaError::invalid_input(
                "Invalid image sequence dimensions or frame list",
            ));
        }
        Ok(Box::new(SequenceDecoder {
            directory: asset.path.clone(),
            source_size: (*width, *height),
            frame_rate: *frame_rate,
            frames: frames.clone(),
            cached: FrameCache::new(RASTER_CACHE_BYTES),
        }))
    }
}

fn path_error(path: &Path, error: MediaError) -> MediaError {
    match error {
        MediaError::Cancelled => MediaError::Cancelled,
        error => MediaError::external(format!("'{}': {error}", path.display())),
    }
}

struct SequenceDecoder {
    directory: PathBuf,
    source_size: (u32, u32),
    frame_rate: VideoFrameRate,
    frames: Arc<[PathBuf]>,
    cached: FrameCache<usize>,
}

impl SequenceDecoder {
    fn frame_time(&self, index: usize) -> Duration {
        Duration::from_secs_f64(self.frame_rate.frame_to_seconds(index as u64))
    }

    fn read_frame(
        &self,
        index: usize,
        cancelled: &AtomicBool,
    ) -> Result<Arc<VisualFrame>, MediaError> {
        let path = self.directory.join(&self.frames[index]);
        let image = decode_image(&path, cancelled).map_err(|error| path_error(&path, error))?;
        if (image.width(), image.height()) != self.source_size {
            return Err(MediaError::invalid_input(format!(
                "'{}' has changed dimensions",
                path.display()
            )));
        }
        rgba_frame(image.into_rgba8(), cancelled)
    }
}

impl VideoDecoder for SequenceDecoder {
    fn decode_at(
        &mut self,
        time: Duration,
        cancelled: &AtomicBool,
    ) -> Result<DecodedFrame, MediaError> {
        if cancelled.load(AtomicOrdering::Acquire) {
            return Err(MediaError::Cancelled);
        }
        if time >= self.frame_time(self.frames.len()) {
            return Err(MediaError::invalid_input(
                "Image sequence time is past the last frame",
            ));
        }
        // Requests and presentation boundaries round to nanoseconds; tolerate
        // that rounding at exact frame boundaries, as DecodedFrame::covers does.
        let index = ((time.as_nanos() + 1) * u128::from(self.frame_rate.numerator())
            / (1_000_000_000 * u128::from(self.frame_rate.denominator())))
            as usize;
        let index = index.min(self.frames.len() - 1);
        let frame = if let Some(frame) = self.cached.get(&index) {
            frame
        } else {
            let frame = self.read_frame(index, cancelled)?;
            self.cached.insert(index, frame.clone());
            frame
        };
        let start = self.frame_time(index);
        let end = self.frame_time(index + 1);
        Ok(DecodedFrame {
            presentation_time: start,
            duration: end - start,
            frame,
        })
    }
}
