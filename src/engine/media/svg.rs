use std::{
    fs,
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use resvg::{tiny_skia, usvg};

use crate::{
    domain::{
        media::{MediaAsset, MediaKind},
        plugin::MediaType,
    },
    engine::frame::RgbaFrame,
};

use super::reader::{
    ImageDecoderSession, MediaError, MediaProbe, MediaReader, MediaStreamDurations, VideoDecodeSize,
};

pub(super) const READER_ID: &str = "zerium.svg";
const IMAGE_DURATION: Duration = Duration::from_secs(5);

pub(super) struct SvgMediaReader;

impl MediaReader for SvgMediaReader {
    fn probe(&self, path: &Path, media_type: MediaType) -> Result<Option<MediaProbe>, MediaError> {
        if media_type != MediaType::Image {
            return Ok(None);
        }
        let tree = read_tree(path)?;
        let width = dimension(tree.size().width())?;
        let height = dimension(tree.size().height())?;
        Ok(Some(MediaProbe {
            duration: IMAGE_DURATION,
            kind: MediaKind::Image { width, height },
            streams: MediaStreamDurations { video: None },
        }))
    }

    fn open_image_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn ImageDecoderSession>, MediaError> {
        if !matches!(asset.kind, MediaKind::Image { .. }) {
            return Err(MediaError::external("SVG readerには画像素材が必要です"));
        }
        Ok(Box::new(SvgDecoder {
            tree: read_tree(&asset.path)?,
            cached: None,
        }))
    }
}

fn dimension(value: f32) -> Result<u32, MediaError> {
    if !value.is_finite() || value <= 0. || value > u32::MAX as f32 {
        return Err(MediaError::external("SVGのサイズが不正です"));
    }
    Ok(value.round().max(1.) as u32)
}

fn read_tree(path: &Path) -> Result<usvg::Tree, MediaError> {
    let data = fs::read(path).map_err(|error| {
        MediaError::external(format!("SVG '{}' を開けません: {error}", path.display()))
    })?;
    let options = usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        fontdb: system_fonts().clone(),
        ..usvg::Options::default()
    };
    usvg::Tree::from_data(&data, &options).map_err(|error| {
        MediaError::external(format!(
            "SVG '{}' を読み込めません: {error}",
            path.display()
        ))
    })
}

fn system_fonts() -> &'static Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        Arc::new(fonts)
    })
}

struct SvgDecoder {
    tree: usvg::Tree,
    cached: Option<RgbaFrame>,
}

impl ImageDecoderSession for SvgDecoder {
    fn render(
        &mut self,
        size: VideoDecodeSize,
        cancelled: &AtomicBool,
    ) -> Result<RgbaFrame, MediaError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        if size.max_width == 0 || size.max_height == 0 {
            return Err(MediaError::external("SVGの描画サイズが不正です"));
        }
        if let Some(frame) = &self.cached
            && frame.width == size.max_width
            && frame.height == size.max_height
        {
            return Ok(frame.clone());
        }
        let mut pixmap = tiny_skia::Pixmap::new(size.max_width, size.max_height)
            .ok_or_else(|| MediaError::external("SVGの描画領域を確保できません"))?;
        let scale_x = size.max_width as f32 / self.tree.size().width();
        let scale_y = size.max_height as f32 / self.tree.size().height();
        if !scale_x.is_finite() || !scale_y.is_finite() {
            return Err(MediaError::external("SVGの拡大率が不正です"));
        }
        resvg::render(
            &self.tree,
            tiny_skia::Transform::from_scale(scale_x, scale_y),
            &mut pixmap.as_mut(),
        );
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        // tiny-skia stores premultiplied RGBA; media textures use straight alpha.
        let mut rgba = pixmap.take();
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let alpha = u32::from(pixel[3]);
            if alpha == 0 {
                pixel[..3].fill(0);
            } else if alpha < 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        let frame = RgbaFrame {
            width: size.max_width,
            height: size.max_height,
            rgba: Arc::from(rgba),
        };
        self.cached = Some(frame.clone());
        Ok(frame)
    }
}
