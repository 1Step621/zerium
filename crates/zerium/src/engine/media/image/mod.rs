//! Raster and vector still-image readers.
mod raster;
mod sequence;
mod svg;

pub(super) use raster::{ImageReader, READER_ID as IMAGE_READER_ID};
pub(super) use sequence::{ImageSequenceReader, READER_ID as SEQUENCE_READER_ID};
pub(super) use svg::{READER_ID as SVG_READER_ID, SvgMediaReader};
