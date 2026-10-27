use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::timeline::ProjectSettingsError;

/// Output primaries and transfer function are chosen together as a color standard.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportColorSpace {
    #[default]
    Bt709,
    Bt2020,
    Bt2100Pq,
    Bt2100Hlg,
}

impl ExportColorSpace {
    pub const ALL: [Self; 4] = [Self::Bt709, Self::Bt2020, Self::Bt2100Pq, Self::Bt2100Hlg];

    pub fn is_hdr(self) -> bool {
        self == Self::Bt2100Pq || self == Self::Bt2100Hlg
    }
}

impl std::fmt::Display for ExportColorSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Bt709 => "BT.709",
            Self::Bt2020 => "BT.2020 (SDR)",
            Self::Bt2100Pq => "BT.2100 PQ (HDR)",
            Self::Bt2100Hlg => "BT.2100 HLG (HDR)",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportColorRange {
    #[default]
    Limited,
    Full,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportBitDepth {
    #[default]
    Eight,
    Ten,
}

impl ExportBitDepth {
    pub fn bits(self) -> u32 {
        match self {
            Self::Eight => 8,
            Self::Ten => 10,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExportColorSettings {
    pub space: ExportColorSpace,
    pub range: ExportColorRange,
    pub depth: ExportBitDepth,
}

impl ExportColorSettings {
    pub fn set_space(&mut self, space: ExportColorSpace) {
        self.space = space;
        if space.is_hdr() {
            self.depth = ExportBitDepth::Ten;
        }
    }

    pub fn set_depth(&mut self, depth: ExportBitDepth) {
        self.depth = depth;
        if depth == ExportBitDepth::Eight && self.space.is_hdr() {
            self.space = ExportColorSpace::Bt2020;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportContainer {
    #[default]
    Mp4,
    Matroska,
    Webm,
    Gif,
}

impl ExportContainer {
    pub const ALL: [Self; 4] = [Self::Mp4, Self::Matroska, Self::Webm, Self::Gif];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Matroska => "mkv",
            Self::Webm => "webm",
            Self::Gif => "gif",
        }
    }

    pub fn video_codecs(self) -> &'static [VideoCodec] {
        match self {
            Self::Mp4 | Self::Matroska => &[
                VideoCodec::H264,
                VideoCodec::H265,
                VideoCodec::Vp9,
                VideoCodec::Av1,
            ],
            Self::Webm => &[VideoCodec::Vp9, VideoCodec::Av1],
            Self::Gif => &[],
        }
    }

    pub fn audio_codecs(self) -> &'static [AudioCodec] {
        // Export combinations with correct encoder-delay and padding handling.
        match self {
            Self::Mp4 => &[AudioCodec::Aac],
            Self::Matroska => &[AudioCodec::Opus, AudioCodec::Flac],
            Self::Webm => &[AudioCodec::Opus],
            Self::Gif => &[],
        }
    }
}

impl std::fmt::Display for ExportContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Mp4 => "MP4",
            Self::Matroska => "Matroska",
            Self::Webm => "WebM",
            Self::Gif => "GIF",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    #[default]
    H264,
    H265,
    Vp9,
    Av1,
}

impl VideoCodec {
    fn max_bitrate_kbps(self, depth: ExportBitDepth) -> u32 {
        match (self, depth) {
            (Self::Av1, ExportBitDepth::Ten) => 100_000,
            (Self::H265, _) => 102_400,
            _ => 1_000_000,
        }
    }
}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::H264 => "H.264",
            Self::H265 => "H.265",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioCodec {
    #[default]
    Aac,
    Opus,
    Flac,
}

impl AudioCodec {
    pub fn sample_rates(self) -> &'static [u32] {
        match self {
            Self::Opus => &[48_000],
            Self::Aac | Self::Flac => &[44_100, 48_000],
        }
    }
}

impl std::fmt::Display for AudioCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Aac => "AAC",
            Self::Opus => "Opus",
            Self::Flac => "FLAC",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExportSettings {
    pub destination: Option<PathBuf>,
    #[serde(default)]
    pub container: ExportContainer,
    #[serde(default)]
    pub video_codec: VideoCodec,
    #[serde(default)]
    pub audio_codec: AudioCodec,
    #[serde(default)]
    pub color: ExportColorSettings,
    pub gif_loop: bool,
    /// None keeps the encoder's automatic quality policy.
    pub video_bitrate_kbps: Option<u32>,
    pub audio_enabled: bool,
    pub audio_sample_rate: u32,
    pub audio_channels: u16,
    pub audio_bitrate_kbps: u32,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            destination: None,
            container: ExportContainer::default(),
            video_codec: VideoCodec::default(),
            audio_codec: AudioCodec::default(),
            color: ExportColorSettings::default(),
            gif_loop: true,
            video_bitrate_kbps: None,
            audio_enabled: true,
            audio_sample_rate: 48_000,
            audio_channels: 2,
            audio_bitrate_kbps: 192,
        }
    }
}

impl ExportSettings {
    pub fn set_container(&mut self, container: ExportContainer) {
        self.container = container;
        if !container.video_codecs().contains(&self.video_codec)
            && let Some(&codec) = container.video_codecs().first()
        {
            self.video_codec = codec;
        }
        if !container.audio_codecs().contains(&self.audio_codec)
            && let Some(&codec) = container.audio_codecs().first()
        {
            self.set_audio_codec(codec);
        }
        if let Some(path) = &mut self.destination {
            path.set_extension(container.extension());
        }
    }

    pub fn set_audio_codec(&mut self, codec: AudioCodec) {
        self.audio_codec = codec;
        if !codec.sample_rates().contains(&self.audio_sample_rate) {
            self.audio_sample_rate = codec.sample_rates()[0];
        }
    }

    pub fn validate(&self) -> Result<(), ProjectSettingsError> {
        if self
            .destination
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
        {
            return Err(ProjectSettingsError::out_of_range(
                "Export destination must not be empty",
            ));
        }
        if self.container == ExportContainer::Gif {
            return Ok(());
        }
        if !self.container.video_codecs().contains(&self.video_codec)
            || !self.container.audio_codecs().contains(&self.audio_codec)
        {
            return Err(ProjectSettingsError::out_of_range(
                "The selected codecs are not supported by the export container",
            ));
        }
        if self.color.space.is_hdr() && self.color.depth != ExportBitDepth::Ten {
            return Err(ProjectSettingsError::out_of_range(
                "HDR output requires 10-bit encoding",
            ));
        }
        let max_bitrate = self.video_codec.max_bitrate_kbps(self.color.depth);
        if self
            .video_bitrate_kbps
            .is_some_and(|value| !(100..=max_bitrate).contains(&value))
        {
            return Err(ProjectSettingsError::out_of_range(format!(
                "Video bitrate must be 100–{max_bitrate} kbps"
            )));
        }
        if !self
            .audio_codec
            .sample_rates()
            .contains(&self.audio_sample_rate)
            || ![1, 2].contains(&self.audio_channels)
            || !(32..=512).contains(&self.audio_bitrate_kbps)
        {
            return Err(ProjectSettingsError::out_of_range(
                "Invalid export audio settings",
            ));
        }
        Ok(())
    }

    pub fn exports_audio(&self) -> bool {
        self.audio_enabled && !self.container.audio_codecs().is_empty()
    }
}
