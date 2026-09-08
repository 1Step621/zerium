use std::sync::Arc;

/// An owned RGBA8 pixel buffer that can be produced by any frame source.
///
/// This is intentionally independent of media decoding: video, images, text,
/// and future generators can all provide frames to the renderer through the
/// same representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}
