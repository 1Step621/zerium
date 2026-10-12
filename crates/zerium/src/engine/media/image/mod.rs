//! Raster and vector still-image readers.
mod raster;
mod svg;

pub(super) use raster::{ImageReader, READER_ID as IMAGE_READER_ID};
pub(super) use svg::{READER_ID as SVG_READER_ID, SvgMediaReader};
