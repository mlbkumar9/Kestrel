//! Transcode page view model.
//!
//! `TranscodeForm` holds every control shown on the Transcode tabs as plain
//! data. Fields that exist in today's core `JobSettings` are mapped by
//! [`TranscodeForm::to_job_settings`] and [`TranscodeForm::from_job_settings`].

use kestrel_core::formats::Container;
use kestrel_core::mkv::MediaMetadata;
use kestrel_core::transcode::{
    AudioChoice, AudioSettings, BurnIn, Crop, Deinterlace, Denoise, FrameRate, HwCodec, HwVendor,
    JobSettings, Mixdown, RateControl, SubtitleSettings, TenBit, VideoEncode, VideoEncoder,
    VideoSettings, quality_control_label,
};

use super::format::{channel_label, codec_label};

pub const CONTAINERS: [Container; 4] = [
    Container::Mkv,
    Container::Mp4,
    Container::WebM,
    Container::Mov,
];

pub const PREVIEW_LENGTHS: [&str; 3] = ["15 s", "30 s", "60 s"];
pub const FLIPS: [&str; 2] = ["None", "Horizontal"];
pub const ROTATIONS: [&str; 4] = ["0\u{b0}", "90\u{b0}", "180\u{b0}", "270\u{b0}"];
pub const CROP_MODES: [&str; 4] = ["Automatic", "Conservative", "None", "Custom"];
pub const RES_LIMITS: [&str; 8] = [
    "None",
    "4320p 8K",
    "2160p 4K",
    "1080p HD",
    "720p HD",
    "576p PAL",
    "480p NTSC",
    "Custom",
];
pub const RES_LIMIT_HEIGHTS: [Option<u32>; 8] = [
    None,
    Some(4320),
    Some(2160),
    Some(1080),
    Some(720),
    Some(576),
    Some(480),
    None,
];
pub const ANAMORPHIC: [&str; 3] = ["None", "Automatic", "Custom"];
pub const MODULUS: [&str; 4] = ["2", "4", "8", "16"];
pub const BORDER_FILLS: [&str; 5] = ["None", "Height", "Width", "Both", "Custom"];
pub const BORDER_COLORS: [&str; 3] = ["Black", "White", "Custom"];
pub const DETELECINE: [&str; 3] = ["Off", "Default", "Custom"];
pub const INTERLACE_DETECT: [&str; 4] = ["Off", "Default", "Less sensitive", "Custom"];
pub const DEINTERLACE: [&str; 4] = ["Off", "Yadif", "Bwdif", "Decomb"];
pub const DEINTERLACE_PRESETS: [&str; 3] = ["Default", "Bob", "Custom"];
pub const DENOISE: [&str; 3] = ["Off", "NLMeans", "HQDN3D"];
pub const DENOISE_PRESETS: [&str; 3] = ["Light", "Medium", "Strong"];
pub const DENOISE_TUNES: [&str; 7] = [
    "None",
    "Film",
    "Grain",
    "High motion",
    "Animation",
    "Tape",
    "Sprite",
];
pub const CHROMA_SMOOTH: [&str; 5] = ["Off", "Ultralight", "Light", "Medium", "Strong"];
pub const SHARPEN: [&str; 3] = ["Off", "Unsharp", "LapSharp"];
pub const SHARPEN_PRESETS: [&str; 3] = ["Light", "Medium", "Strong"];
pub const DEBLOCK: [&str; 5] = ["Off", "Ultralight", "Light", "Medium", "Strong"];
pub const COLOURSPACES: [&str; 5] = ["Off", "BT.2020", "BT.709", "BT.601 SMPTE-C", "BT.601 EBU"];
pub const HDR_MODES: [&str; 2] = ["Keep HDR10 metadata", "Tone-map to SDR"];
pub const TONEMAPS: [&str; 4] = ["Hable", "Mobius", "Reinhard", "BT.2390"];

pub const ENCODERS: [&str; 15] = [
    "H.264 (x264)",
    "H.264 10-bit (x264)",
    "H.265 (x265)",
    "H.265 10-bit (x265)",
    "H.265 12-bit (x265)",
    "AV1 (SVT-AV1)",
    "AV1 10-bit (SVT-AV1)",
    "VP9",
    "H.264 (NVENC)",
    "H.265 (NVENC)",
    "AV1 (NVENC)",
    "H.265 (Intel QSV)",
    "H.265 (AMD AMF)",
    "MPEG-4",
    "Theora",
];
pub const FRAME_RATES: [&str; 9] = [
    "Same as source",
    "23.976",
    "24",
    "25",
    "29.97",
    "30",
    "50",
    "59.94",
    "60",
];
pub const FPS_MODES: [&str; 3] = ["Constant", "Peak", "Variable"];
pub const RATE_MODES: [&str; 2] = ["Constant quality", "Average bitrate"];
pub const VIDEO_TUNES: [&str; 7] = [
    "None",
    "Film",
    "Animation",
    "Grain",
    "Still image",
    "PSNR",
    "SSIM",
];
pub const VIDEO_PROFILES: [&str; 4] = ["Auto", "Main", "Main 10", "Main Still Picture"];
pub const VIDEO_LEVELS: [&str; 5] = ["Auto", "4.0", "4.1", "5.0", "5.1"];

pub const AUDIO_ENCODERS: [&str; 11] = [
    "AAC (avcodec)",
    "HE-AAC",
    "Opus",
    "AC3",
    "E-AC3",
    "FLAC 24-bit",
    "MP3",
    "Vorbis",
    "Auto passthru",
    "TrueHD passthru",
    "DTS-HD passthru",
];
pub const MIXDOWNS: [&str; 7] = [
    "Mono",
    "Stereo",
    "Dolby Surround",
    "Dolby Pro Logic II",
    "5.1",
    "7.1",
    "Same as source",
];
pub const SAMPLE_RATES: [&str; 3] = ["Auto", "44.1 kHz", "48 kHz"];
pub const AUDIO_BITRATES: [u32; 8] = [96, 128, 160, 192, 256, 320, 448, 640];
pub const AUDIO_SELECTION: [&str; 4] = [
    "First track matching languages",
    "All tracks matching languages",
    "All tracks",
    "None",
];
pub const PASSTHRU_FALLBACKS: [&str; 3] = ["AAC (avcodec)", "AC3", "Opus"];
pub const PASSTHRU_FORMATS: [&str; 9] = [
    "AAC", "AC3", "E-AC3", "DTS", "DTS-HD", "TrueHD", "FLAC", "Opus", "MP3",
];

pub const SUB_SELECTION: [&str; 3] = [
    "None",
    "All tracks matching languages",
    "First track matching languages",
];
pub const SUB_BURN: [&str; 4] = [
    "None",
    "Foreign audio subtitle track",
    "First selected track",
    "Foreign audio, then first track",
];
pub const CHARSETS: [&str; 4] = ["UTF-8", "Windows-1252", "ISO-8859-1", "Shift_JIS"];

#[derive(Clone, Debug, PartialEq)]
pub struct AudioTrackForm {
    pub source: String,
    pub source_meta: String,
    pub encoder: usize,
    pub mixdown: usize,
    pub sample_rate: usize,
    pub bitrate: usize,
    pub gain: i32,
    pub drc: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubTrackForm {
    pub source: String,
    pub kind: String,
    pub language: String,
    pub forced_only: bool,
    pub burn: bool,
    pub default: bool,
    pub offset_ms: i32,
    /// Path of an imported external file, when this track is one.
    pub external: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChapterRow {
    pub start: f64,
    pub duration: f64,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    Summary,
    Dimensions,
    Filters,
    Video,
    Audio,
    Subtitles,
    Chapters,
    Tags,
}

impl Tab {
    pub const ALL: [Self; 8] = [
        Self::Summary,
        Self::Dimensions,
        Self::Filters,
        Self::Video,
        Self::Audio,
        Self::Subtitles,
        Self::Chapters,
        Self::Tags,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Dimensions => "Dimensions",
            Self::Filters => "Filters",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Subtitles => "Subtitles",
            Self::Chapters => "Chapters",
            Self::Tags => "Tags",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscodeForm {
    pub preset: String,
    // Summary
    pub container: Container,
    pub web_optimized: bool,
    pub align_av: bool,
    pub passthru_metadata: bool,
    pub keep_attachments: bool,
    pub preview_length: usize,
    // Dimensions
    pub flip: usize,
    pub rotation: usize,
    pub crop_mode: usize,
    pub crop: [u32; 4],
    pub res_limit: usize,
    pub anamorphic: usize,
    pub pixel_aspect: String,
    pub modulus: usize,
    pub scaled_width: u32,
    pub scaled_height: u32,
    pub optimal_size: bool,
    pub allow_upscale: bool,
    pub border_fill: usize,
    pub border_color: usize,
    // Filters
    pub detelecine: usize,
    pub interlace_detect: usize,
    pub deinterlace: usize,
    pub deinterlace_preset: usize,
    pub denoise: usize,
    pub denoise_preset: usize,
    pub denoise_tune: usize,
    pub chroma_smooth: usize,
    pub sharpen: usize,
    pub sharpen_preset: usize,
    pub deblock: usize,
    pub colourspace: usize,
    pub hdr: usize,
    pub tonemap: usize,
    pub grayscale: bool,
    // Video
    pub encoder: usize,
    pub frame_rate: usize,
    pub fps_mode: usize,
    pub rate_mode: usize,
    pub rf: u8,
    pub bitrate_kbps: u32,
    pub multipass: bool,
    pub turbo: bool,
    pub speed: u8,
    pub video_tune: usize,
    pub video_profile: usize,
    pub video_level: usize,
    pub fast_decode: bool,
    pub advanced: String,
    // Audio
    pub audio_tracks: Vec<AudioTrackForm>,
    pub audio_selection: usize,
    pub languages: String,
    pub passthru_fallback: usize,
    pub passthru_allowed: [bool; 9],
    /// Defaults for tracks added later; a loaded preset sets these.
    pub default_audio_encoder: usize,
    pub default_audio_mixdown: usize,
    pub default_audio_bitrate: usize,
    // Subtitles
    pub sub_tracks: Vec<SubTrackForm>,
    pub sub_selection: usize,
    pub sub_burn: usize,
    pub charset: usize,
    pub foreign_scan: bool,
    pub closed_captions: bool,
    // Chapters and tags
    pub create_chapters: bool,
    pub chapters: Vec<ChapterRow>,
    pub tag_title: String,
    pub tag_actors: String,
    pub tag_director: String,
    pub tag_date: String,
    pub tag_genre: String,
    pub tag_comment: String,
    pub tag_description: String,
    // Output
    pub save_as: String,
    pub folder: String,
    pub apply_to_all: bool,
}

impl Default for TranscodeForm {
    fn default() -> Self {
        Self {
            preset: "Same quality, smaller file".to_owned(),
            container: Container::Mkv,
            web_optimized: true,
            align_av: true,
            passthru_metadata: true,
            keep_attachments: false,
            preview_length: 0,
            flip: 0,
            rotation: 0,
            crop_mode: 0,
            crop: [0; 4],
            res_limit: 0,
            anamorphic: 1,
            pixel_aspect: "1 : 1".to_owned(),
            modulus: 0,
            scaled_width: 0,
            scaled_height: 0,
            optimal_size: true,
            allow_upscale: false,
            border_fill: 0,
            border_color: 0,
            detelecine: 0,
            interlace_detect: 1,
            deinterlace: 0,
            deinterlace_preset: 0,
            denoise: 0,
            denoise_preset: 0,
            denoise_tune: 0,
            chroma_smooth: 0,
            sharpen: 0,
            sharpen_preset: 0,
            deblock: 0,
            colourspace: 0,
            hdr: 0,
            tonemap: 0,
            grayscale: false,
            encoder: 3,
            frame_rate: 0,
            fps_mode: 2,
            rate_mode: 0,
            rf: 22,
            bitrate_kbps: 6000,
            multipass: true,
            turbo: true,
            speed: 5,
            video_tune: 0,
            video_profile: 0,
            video_level: 0,
            fast_decode: false,
            advanced: String::new(),
            audio_tracks: Vec::new(),
            audio_selection: 1,
            languages: String::new(),
            passthru_fallback: 0,
            passthru_allowed: [true, true, true, true, false, false, true, true, false],
            default_audio_encoder: 8,
            default_audio_mixdown: 6,
            default_audio_bitrate: 2,
            sub_tracks: Vec::new(),
            sub_selection: 1,
            sub_burn: 0,
            charset: 0,
            foreign_scan: true,
            closed_captions: false,
            create_chapters: true,
            chapters: Vec::new(),
            tag_title: String::new(),
            tag_actors: String::new(),
            tag_director: String::new(),
            tag_date: String::new(),
            tag_genre: String::new(),
            tag_comment: String::new(),
            tag_description: String::new(),
            save_as: String::new(),
            folder: String::new(),
            apply_to_all: false,
        }
    }
}

/// Which family an encoder-list entry belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    X26x,
    Av1,
    Vp9,
    Hardware,
    Legacy,
}

impl TranscodeForm {
    #[cfg(test)]
    pub fn encoder_name(&self) -> &'static str {
        ENCODERS[self.encoder.min(ENCODERS.len() - 1)]
    }

    pub fn encoder_family(&self) -> Family {
        match self.encoder {
            0..=4 => Family::X26x,
            5 | 6 => Family::Av1,
            7 => Family::Vp9,
            8..=12 => Family::Hardware,
            _ => Family::Legacy,
        }
    }

    pub fn is_constant_quality(&self) -> bool {
        self.rate_mode == 0
    }

    /// Core encoder for the selected list entry. Entries without a core
    /// equivalent (12-bit x265, MPEG-4, Theora) fall back to the nearest one.
    pub fn core_encoder(&self) -> (VideoEncoder, TenBit) {
        let hardware = |vendor, codec| VideoEncoder::Hardware { vendor, codec };
        match self.encoder {
            0 => (VideoEncoder::X264, TenBit::Off),
            1 => (VideoEncoder::X264, TenBit::On),
            2 => (VideoEncoder::X265, TenBit::Off),
            3 | 4 => (VideoEncoder::X265, TenBit::On),
            5 => (VideoEncoder::SvtAv1, TenBit::Off),
            6 => (VideoEncoder::SvtAv1, TenBit::On),
            7 => (VideoEncoder::Vp9, TenBit::MatchSource),
            8 => (
                hardware(HwVendor::Nvenc, HwCodec::H264),
                TenBit::MatchSource,
            ),
            9 => (
                hardware(HwVendor::Nvenc, HwCodec::Hevc),
                TenBit::MatchSource,
            ),
            10 => (hardware(HwVendor::Nvenc, HwCodec::Av1), TenBit::MatchSource),
            11 => (hardware(HwVendor::Qsv, HwCodec::Hevc), TenBit::MatchSource),
            12 => (hardware(HwVendor::Amf, HwCodec::Hevc), TenBit::MatchSource),
            _ => (VideoEncoder::X264, TenBit::Off),
        }
    }

    fn encoder_index_for(encoder: &VideoEncoder, ten_bit: TenBit) -> usize {
        let ten = ten_bit == TenBit::On;
        match encoder {
            VideoEncoder::X264 => usize::from(ten),
            VideoEncoder::X265 => {
                if ten {
                    3
                } else {
                    2
                }
            }
            VideoEncoder::SvtAv1 | VideoEncoder::AomAv1 => {
                if ten {
                    6
                } else {
                    5
                }
            }
            VideoEncoder::Vp9 => 7,
            VideoEncoder::Hardware { vendor, codec } => match (vendor, codec) {
                (HwVendor::Nvenc, HwCodec::H264) => 8,
                (HwVendor::Nvenc, HwCodec::Av1) => 10,
                (HwVendor::Qsv, _) => 11,
                (HwVendor::Amf, _) => 12,
                _ => 9,
            },
        }
    }

    /// Label for the constant-quality value: RF for x264/x265, CRF for AV1 and VP9, CQ for hardware.
    pub fn quality_label(&self) -> &'static str {
        match self.encoder_family() {
            Family::Hardware => "CQ",
            Family::Av1 | Family::Vp9 => "CRF",
            _ => "RF",
        }
    }

    pub fn quality_hint(&self) -> &'static str {
        if self.rf <= 18 {
            "Visually lossless for most sources. Large files."
        } else if self.rf <= 24 {
            "Recommended range for HD. Good balance of size and detail."
        } else {
            "Small files. Visible loss on fine grain and dark scenes."
        }
    }

    /// Encoder preset name for the 0..=9 slider.
    pub fn speed_label(&self) -> String {
        const X26X: [&str; 10] = [
            "ultrafast",
            "superfast",
            "veryfast",
            "faster",
            "fast",
            "medium",
            "slow",
            "slower",
            "veryslow",
            "placebo",
        ];
        const NVENC: [&str; 10] = ["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p7", "p7", "p7"];
        let speed = usize::from(self.speed.min(9));
        match self.encoder_family() {
            Family::Av1 => format!("preset {}", 12 - speed),
            Family::Hardware => NVENC[speed].to_owned(),
            Family::Vp9 => format!("cpu-used {}", 9 - speed),
            Family::Legacy => format!("level {speed}"),
            Family::X26x => X26X[speed].to_owned(),
        }
    }

    /// Short summary such as `H.265 10-bit - RF 22`.
    pub fn video_summary(&self) -> String {
        let name = ENCODERS[self.encoder]
            .replace(" (x265)", "")
            .replace(" (x264)", "")
            .replace(" (SVT-AV1)", " SVT");
        if self.is_constant_quality() {
            format!("{name} \u{b7} {} {}", self.quality_label(), self.rf)
        } else {
            format!("{name} \u{b7} {} kb/s", self.bitrate_kbps)
        }
    }

    /// Maps the 0..=9 slider onto core's 0..=4 speed scale (0 is slowest there).
    fn core_speed(&self) -> u8 {
        4 - ((u32::from(self.speed.min(9)) * 4 + 4) / 9).min(4) as u8
    }

    fn slider_speed(core: u8) -> u8 {
        ((4 - u32::from(core.min(4))) * 9 / 4) as u8
    }

    pub fn core_quality(&self) -> u8 {
        quality_from_rf(&self.core_encoder().0, self.rf)
    }

    pub fn filter_summary(&self) -> String {
        let mut parts = Vec::new();
        if self.deinterlace > 0 {
            parts.push(DEINTERLACE[self.deinterlace].to_lowercase());
        }
        if self.denoise > 0 {
            parts.push(DENOISE[self.denoise].to_lowercase());
        }
        if self.hdr == 1 {
            parts.push("tonemap".to_owned());
        }
        if self.grayscale {
            parts.push("gray".to_owned());
        }
        if parts.is_empty() {
            "none".to_owned()
        } else {
            parts.join(", ")
        }
    }

    pub fn audio_summary(&self) -> String {
        if self.audio_tracks.is_empty() {
            return "No tracks selected".to_owned();
        }
        let tracks = self
            .audio_tracks
            .iter()
            .map(|track| {
                let encoder = AUDIO_ENCODERS[track.encoder].replace(" passthru", "");
                format!("{encoder} {}", MIXDOWNS[track.mixdown])
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} track(s): {tracks}", self.audio_tracks.len())
    }

    pub fn subtitle_summary(&self) -> String {
        if self.sub_tracks.is_empty() {
            return "No subtitle tracks".to_owned();
        }
        self.sub_tracks
            .iter()
            .map(|track| {
                let how = if track.burn { "burned" } else { "soft" };
                let forced = if track.forced_only { " forced" } else { "" };
                format!("{}{forced} ({how})", track.language)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Output size in pixels after crop, limit and scaling, for the dimension summary.
    pub fn final_dimensions(&self, source: Option<(u32, u32)>) -> String {
        let Some((width, height)) = source else {
            return "Source dimensions unknown".to_owned();
        };
        let cropped = (
            width.saturating_sub(self.crop[2] + self.crop[3]),
            height.saturating_sub(self.crop[0] + self.crop[1]),
        );
        let target = match RES_LIMIT_HEIGHTS[self.res_limit] {
            Some(limit) if cropped.1 > limit => (
                (u64::from(cropped.0) * u64::from(limit) / u64::from(cropped.1.max(1))) as u32 / 2
                    * 2,
                limit,
            ),
            _ => cropped,
        };
        let ratio = f64::from(target.0) / f64::from(target.1.max(1));
        format!(
            "{width}x{height} -> crop {}x{} -> {}x{} ({ratio:.2}:1)",
            cropped.0, cropped.1, target.0, target.1
        )
    }

    /// Output row for source audio track `index`, using the default encoder settings.
    pub fn source_audio_track(&self, meta: &MediaMetadata, index: usize) -> Option<AudioTrackForm> {
        let track = meta.audio_tracks.get(index)?;
        let info = format!(
            "{} {}",
            codec_label(&track.codec),
            channel_label(track.channels)
        );
        Some(AudioTrackForm {
            source: format!("{} \u{b7} {} {}", index + 1, track.language, info.trim()),
            source_meta: track.title.clone().unwrap_or_default(),
            encoder: self.default_audio_encoder,
            mixdown: self.default_audio_mixdown,
            sample_rate: 0,
            bitrate: self.default_audio_bitrate,
            gain: 0,
            drc: 0,
        })
    }

    /// Source subtitle track `index` as an output row.
    pub fn source_sub_track(&self, meta: &MediaMetadata, index: usize) -> Option<SubTrackForm> {
        let track = meta.subtitle_tracks.get(index)?;
        Some(SubTrackForm {
            source: format!(
                "{} \u{b7} {} {}",
                index + 1,
                track.language,
                codec_label(&track.codec)
            ),
            kind: codec_label(&track.codec),
            language: track.language.clone(),
            forced_only: track.forced,
            burn: false,
            default: track.default,
            offset_ms: 0,
            external: None,
        })
    }

    /// Fills the track, chapter and output fields from a newly selected source.
    pub fn seed_from_source(&mut self, meta: &MediaMetadata, stem: &str, folder: &str) {
        self.audio_tracks = (0..meta.audio_tracks.len())
            .filter_map(|index| self.source_audio_track(meta, index))
            .collect();
        self.sub_tracks = Vec::new();
        if self.foreign_scan {
            self.sub_tracks.push(SubTrackForm {
                source: "Foreign audio scan".to_owned(),
                kind: "search".to_owned(),
                language: "und".to_owned(),
                forced_only: true,
                burn: self.sub_burn == 1 || self.sub_burn == 3,
                default: false,
                offset_ms: 0,
                external: None,
            });
        }
        for index in 0..meta.subtitle_tracks.len() {
            if let Some(row) = self.source_sub_track(meta, index) {
                self.sub_tracks.push(row);
            }
        }
        self.tag_title = stem.to_owned();
        self.save_as = format!("{stem}.{}", self.container.extension());
        if self.folder.is_empty() {
            self.folder = folder.to_owned();
        }
    }

    /// Current form as core job settings. Controls with no core field yet are
    /// carried by the form only.
    // TODO(integration): TranscodeForm -> kestrel_core::transcode::JobSettings
    pub fn to_job_settings(&self) -> JobSettings {
        let (encoder, ten_bit) = self.core_encoder();
        let rate = if self.is_constant_quality() {
            RateControl::Quality(self.core_quality())
        } else {
            RateControl::Bitrate(self.bitrate_kbps)
        };
        let frame_rate = match FRAME_RATES[self.frame_rate].parse::<f64>() {
            Ok(fps) => FrameRate::Fixed(fps),
            Err(_) => FrameRate::Source,
        };
        let deinterlace = match (self.deinterlace, self.interlace_detect) {
            (0, _) => Deinterlace::Off,
            (_, 0) => Deinterlace::On,
            _ => Deinterlace::Auto,
        };
        let denoise = match (self.denoise, self.denoise_preset) {
            (0, _) => Denoise::Off,
            (_, 0) => Denoise::Light,
            (_, 1) => Denoise::Medium,
            _ => Denoise::Strong,
        };
        let crop = if self.crop_mode == 3 {
            Crop::Custom {
                top: self.crop[0],
                bottom: self.crop[1],
                left: self.crop[2],
                right: self.crop[3],
            }
        } else {
            Crop::None
        };
        let first_audio = self.audio_tracks.first();
        let audio_encoder = first_audio.map_or(self.default_audio_encoder, |t| t.encoder);
        let audio_mixdown = first_audio.map_or(self.default_audio_mixdown, |t| t.mixdown);
        let audio_bitrate = first_audio.map_or(self.default_audio_bitrate, |t| t.bitrate);
        let languages = parse_languages(&self.languages);
        JobSettings {
            container: self.container,
            video: VideoSettings::Encode(VideoEncode {
                encoder,
                rate,
                speed: self.core_speed(),
                max_height: RES_LIMIT_HEIGHTS[self.res_limit],
                frame_rate,
                deinterlace,
                denoise,
                crop,
                tone_map_to_sdr: self.hdr == 1,
                ten_bit,
            }),
            audio: AudioSettings {
                languages: languages.clone(),
                codec: audio_choice(audio_encoder),
                bitrate_kbps: AUDIO_BITRATES[audio_bitrate.min(AUDIO_BITRATES.len() - 1)],
                mixdown: mixdown_choice(audio_mixdown),
            },
            subtitles: SubtitleSettings {
                languages,
                burn_in: match self.sub_burn {
                    0 => BurnIn::None,
                    1 => BurnIn::ForcedOnly,
                    _ => BurnIn::FirstSelected,
                },
                include: self.sub_selection != 0,
            },
            keep_chapters: self.create_chapters,
            keep_metadata: self.passthru_metadata,
            title: (!self.tag_title.trim().is_empty()).then(|| self.tag_title.trim().to_owned()),
        }
    }

    /// Loads a preset's settings over the current form, keeping controls the
    /// core settings do not carry.
    pub fn apply_job_settings(&mut self, name: &str, settings: &JobSettings) {
        self.preset = name.to_owned();
        self.container = settings.container;
        match &settings.video {
            VideoSettings::Copy => {}
            VideoSettings::Encode(encode) => {
                self.encoder = Self::encoder_index_for(&encode.encoder, encode.ten_bit);
                match &encode.rate {
                    RateControl::Quality(quality) => {
                        self.rate_mode = 0;
                        self.rf = rf_from_quality(&encode.encoder, *quality);
                    }
                    RateControl::Bitrate(kbps) => {
                        self.rate_mode = 1;
                        self.bitrate_kbps = *kbps;
                    }
                }
                self.speed = Self::slider_speed(encode.speed);
                self.res_limit = RES_LIMIT_HEIGHTS
                    .iter()
                    .position(|height| *height == encode.max_height)
                    .unwrap_or(0);
                self.frame_rate = match encode.frame_rate {
                    FrameRate::Source => 0,
                    FrameRate::Fixed(fps) => FRAME_RATES
                        .iter()
                        .position(|label| {
                            label
                                .parse::<f64>()
                                .is_ok_and(|candidate| (candidate - fps).abs() < 0.01)
                        })
                        .unwrap_or(0),
                };
                (self.deinterlace, self.interlace_detect) = match encode.deinterlace {
                    Deinterlace::Off => (0, self.interlace_detect),
                    Deinterlace::Auto => (2, 1),
                    Deinterlace::On => (2, 0),
                };
                (self.denoise, self.denoise_preset) = match encode.denoise {
                    Denoise::Off => (0, self.denoise_preset),
                    Denoise::Light => (1, 0),
                    Denoise::Medium => (1, 1),
                    Denoise::Strong => (1, 2),
                };
                match &encode.crop {
                    Crop::None => {
                        if self.crop_mode == 3 {
                            self.crop_mode = 2;
                        }
                        self.crop = [0; 4];
                    }
                    Crop::Custom {
                        top,
                        bottom,
                        left,
                        right,
                    } => {
                        self.crop_mode = 3;
                        self.crop = [*top, *bottom, *left, *right];
                    }
                }
                self.hdr = usize::from(encode.tone_map_to_sdr);
            }
        }
        self.default_audio_encoder = audio_index(&settings.audio.codec);
        self.default_audio_mixdown = match settings.audio.mixdown {
            Mixdown::Source => 6,
            Mixdown::Stereo => 1,
            Mixdown::Surround51 => 4,
        };
        self.default_audio_bitrate = AUDIO_BITRATES
            .iter()
            .position(|rate| *rate >= settings.audio.bitrate_kbps)
            .unwrap_or(AUDIO_BITRATES.len() - 1);
        for track in &mut self.audio_tracks {
            track.encoder = self.default_audio_encoder;
            track.mixdown = self.default_audio_mixdown;
            track.bitrate = self.default_audio_bitrate;
        }
        self.languages = settings.audio.languages.join(", ");
        self.sub_burn = match settings.subtitles.burn_in {
            BurnIn::None => 0,
            BurnIn::ForcedOnly => 1,
            BurnIn::FirstSelected => 2,
        };
        self.sub_selection = if settings.subtitles.include { 1 } else { 0 };
        for track in &mut self.sub_tracks {
            if track.kind == "search" {
                track.burn = matches!(self.sub_burn, 1 | 3);
            }
        }
        self.create_chapters = settings.keep_chapters;
        self.passthru_metadata = settings.keep_metadata;
        if let Some(title) = &settings.title {
            self.tag_title.clone_from(title);
        }
        self.save_as = replace_extension(&self.save_as, self.container.extension());
    }

    /// Form loaded from a preset.
    #[cfg(test)]
    pub fn from_job_settings(name: &str, settings: &JobSettings) -> Self {
        let mut form = Self::default();
        form.apply_job_settings(name, settings);
        form
    }

    pub fn set_container(&mut self, container: Container) {
        self.container = container;
        self.save_as = replace_extension(&self.save_as, container.extension());
    }
}

fn replace_extension(name: &str, extension: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    match name.rsplit_once('.') {
        Some((stem, _)) => format!("{stem}.{extension}"),
        None => format!("{name}.{extension}"),
    }
}

pub fn parse_languages(text: &str) -> Vec<String> {
    kestrel_core::mkv::parse_language_field(text)
}

fn audio_choice(index: usize) -> AudioChoice {
    match index {
        0 | 1 | 6 | 7 => AudioChoice::Aac,
        2 => AudioChoice::Opus,
        3 => AudioChoice::Ac3,
        4 => AudioChoice::Eac3,
        5 => AudioChoice::Flac,
        _ => AudioChoice::Passthrough,
    }
}

fn audio_index(choice: &AudioChoice) -> usize {
    match choice {
        AudioChoice::Aac => 0,
        AudioChoice::Opus => 2,
        AudioChoice::Ac3 => 3,
        AudioChoice::Eac3 => 4,
        AudioChoice::Flac => 5,
        AudioChoice::Passthrough => 8,
    }
}

fn mixdown_choice(index: usize) -> Mixdown {
    match index {
        0..=3 => Mixdown::Stereo,
        4 | 5 => Mixdown::Surround51,
        _ => Mixdown::Source,
    }
}

/// Constant-quality value (RF/CRF/CQ) core shows for a 0..=100 quality, parsed from its own label.
pub fn rf_from_quality(encoder: &VideoEncoder, quality: u8) -> u8 {
    quality_control_label(encoder, quality.min(100))
        .split_whitespace()
        .last()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(22)
        .min(51)
}

/// The 0..=100 quality whose label is closest to `rf`.
pub fn quality_from_rf(encoder: &VideoEncoder, rf: u8) -> u8 {
    (0..=100u8)
        .min_by_key(|quality| i32::from(rf_from_quality(encoder, *quality)).abs_diff(i32::from(rf)))
        .unwrap_or(60)
}

/// Rough output size for a form against a source, used until a sample is measured.
pub fn heuristic_size(form: &TranscodeForm, source_bytes: u64, duration: Option<f64>) -> u64 {
    if !form.is_constant_quality() {
        let seconds = duration.unwrap_or(0.0);
        return (f64::from(form.bitrate_kbps) * 1000.0 / 8.0 * seconds) as u64;
    }
    // Calibrated against a 9.2 GB reference at RF 18 for a 18.4 GB source.
    let mut ratio = 0.5 * 0.89f64.powi(i32::from(form.rf) - 18);
    if form.encoder_family() == Family::Av1 {
        ratio *= 0.75;
    }
    if matches!(form.encoder, 0 | 1 | 8) {
        ratio *= 1.45;
    }
    (source_bytes as f64 * ratio.clamp(0.02, 1.0)) as u64
}

/// Rough encode time in minutes for a source of the given duration.
pub fn heuristic_minutes(form: &TranscodeForm, duration: Option<f64>) -> u64 {
    let hours = duration.unwrap_or(0.0) / 3600.0;
    let hardware = form.encoder_family() == Family::Hardware;
    let av1 = form.encoder_family() == Family::Av1;
    let per_reference = (if hardware { 9.0 } else { 22.0 })
        * (1.0 + f64::from(form.speed) * 0.35)
        * if av1 { 1.4 } else { 1.0 };
    (per_reference * hours / 2.19).round().max(1.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::presets::builtins;

    #[test]
    fn defaults_match_the_prototype() {
        let form = TranscodeForm::default();
        assert_eq!(form.encoder_name(), "H.265 10-bit (x265)");
        assert_eq!(form.rf, 22);
        assert_eq!(form.speed, 5);
        assert_eq!(form.speed_label(), "medium");
        assert_eq!(form.quality_label(), "RF");
        assert_eq!(form.container, Container::Mkv);
        assert!(form.web_optimized && form.align_av && form.passthru_metadata);
        assert!(!form.keep_attachments);
    }

    #[test]
    fn speed_and_quality_labels_follow_the_encoder() {
        let mut form = TranscodeForm {
            encoder: 5,
            speed: 6,
            ..TranscodeForm::default()
        };
        assert_eq!(form.speed_label(), "preset 6");
        assert_eq!(form.quality_label(), "CRF");
        form.encoder = 9;
        form.speed = 2;
        assert_eq!(form.speed_label(), "p3");
        assert_eq!(form.quality_label(), "CQ");
    }

    #[test]
    fn quality_mapping_round_trips_through_core_labels() {
        let encoder = VideoEncoder::X265;
        for rf in [16u8, 20, 22, 26] {
            let quality = quality_from_rf(&encoder, rf);
            assert!(
                rf_from_quality(&encoder, quality).abs_diff(rf) <= 1,
                "rf {rf}"
            );
        }
        assert!(quality_from_rf(&encoder, 14) > quality_from_rf(&encoder, 28));
    }

    #[test]
    fn every_builtin_preset_loads_and_maps_back() {
        for preset in builtins(&kestrel_core::encoders::EncoderSet::default()) {
            let form = TranscodeForm::from_job_settings(&preset.name, &preset.settings);
            assert_eq!(form.preset, preset.name);
            let back = form.to_job_settings();
            assert_eq!(back.container, preset.settings.container, "{}", preset.id);
            if let (VideoSettings::Encode(want), VideoSettings::Encode(got)) =
                (&preset.settings.video, &back.video)
            {
                assert_eq!(got.encoder, want.encoder, "{}", preset.id);
                assert_eq!(got.max_height, want.max_height, "{}", preset.id);
                assert_eq!(got.tone_map_to_sdr, want.tone_map_to_sdr, "{}", preset.id);
                assert!(got.speed.abs_diff(want.speed) <= 1, "{}", preset.id);
                match (&got.rate, &want.rate) {
                    (RateControl::Quality(a), RateControl::Quality(b)) => {
                        assert!(a.abs_diff(*b) <= 4, "{} {a} {b}", preset.id);
                    }
                    (a, b) => assert_eq!(a, b),
                }
            }
            assert_eq!(
                back.audio.codec, preset.settings.audio.codec,
                "{}",
                preset.id
            );
            assert_eq!(back.subtitles.burn_in, preset.settings.subtitles.burn_in);
        }
    }

    #[test]
    fn archive_preset_selects_ten_bit_svt_av1() {
        let archive = kestrel_core::presets::builtin_preset("archive").unwrap();
        let form = TranscodeForm::from_job_settings(&archive.name, &archive.settings);
        assert_eq!(form.encoder_name(), "AV1 10-bit (SVT-AV1)");
        assert_eq!(form.container, Container::Mkv);
    }

    #[test]
    fn custom_crop_and_bitrate_map_to_core() {
        let form = TranscodeForm {
            crop_mode: 3,
            crop: [10, 20, 2, 4],
            rate_mode: 1,
            bitrate_kbps: 5000,
            frame_rate: 3,
            ..TranscodeForm::default()
        };
        let settings = form.to_job_settings();
        let VideoSettings::Encode(encode) = settings.video else {
            panic!("expected an encode");
        };
        assert_eq!(
            encode.crop,
            Crop::Custom {
                top: 10,
                bottom: 20,
                left: 2,
                right: 4
            }
        );
        assert_eq!(encode.rate, RateControl::Bitrate(5000));
        assert_eq!(encode.frame_rate, FrameRate::Fixed(25.0));
    }

    #[test]
    fn container_change_renames_the_output() {
        let mut form = TranscodeForm {
            save_as: "movie.mkv".to_owned(),
            ..TranscodeForm::default()
        };
        form.set_container(Container::Mp4);
        assert_eq!(form.save_as, "movie.mp4");
    }

    #[test]
    fn final_dimensions_apply_crop_and_limit() {
        let form = TranscodeForm {
            crop: [280, 280, 0, 0],
            res_limit: 3,
            ..TranscodeForm::default()
        };
        assert_eq!(
            form.final_dimensions(Some((3840, 2160))),
            "3840x2160 -> crop 3840x1600 -> 2592x1080 (2.40:1)"
        );
    }

    #[test]
    fn heuristic_estimate_shrinks_with_higher_rf() {
        let mut form = TranscodeForm {
            rf: 18,
            ..TranscodeForm::default()
        };
        let high = heuristic_size(&form, 10_000_000_000, Some(3600.0));
        form.rf = 28;
        let low = heuristic_size(&form, 10_000_000_000, Some(3600.0));
        assert!(low < high);
        assert!(heuristic_minutes(&form, Some(3600.0)) >= 1);
    }
}
