//! Raster and vector still-image readers.
mod raster;
mod sequence;
mod svg;

pub(in crate::engine::input) use self::{
    raster::{ImageReader, READER_ID as IMAGE_READER_ID},
    sequence::{ImageSequenceReader, READER_ID as SEQUENCE_READER_ID},
    svg::{READER_ID as SVG_READER_ID, SvgMediaReader},
};
