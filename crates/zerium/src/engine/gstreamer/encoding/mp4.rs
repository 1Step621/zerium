//! Preserve AAC decoder preroll and describe the audible interval in MP4.
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::Path,
};

use mp4::{BoxHeader, BoxType, Error, MoovBox, ReadBox, Result, WriteBox};

// Native AAC-LC in avenc_aac has one frame of encoder delay. Keep this packet:
// decoding later packets alone loses the overlapping window at the beginning.
const AAC_PRIMING_SAMPLES: u64 = 1024;

pub(super) fn finalize_audio(path: &Path, sample_rate: u32, samples: u64) -> Result<()> {
    if samples == 0 {
        return Ok(());
    }
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let length = file.metadata()?.len();
    let moov = find(&children(&mut file, 0, length)?, BoxType::MoovBox)?;
    let boxes = moov.children(&mut file)?;
    let mvhd = find(&boxes, BoxType::MvhdBox)?;
    // Only load timing atoms, never sample tables or compressed media. MoovBox
    // exposes the library's typed headers without reconstructing the full moov.
    let mut timing = MoovBox {
        mvhd: mvhd.read(&mut file)?,
        ..Default::default()
    };
    if timing.mvhd.timescale != sample_rate {
        return Err(Error::InvalidData("Unexpected MP4 movie timescale"));
    }

    let mut patches = Vec::new();
    let mut duration = 0;
    let mut audio_tracks = 0;
    for range in boxes
        .iter()
        .filter(|range| range.header.name == BoxType::TrakBox)
    {
        let boxes = range.children(&mut file)?;
        let tkhd = find(&boxes, BoxType::TkhdBox)?;
        let mdia = find(&boxes, BoxType::MdiaBox)?.children(&mut file)?;
        timing.traks.push(Default::default());
        let track = timing.traks.last_mut().expect("a track was just added");
        track.tkhd = tkhd.read(&mut file)?;
        track.mdia.hdlr = find(&mdia, BoxType::HdlrBox)?.read(&mut file)?;
        if track.mdia.hdlr.handler_type == (*b"soun").into() {
            audio_tracks += 1;
            track.mdia.mdhd = find(&mdia, BoxType::MdhdBox)?.read(&mut file)?;
            if track.mdia.mdhd.timescale != sample_rate
                || track.mdia.mdhd.duration.saturating_sub(samples) < AAC_PRIMING_SAMPLES
            {
                return Err(Error::InvalidData("Unexpected AAC encoder delay"));
            }
            let edts = find(&boxes, BoxType::EdtsBox)?;
            track.edts = Some(edts.read(&mut file)?);
            let edit = track
                .edts
                .as_mut()
                .and_then(|edts| edts.elst.as_mut())
                .ok_or(Error::BoxNotFound(BoxType::ElstBox))?;
            if edit.entries.len() != 1 || edit.entries[0].media_time != 0 {
                return Err(Error::InvalidData("Unexpected AAC edit list"));
            }
            edit.entries[0].media_time = AAC_PRIMING_SAMPLES;
            edit.entries[0].segment_duration = samples;
            track.tkhd.duration = samples;
            patches.push(find(&edts.children(&mut file)?, BoxType::ElstBox)?.patch(edit)?);
            patches.push(tkhd.patch(&track.tkhd)?);
        }
        duration = duration.max(track.tkhd.duration);
    }
    if audio_tracks != 1 {
        return Err(Error::InvalidData("Expected one AAC track"));
    }
    timing.mvhd.duration = duration;
    patches.push(mvhd.patch(&timing.mvhd)?);
    // All patches are validated before writing. Byte lengths and sample offsets
    // stay unchanged, including faststart placement and unrelated metadata.
    for (offset, bytes) in patches {
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&bytes)?;
    }
    file.flush()?;
    Ok(())
}

#[derive(Clone, Copy)]
struct BoxRange {
    header: BoxHeader,
    start: u64,
    body: u64,
    end: u64,
}

impl BoxRange {
    fn children(self, file: &mut File) -> Result<Vec<Self>> {
        children(file, self.body, self.end)
    }

    fn read<T>(self, file: &mut File) -> Result<T>
    where
        for<'a> T: ReadBox<&'a mut File>,
    {
        file.seek(SeekFrom::Start(self.body))?;
        T::read_box(file, self.header.size)
    }

    fn patch<T>(self, atom: &T) -> Result<(u64, Vec<u8>)>
    where
        for<'a> T: WriteBox<&'a mut Vec<u8>>,
    {
        let mut bytes = Vec::new();
        atom.write_box(&mut bytes)?;
        if bytes.len() as u64 != self.end - self.start {
            return Err(Error::InvalidData("MP4 timing atom changed size"));
        }
        Ok((self.start, bytes))
    }
}

fn find(boxes: &[BoxRange], kind: BoxType) -> Result<BoxRange> {
    boxes
        .iter()
        .find(|range| range.header.name == kind)
        .copied()
        .ok_or(Error::BoxNotFound(kind))
}

fn children(file: &mut File, mut start: u64, end: u64) -> Result<Vec<BoxRange>> {
    let mut boxes = Vec::new();
    while start < end {
        if end - start < mp4::HEADER_SIZE {
            return Err(Error::InvalidData("Truncated MP4 atom"));
        }
        file.seek(SeekFrom::Start(start))?;
        let header = BoxHeader::read(file)?;
        let body = file.stream_position()?;
        // BoxHeader normalizes extended headers to an 8-byte header size.
        let limit = if header.size == 0 {
            end
        } else {
            body.checked_add(
                header
                    .size
                    .checked_sub(mp4::HEADER_SIZE)
                    .ok_or(Error::InvalidData("Invalid MP4 atom size"))?,
            )
            .ok_or(Error::InvalidData("MP4 atom size overflow"))?
        };
        if limit > end || limit < body {
            return Err(Error::InvalidData("MP4 atom exceeds its parent"));
        }
        boxes.push(BoxRange {
            header,
            start,
            body,
            end: limit,
        });
        start = limit;
    }
    Ok(boxes)
}
