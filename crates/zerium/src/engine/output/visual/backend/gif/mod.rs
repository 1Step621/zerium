//! Animated GIF output from straight sRGB frames.
use std::{
    fs::File,
    io::{BufWriter, Write as _},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

use gif::{DisposalMethod, Encoder, Frame, Repeat};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};
use zerium_core::media::VideoFrameRate;

use crate::engine::output::{EncodingError, EncodingSettings, VideoEncoderFrame};

pub(crate) struct Writer<'a> {
    encoder: Encoder<BufWriter<File>>,
    width: u16,
    height: u16,
    frame_rate: VideoFrameRate,
    queued_until: u128,
    pending: Vec<(Vec<u8>, u128)>,
    pool: ThreadPool,
    cancelled: &'a AtomicBool,
}

impl<'a> Writer<'a> {
    pub(super) fn create(
        path: &Path,
        settings: EncodingSettings,
        cancelled: &'a AtomicBool,
    ) -> Result<Self, EncodingError> {
        let width = u16::try_from(settings.output.width)
            .map_err(|_| EncodingError::Invalid("GIF width exceeds 65535 pixels"))?;
        let height = u16::try_from(settings.output.height)
            .map_err(|_| EncodingError::Invalid("GIF height exceeds 65535 pixels"))?;
        // Leave CPU time for rendering and decoding, and bound retained RGBA frames.
        let frame_bytes = usize::from(width) * usize::from(height) * 4;
        let parallelism = thread::available_parallelism()
            .map_or(1, usize::from)
            .min(4)
            .min((64 * 1024 * 1024 / frame_bytes).max(1));
        let pool = ThreadPoolBuilder::new()
            .num_threads(parallelism)
            .thread_name(|index| format!("zerium-gif-{index}"))
            .build()
            .map_err(EncodingError::backend)?;
        let file = BufWriter::new(File::create(path).map_err(EncodingError::backend)?);
        let mut encoder = Encoder::new(file, width, height, &[]).map_err(EncodingError::backend)?;
        if settings.gif_loop {
            encoder
                .set_repeat(Repeat::Infinite)
                .map_err(EncodingError::backend)?;
        }
        Ok(Self {
            encoder,
            width,
            height,
            frame_rate: settings.frame_rate,
            queued_until: 0,
            pending: Vec::new(),
            pool,
            cancelled,
        })
    }

    pub(super) fn write_frame(&mut self, frame: VideoEncoderFrame) -> Result<(), EncodingError> {
        // Quantize absolute boundaries instead of rounding every frame's duration.
        // Sub-centisecond frames are skipped; even a very short export keeps one frame.
        let numerator = u128::from(self.frame_rate.numerator());
        let end = ((u128::from(frame.index) + 1) * u128::from(self.frame_rate.denominator()) * 100
            + numerator / 2)
            / numerator;
        let mut delay = end.saturating_sub(self.queued_until);
        if delay == 0 {
            if self.queued_until != 0 {
                return Ok(());
            }
            delay = 1;
        }
        if frame.data.len() != usize::from(self.width) * usize::from(self.height) * 4 {
            return Err(EncodingError::Invalid("Invalid GIF frame dimensions"));
        }
        if let Some((pixels, duration)) = self.pending.last_mut()
            && *pixels == frame.data
        {
            *duration += delay;
        } else {
            if self.pending.len() == self.pool.current_num_threads() {
                self.flush()?;
            }
            self.pending.push((frame.data, delay));
        }
        self.queued_until += delay;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), EncodingError> {
        let pending = std::mem::take(&mut self.pending);
        let frames = self.pool.install(|| {
            pending
                .into_par_iter()
                .map(|(mut pixels, delay)| {
                    self.check_cancelled()?;
                    // GIF supports a single transparent palette entry, without partial alpha.
                    for pixel in pixels.as_chunks_mut::<4>().0 {
                        pixel[3] = if pixel[3] < 128 { 0 } else { 255 };
                    }
                    let mut frame =
                        Frame::from_rgba_speed(self.width, self.height, &mut pixels, 10);
                    self.check_cancelled()?;
                    frame.dispose = DisposalMethod::Background;
                    frame.make_lzw_pre_encoded();
                    Ok((frame, delay))
                })
                .collect::<Result<Vec<_>, EncodingError>>()
        })?;
        // Parallel collection preserves frame order; only the file writes are serial.
        for (mut frame, mut delay) in frames {
            while delay > 0 {
                self.check_cancelled()?;
                frame.delay = delay.min(u128::from(u16::MAX)) as u16;
                self.encoder
                    .write_lzw_pre_encoded_frame(&frame)
                    .map_err(EncodingError::backend)?;
                delay -= u128::from(frame.delay);
            }
        }
        Ok(())
    }

    fn check_cancelled(&self) -> Result<(), EncodingError> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(EncodingError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub(super) fn finish(mut self) -> Result<(), EncodingError> {
        self.flush()?;
        // Report trailer and buffered-write failures before publishing the output file.
        self.encoder
            .into_inner()
            .map_err(EncodingError::backend)?
            .flush()
            .map_err(EncodingError::backend)
    }
}
