//! Transcode page view model.
//!
//! `TranscodeForm` holds every control shown on the Transcode tabs as plain data.
//! [`TranscodeForm::to_job_settings`] maps it onto `kestrel_core::transcode::JobSettings` and
//! [`TranscodeForm::apply_job_settings`] maps core settings back onto the form. The two are
//! inverse for every control the form carries, which the tests assert.

use std::path::PathBuf;

use kestrel_core::encoders::EncoderSet;
use kestrel_core::external::{ExternalKind, ExternalTrack};
use kestrel_core::formats::Container;
use kestrel_core::mkv::MediaMetadata;
use kestrel_core::transcode::{
    Anamorphic, AudioChoice, AudioEncoder, AudioMixdown, AudioSettings, AudioTrackSetting,
    BorderFill, BurnBehavior, ColorSpaceTarget, Crop, Deinterlace, DeinterlaceFilter,
    DeinterlacePreset, Denoise, DenoiseFilter, DenoiseTune, Detelecine, EncoderChoice,
    FilterStrength, FrameRate, FrameRateMode, HwCodec, HwVendor, JobSettings, MediaTags, Mixdown,
    PassthruCodec, RateControl, Rotation, SharpenFilter, SubtitleDefault, SubtitleSettings, TenBit,
    ToneMapOperator, TrackSelection, VideoEncode, VideoEncoder, VideoSettings, encoder_choices,
    encoder_presets, encoder_tunes, quality_control_label,
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
pub const ROTATION_VALUES: [Rotation; 4] = [
    Rotation::Deg0,
    Rotation::Deg90,
    Rotation::Deg180,
    Rotation::Deg270,
];
pub const CROP_MODES: [&str; 4] = ["Automatic", "Conservative", "None", "Custom"];
pub const CROP_CUSTOM: usize = 3;
pub const RES_LIMITS: [&str; 7] = [
    "None",
    "4320p 8K",
    "2160p 4K",
    "1080p HD",
    "720p HD",
    "576p PAL",
    "480p NTSC",
];
pub const RES_LIMIT_HEIGHTS: [Option<u32>; 7] = [
    None,
    Some(4320),
    Some(2160),
    Some(1080),
    Some(720),
    Some(576),
    Some(480),
];
pub const ANAMORPHIC: [&str; 3] = ["None", "Automatic", "Custom"];
pub const ANAMORPHIC_CUSTOM: usize = 2;
pub const MODULUS: [&str; 4] = ["2", "4", "8", "16"];
pub const MODULUS_VALUES: [u32; 4] = [2, 4, 8, 16];
pub const BORDER_FILLS: [&str; 5] = ["None", "Height", "Width", "Both", "Custom"];
pub const BORDER_CUSTOM: usize = 4;
pub const BORDER_COLORS: [&str; 3] = ["Black", "White", "Custom"];
pub const BORDER_COLOR_CUSTOM: usize = 2;
pub const DETELECINE: [&str; 2] = ["Off", "Default"];
pub const INTERLACE_DETECT: [&str; 2] = ["Off", "Default"];
pub const DEINTERLACE: [&str; 4] = ["Off", "Yadif", "Bwdif", "Decomb"];
pub const DEINTERLACE_FILTERS: [DeinterlaceFilter; 3] = [
    DeinterlaceFilter::Yadif,
    DeinterlaceFilter::Bwdif,
    DeinterlaceFilter::Decomb,
];
pub const DEINTERLACE_PRESETS: [&str; 3] = ["Default", "Fast", "Bob"];
pub const DEINTERLACE_PRESET_VALUES: [DeinterlacePreset; 3] = [
    DeinterlacePreset::Default,
    DeinterlacePreset::Fast,
    DeinterlacePreset::Bob,
];
pub const DENOISE: [&str; 3] = ["Off", "NLMeans", "HQDN3D"];
pub const DENOISE_FILTERS: [DenoiseFilter; 2] = [DenoiseFilter::NlMeans, DenoiseFilter::Hqdn3d];
pub const DENOISE_PRESETS: [&str; 3] = ["Light", "Medium", "Strong"];
pub const DENOISE_LEVELS: [Denoise; 3] = [Denoise::Light, Denoise::Medium, Denoise::Strong];
pub const DENOISE_TUNES: [&str; 7] = [
    "None",
    "Film",
    "Grain",
    "High motion",
    "Animation",
    "Tape",
    "Sprite",
];
pub const DENOISE_TUNE_VALUES: [DenoiseTune; 7] = [
    DenoiseTune::None,
    DenoiseTune::Film,
    DenoiseTune::Grain,
    DenoiseTune::HighMotion,
    DenoiseTune::Animation,
    DenoiseTune::Tape,
    DenoiseTune::Sprite,
];
/// Strength steps shared by chroma smooth, sharpen and deblock; index 0 is off.
pub const STRENGTHS: [&str; 7] = [
    "Off",
    "Ultralight",
    "Light",
    "Medium",
    "Strong",
    "Stronger",
    "Very strong",
];
pub const STRENGTH_VALUES: [FilterStrength; 7] = [
    FilterStrength::Off,
    FilterStrength::UltraLight,
    FilterStrength::Light,
    FilterStrength::Medium,
    FilterStrength::Strong,
    FilterStrength::Stronger,
    FilterStrength::VeryStrong,
];
/// Sharpen presets are the strength steps without "Off".
pub const SHARPEN_PRESETS: [&str; 6] = [
    "Ultralight",
    "Light",
    "Medium",
    "Strong",
    "Stronger",
    "Very strong",
];
pub const SHARPEN: [&str; 3] = ["Off", "Unsharp", "LapSharp"];
pub const SHARPEN_FILTERS: [SharpenFilter; 3] = [
    SharpenFilter::Off,
    SharpenFilter::Unsharp,
    SharpenFilter::LapSharp,
];
pub const COLOURSPACES: [&str; 4] = ["Keep", "BT.709", "BT.601", "BT.2020"];
pub const COLOURSPACE_VALUES: [ColorSpaceTarget; 4] = [
    ColorSpaceTarget::Keep,
    ColorSpaceTarget::Bt709,
    ColorSpaceTarget::Bt601,
    ColorSpaceTarget::Bt2020,
];
pub const HDR_MODES: [&str; 2] = ["Keep HDR10", "Tone-map to SDR"];
pub const TONEMAPS: [&str; 4] = ["Hable", "Mobius", "Reinhard", "BT.2390"];
pub const TONEMAP_VALUES: [ToneMapOperator; 4] = [
    ToneMapOperator::Hable,
    ToneMapOperator::Mobius,
    ToneMapOperator::Reinhard,
    ToneMapOperator::Bt2390,
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
pub const FPS_MODES: [&str; 4] = ["Auto", "Constant", "Peak", "Variable"];
pub const FPS_MODE_VALUES: [FrameRateMode; 4] = [
    FrameRateMode::Auto,
    FrameRateMode::Cfr,
    FrameRateMode::Pfr,
    FrameRateMode::Vfr,
];
pub const RATE_MODES: [&str; 2] = ["Constant quality", "Average bitrate"];

/// Audio encoders in display order. HE-AAC needs `libfdk_aac` and is listed only when found.
pub const AUDIO_ENCODERS: [(AudioEncoder, &str); 11] = [
    (AudioEncoder::Aac, "AAC (avcodec)"),
    (AudioEncoder::HeAac, "HE-AAC"),
    (AudioEncoder::Opus, "Opus"),
    (AudioEncoder::Ac3, "AC3"),
    (AudioEncoder::Eac3, "E-AC3"),
    (AudioEncoder::Flac, "FLAC 24-bit"),
    (AudioEncoder::Mp3, "MP3"),
    (AudioEncoder::Vorbis, "Vorbis"),
    (AudioEncoder::AutoPassthru, "Auto passthru"),
    (
        AudioEncoder::Passthru(PassthruCodec::TrueHd),
        "TrueHD passthru",
    ),
    (
        AudioEncoder::Passthru(PassthruCodec::DtsHd),
        "DTS-HD passthru",
    ),
];
pub const MIXDOWNS: [&str; 6] = [
    "Mono",
    "Stereo",
    "Dolby Pro Logic II",
    "5.1",
    "7.1",
    "Same as source",
];
pub const MIXDOWN_VALUES: [AudioMixdown; 6] = [
    AudioMixdown::Mono,
    AudioMixdown::Stereo,
    AudioMixdown::DolbyProLogicII,
    AudioMixdown::Surround51,
    AudioMixdown::Surround71,
    AudioMixdown::Source,
];
pub const MIXDOWN_SOURCE: usize = 5;
pub const SAMPLE_RATES: [&str; 3] = ["Auto", "44.1 kHz", "48 kHz"];
pub const SAMPLE_RATE_VALUES: [Option<u32>; 3] = [None, Some(44_100), Some(48_000)];
pub const AUDIO_BITRATES: [u32; 8] = [96, 128, 160, 192, 256, 320, 448, 640];
pub const AUDIO_SELECTION: [&str; 4] = [
    "First matching track",
    "All matching tracks",
    "All tracks",
    "None",
];
pub const AUDIO_SELECTION_VALUES: [TrackSelection; 4] = [
    TrackSelection::FirstMatching,
    TrackSelection::AllMatching,
    TrackSelection::All,
    TrackSelection::None,
];
pub const PASSTHRU_FALLBACKS: [&str; 4] = ["Automatic", "AAC (avcodec)", "AC3", "Opus"];
pub const PASSTHRU_FALLBACK_VALUES: [Option<AudioEncoder>; 4] = [
    None,
    Some(AudioEncoder::Aac),
    Some(AudioEncoder::Ac3),
    Some(AudioEncoder::Opus),
];
/// Same order as `PassthruCodec::ALL`.
pub const PASSTHRU_FORMATS: [&str; 9] = [
    "AAC", "AC3", "E-AC3", "DTS", "DTS-HD", "TrueHD", "FLAC", "MP3", "Opus",
];

pub const SUB_SELECTION: [&str; 4] = [
    "None",
    "All matching tracks",
    "First matching track",
    "All tracks",
];
pub const SUB_SELECTION_VALUES: [TrackSelection; 4] = [
    TrackSelection::None,
    TrackSelection::AllMatching,
    TrackSelection::FirstMatching,
    TrackSelection::All,
];
pub const SUB_BURN: [&str; 4] = [
    "None",
    "Foreign audio subtitle track",
    "First selected track",
    "Foreign audio, then first track",
];
pub const SUB_BURN_VALUES: [BurnBehavior; 4] = [
    BurnBehavior::None,
    BurnBehavior::ForeignAudio,
    BurnBehavior::FirstSelected,
    BurnBehavior::ForeignThenFirst,
];
pub const SUB_DEFAULT: [&str; 3] = ["First track", "None", "Forced track"];
pub const SUB_DEFAULT_VALUES: [SubtitleDefault; 3] = [
    SubtitleDefault::First,
    SubtitleDefault::None,
    SubtitleDefault::Forced,
];
pub const CHARSETS: [&str; 4] = ["UTF-8", "Windows-1252", "ISO-8859-1", "Shift_JIS"];

#[derive(Clone, Debug, PartialEq)]
pub struct AudioTrackForm {
    /// Position of the source track among the file's audio tracks.
    pub source_index: usize,
    pub source: String,
    pub source_meta: String,
    pub encoder: AudioEncoder,
    pub mixdown: usize,
    pub sample_rate: usize,
    pub bitrate: usize,
    pub gain: i32,
    pub drc: i32,
    pub name: Option<String>,
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
    /// Path of an imported external file, when this track is one. Rows without a path describe
    /// source tracks (or the foreign audio scan) and are chosen by the selection settings.
    pub external: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChapterRow {
    pub start: f64,
    pub duration: f64,
    pub name: String,
    /// Title stored in the source; a row whose name differs renames the chapter.
    pub original: String,
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

/// Which family an encoder belongs to; it decides labels and slider ranges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    X26x,
    Av1,
    Vp9,
    Hardware,
}

pub fn family_of(encoder: &VideoEncoder) -> Family {
    match encoder {
        VideoEncoder::X264 | VideoEncoder::X265 => Family::X26x,
        VideoEncoder::SvtAv1 | VideoEncoder::AomAv1 => Family::Av1,
        VideoEncoder::Vp9 => Family::Vp9,
        VideoEncoder::Hardware { .. } => Family::Hardware,
    }
}

/// The encoder's own constant-quality value a new selection starts at.
pub fn default_rf(encoder: &VideoEncoder) -> u8 {
    match family_of(encoder) {
        Family::X26x => 22,
        Family::Av1 => 30,
        Family::Vp9 => 31,
        Family::Hardware => 24,
    }
}

/// Largest native constant-quality value.
pub fn rf_max(encoder: &VideoEncoder) -> u8 {
    match family_of(encoder) {
        Family::Av1 | Family::Vp9 => 63,
        _ => 51,
    }
}

/// Encoder preset names ordered from fastest to slowest.
pub fn preset_options(encoder: &VideoEncoder) -> Vec<&'static str> {
    let mut list = encoder_presets(encoder).to_vec();
    if matches!(
        encoder,
        VideoEncoder::SvtAv1 | VideoEncoder::AomAv1 | VideoEncoder::Vp9
    ) {
        list.reverse();
    }
    list
}

/// The preset a fresh selection of this encoder starts at.
pub fn default_preset(encoder: &VideoEncoder) -> &'static str {
    let wanted = match encoder {
        VideoEncoder::X264 | VideoEncoder::X265 => "medium",
        VideoEncoder::SvtAv1 => "8",
        VideoEncoder::AomAv1 => "6",
        VideoEncoder::Vp9 => "3",
        VideoEncoder::Hardware {
            vendor: HwVendor::Nvenc,
            ..
        } => "p5",
        VideoEncoder::Hardware {
            vendor: HwVendor::Qsv,
            ..
        } => "medium",
        VideoEncoder::Hardware {
            vendor: HwVendor::Amf,
            ..
        } => "balanced",
        VideoEncoder::Hardware { .. } => "",
    };
    encoder_presets(encoder)
        .iter()
        .copied()
        .find(|name| *name == wanted)
        .unwrap_or("")
}

pub fn profile_names(encoder: &VideoEncoder) -> &'static [&'static str] {
    match encoder {
        VideoEncoder::X264 => &["baseline", "main", "high", "high10"],
        VideoEncoder::X265 => &["main", "main10", "main12"],
        VideoEncoder::SvtAv1 => &["main"],
        VideoEncoder::AomAv1 | VideoEncoder::Vp9 => &[],
        VideoEncoder::Hardware { codec, .. } => match codec {
            HwCodec::H264 => &["baseline", "main", "high"],
            HwCodec::Hevc => &["main", "main10"],
            HwCodec::Av1 => &["main"],
        },
    }
}

pub fn level_names(encoder: &VideoEncoder) -> &'static [&'static str] {
    const LEVELS: [&str; 8] = ["3.0", "3.1", "4.0", "4.1", "4.2", "5.0", "5.1", "5.2"];
    match encoder {
        VideoEncoder::X264 | VideoEncoder::X265 => &LEVELS,
        VideoEncoder::Hardware { codec, .. } if *codec != HwCodec::Av1 => &LEVELS,
        _ => &[],
    }
}

/// "Auto" followed by the names, for a select.
pub fn with_auto(names: &[&str], auto: &str) -> Vec<String> {
    std::iter::once(auto.to_owned())
        .chain(names.iter().map(|name| (*name).to_owned()))
        .collect()
}

/// Selected position in a `with_auto` list: 0 when `value` is empty or unknown.
pub fn auto_index(names: &[&str], value: &str) -> usize {
    names
        .iter()
        .position(|name| *name == value)
        .map_or(0, |index| index + 1)
}

/// The audio encoders to offer, hiding those the FFmpeg build cannot run.
pub fn audio_encoder_options(
    set: Option<&EncoderSet>,
    current: Option<AudioEncoder>,
) -> Vec<(AudioEncoder, &'static str)> {
    AUDIO_ENCODERS
        .iter()
        .copied()
        .filter(|(encoder, _)| {
            *encoder != AudioEncoder::HeAac
                || Some(*encoder) == current
                || set.is_some_and(|set| set.has_audio_encoder("libfdk_aac"))
        })
        .collect()
}

fn audio_choice(encoder: AudioEncoder) -> AudioChoice {
    match encoder {
        AudioEncoder::AutoPassthru | AudioEncoder::Passthru(_) => AudioChoice::Passthrough,
        AudioEncoder::Opus => AudioChoice::Opus,
        AudioEncoder::Ac3 => AudioChoice::Ac3,
        AudioEncoder::Eac3 => AudioChoice::Eac3,
        AudioEncoder::Flac => AudioChoice::Flac,
        AudioEncoder::Aac | AudioEncoder::HeAac | AudioEncoder::Mp3 | AudioEncoder::Vorbis => {
            AudioChoice::Aac
        }
    }
}

fn audio_encoder_for(choice: &AudioChoice) -> AudioEncoder {
    AudioEncoder::from(choice)
}

fn mixdown_index(mixdown: AudioMixdown) -> usize {
    MIXDOWN_VALUES
        .iter()
        .position(|value| *value == mixdown)
        .unwrap_or(MIXDOWN_SOURCE)
}

fn legacy_mixdown(mixdown: Mixdown) -> AudioMixdown {
    AudioMixdown::from(mixdown)
}

fn legacy_mixdown_from(mixdown: AudioMixdown) -> Mixdown {
    match mixdown {
        AudioMixdown::Mono | AudioMixdown::Stereo | AudioMixdown::DolbyProLogicII => {
            Mixdown::Stereo
        }
        AudioMixdown::Surround51 | AudioMixdown::Surround71 => Mixdown::Surround51,
        AudioMixdown::Source => Mixdown::Source,
    }
}

fn bitrate_index(kbps: u32) -> usize {
    AUDIO_BITRATES
        .iter()
        .position(|rate| *rate >= kbps)
        .unwrap_or(AUDIO_BITRATES.len() - 1)
}

/// Lowercase ISO 639-2 code for the two- and three-letter codes in language fields.
pub fn language_code(text: &str) -> Option<String> {
    let text = text.trim().to_ascii_lowercase();
    if text.is_empty() || text == "und" {
        return None;
    }
    const TWO_LETTER: [(&str, &str); 24] = [
        ("en", "eng"),
        ("es", "spa"),
        ("fr", "fra"),
        ("de", "deu"),
        ("it", "ita"),
        ("ja", "jpn"),
        ("ko", "kor"),
        ("zh", "zho"),
        ("ru", "rus"),
        ("pt", "por"),
        ("ar", "ara"),
        ("hi", "hin"),
        ("nl", "nld"),
        ("pl", "pol"),
        ("tr", "tur"),
        ("sv", "swe"),
        ("no", "nor"),
        ("da", "dan"),
        ("fi", "fin"),
        ("cs", "ces"),
        ("el", "ell"),
        ("he", "heb"),
        ("th", "tha"),
        ("uk", "ukr"),
    ];
    if text.len() == 2 {
        return TWO_LETTER
            .iter()
            .find(|(short, _)| *short == text)
            .map(|(_, long)| (*long).to_owned());
    }
    (text.len() == 3 && text.chars().all(|ch| ch.is_ascii_lowercase())).then_some(text)
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
    /// Top, bottom, left, right.
    pub crop: [u32; 4],
    pub res_limit: usize,
    pub anamorphic: usize,
    pub pixel_aspect: String,
    pub modulus: usize,
    /// Size box used when `optimal_size` is off; otherwise shown from the source.
    pub scaled_width: u32,
    pub scaled_height: u32,
    pub optimal_size: bool,
    pub allow_upscale: bool,
    pub border_fill: usize,
    /// Top, bottom, left, right.
    pub border: [u32; 4],
    pub border_color: usize,
    pub border_color_custom: String,
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
    pub encoder: VideoEncoder,
    pub ten_bit: TenBit,
    pub frame_rate: usize,
    pub fps_mode: usize,
    pub rate_mode: usize,
    /// The encoder's own constant-quality value (CRF, CQ, global quality, QP).
    pub rf: u8,
    pub bitrate_kbps: u32,
    pub multipass: bool,
    pub turbo: bool,
    /// Encoder preset name; empty for encoders without presets.
    pub preset_name: String,
    /// Empty means none.
    pub tune: String,
    /// Empty means automatic.
    pub profile: String,
    pub level: String,
    pub fast_decode: bool,
    pub advanced: String,
    // Audio
    pub audio_tracks: Vec<AudioTrackForm>,
    pub audio_selection: usize,
    pub languages: String,
    pub passthru_fallback: usize,
    pub passthru_allowed: [bool; 9],
    /// Defaults for tracks added later; a loaded preset sets these.
    pub default_audio_encoder: AudioEncoder,
    pub default_audio_mixdown: usize,
    pub default_audio_bitrate: usize,
    // Subtitles
    pub sub_tracks: Vec<SubTrackForm>,
    pub sub_selection: usize,
    pub sub_burn: usize,
    pub sub_forced_only: bool,
    pub sub_default: usize,
    pub charset: usize,
    pub foreign_scan: bool,
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
        let encoder = VideoEncoder::X265;
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
            border: [0; 4],
            border_color: 0,
            border_color_custom: "#000000".to_owned(),
            detelecine: 0,
            interlace_detect: 1,
            deinterlace: 0,
            deinterlace_preset: 0,
            denoise: 0,
            denoise_preset: 0,
            denoise_tune: 0,
            chroma_smooth: 0,
            sharpen: 0,
            sharpen_preset: 2,
            deblock: 0,
            colourspace: 0,
            hdr: 0,
            tonemap: 0,
            grayscale: false,
            rf: default_rf(&encoder),
            preset_name: default_preset(&encoder).to_owned(),
            encoder,
            ten_bit: TenBit::On,
            frame_rate: 0,
            fps_mode: 0,
            rate_mode: 0,
            bitrate_kbps: 6000,
            multipass: true,
            turbo: true,
            tune: String::new(),
            profile: String::new(),
            level: String::new(),
            fast_decode: false,
            advanced: String::new(),
            audio_tracks: Vec::new(),
            audio_selection: 1,
            languages: String::new(),
            passthru_fallback: 0,
            passthru_allowed: [true; 9],
            default_audio_encoder: AudioEncoder::AutoPassthru,
            default_audio_mixdown: MIXDOWN_SOURCE,
            default_audio_bitrate: 2,
            sub_tracks: Vec::new(),
            sub_selection: 1,
            sub_burn: 0,
            sub_forced_only: false,
            sub_default: 0,
            charset: 0,
            foreign_scan: true,
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

impl TranscodeForm {
    pub fn encoder_family(&self) -> Family {
        family_of(&self.encoder)
    }

    pub fn is_constant_quality(&self) -> bool {
        self.rate_mode == 0
    }

    // ----- encoder picker -----

    /// The encoder list to offer: every choice FFmpeg provides, plus the current one so the
    /// picker always shows what the form holds.
    pub fn encoder_options(&self, set: Option<&EncoderSet>) -> Vec<EncoderChoice> {
        let mut options: Vec<EncoderChoice> = encoder_choices()
            .into_iter()
            .filter(|choice| set.is_some_and(|set| set.supports(&choice.encoder)))
            .collect();
        if self.encoder_index(&options).is_none() {
            let available = set.is_some_and(|set| set.supports(&self.encoder));
            let label = match (available, self.ten_bit) {
                (true, _) => self.encoder.label(),
                (false, _) => format!("{} (not available)", self.encoder.label()),
            };
            options.push(EncoderChoice {
                label,
                encoder: self.encoder.clone(),
                ten_bit: self.ten_bit,
            });
        }
        options
    }

    /// Position of the form's encoder and bit depth in `options`.
    pub fn encoder_index(&self, options: &[EncoderChoice]) -> Option<usize> {
        options
            .iter()
            .position(|choice| choice.encoder == self.encoder && choice.ten_bit == self.ten_bit)
    }

    /// Switches to a picked encoder, keeping what carries over (relative preset speed, tune,
    /// profile and level when the new encoder knows them) and resetting what does not.
    pub fn set_encoder(&mut self, choice: &EncoderChoice) {
        let old = std::mem::replace(&mut self.encoder, choice.encoder.clone());
        self.ten_bit = choice.ten_bit;
        if old == self.encoder {
            return;
        }
        let old_presets = preset_options(&old);
        let new_presets = preset_options(&self.encoder);
        self.preset_name = match old_presets
            .iter()
            .position(|name| *name == self.preset_name)
            .filter(|_| old_presets.len() > 1 && new_presets.len() > 1)
        {
            Some(position) => {
                let fraction = position as f64 / (old_presets.len() - 1) as f64;
                let target = (fraction * (new_presets.len() - 1) as f64).round() as usize;
                new_presets[target].to_owned()
            }
            None => default_preset(&self.encoder).to_owned(),
        };
        if family_of(&old) != family_of(&self.encoder) {
            self.rf = default_rf(&self.encoder);
        }
        self.rf = self.rf.min(rf_max(&self.encoder));
        if !encoder_tunes(&self.encoder).contains(&self.tune.as_str()) {
            self.tune.clear();
        }
        if !profile_names(&self.encoder).contains(&self.profile.as_str()) {
            self.profile.clear();
        }
        if !level_names(&self.encoder).contains(&self.level.as_str()) {
            self.level.clear();
        }
        if !self.encoder.supports_fast_decode() {
            self.fast_decode = false;
        }
        if !self.encoder.supports_advanced_options() {
            self.advanced.clear();
        }
    }

    /// Slider position (0 to 1) of the preset within the encoder's list.
    pub fn preset_fraction(&self) -> f32 {
        let presets = preset_options(&self.encoder);
        match presets.iter().position(|name| *name == self.preset_name) {
            Some(position) if presets.len() > 1 => position as f32 / (presets.len() - 1) as f32,
            _ => 0.5,
        }
    }

    pub fn set_preset_fraction(&mut self, fraction: f32) {
        let presets = preset_options(&self.encoder);
        if presets.len() > 1 {
            let index = (fraction.clamp(0.0, 1.0) * (presets.len() - 1) as f32).round() as usize;
            self.preset_name = presets[index].to_owned();
        }
    }

    pub fn has_presets(&self) -> bool {
        !encoder_presets(&self.encoder).is_empty()
    }

    /// Label for the constant-quality value: CQ for hardware, CRF for the rest.
    pub fn quality_label(&self) -> &'static str {
        match self.encoder_family() {
            Family::Hardware => "CQ",
            _ => "CRF",
        }
    }

    pub fn quality_hint(&self) -> &'static str {
        if self.encoder_family() == Family::Hardware {
            return "Hardware encoders need a lower value than x264 or x265 for the same quality.";
        }
        let (high, good) = match self.encoder_family() {
            Family::Av1 | Family::Vp9 => (24, 34),
            _ => (18, 24),
        };
        if self.rf <= high {
            "Visually lossless for most sources. Large files."
        } else if self.rf <= good {
            "Recommended range for HD. Good balance of size and detail."
        } else {
            "Small files. Visible loss on fine grain and dark scenes."
        }
    }

    /// Preset name shown next to the slider.
    pub fn speed_label(&self) -> String {
        if self.preset_name.is_empty() {
            "automatic".to_owned()
        } else {
            self.preset_name.clone()
        }
    }

    /// Short summary such as `H.265 10-bit · CRF 22`.
    pub fn video_summary(&self) -> String {
        let depth = match self.ten_bit {
            TenBit::On => " 10-bit",
            TenBit::Twelve => " 12-bit",
            _ => "",
        };
        let name = match &self.encoder {
            VideoEncoder::X264 => "H.264".to_owned(),
            VideoEncoder::X265 => "H.265".to_owned(),
            VideoEncoder::SvtAv1 => "AV1 SVT".to_owned(),
            VideoEncoder::AomAv1 => "AV1 libaom".to_owned(),
            VideoEncoder::Vp9 => "VP9".to_owned(),
            VideoEncoder::Hardware { .. } => {
                let label = self.encoder.label();
                match label.split_once(" (") {
                    Some((codec, vendor)) => format!("{codec} {}", vendor.trim_end_matches(')')),
                    None => label,
                }
            }
        };
        if self.is_constant_quality() {
            format!("{name}{depth} \u{b7} {} {}", self.quality_label(), self.rf)
        } else {
            format!("{name}{depth} \u{b7} {} kb/s", self.bitrate_kbps)
        }
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
                let encoder = AUDIO_ENCODERS
                    .iter()
                    .find(|(encoder, _)| *encoder == track.encoder)
                    .map_or("?", |(_, label)| *label)
                    .replace(" passthru", "");
                format!(
                    "{encoder} {}",
                    MIXDOWNS[track.mixdown.min(MIXDOWNS.len() - 1)]
                )
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

    // ----- dimensions -----

    /// Source size after the custom crop, if the source size is known.
    fn cropped(&self, source: (u32, u32)) -> (u32, u32) {
        if self.crop_mode == CROP_CUSTOM {
            (
                source.0.saturating_sub(self.crop[2] + self.crop[3]).max(2),
                source.1.saturating_sub(self.crop[0] + self.crop[1]).max(2),
            )
        } else {
            source
        }
    }

    /// The size box in effect: the resolution limit, or the scaled fields when optimal size is off.
    fn size_box(&self) -> (Option<u32>, Option<u32>) {
        if self.optimal_size {
            (
                None,
                RES_LIMIT_HEIGHTS[self.res_limit.min(RES_LIMIT_HEIGHTS.len() - 1)],
            )
        } else {
            (
                Some(self.scaled_width).filter(|value| *value > 0),
                Some(self.scaled_height).filter(|value| *value > 0),
            )
        }
    }

    /// Output size after crop, resolution limit or size box, and modulus.
    pub fn output_size(&self, source: (u32, u32)) -> (u32, u32) {
        let (width, height) = self.cropped(source);
        let modulus = MODULUS_VALUES[self.modulus.min(MODULUS_VALUES.len() - 1)];
        let (max_width, max_height) = self.size_box();
        let mut scale = f64::MAX;
        if let Some(limit) = max_width {
            scale = scale.min(f64::from(limit) / f64::from(width));
        }
        if let Some(limit) = max_height {
            scale = scale.min(f64::from(limit) / f64::from(height));
        }
        if scale == f64::MAX {
            scale = 1.0;
        } else if !self.allow_upscale {
            scale = scale.min(1.0);
        }
        let round =
            |value: f64| ((value / f64::from(modulus)).floor() as u32 * modulus).max(modulus);
        (
            round(f64::from(width) * scale),
            round(f64::from(height) * scale),
        )
    }

    /// Puts the size computed from the source into the scaled width and height fields.
    pub fn seed_scaled_size(&mut self, source: Option<(u32, u32)>) {
        if let Some(source) = source {
            (self.scaled_width, self.scaled_height) = self.output_size(source);
        }
    }

    /// Output size in pixels after crop, limit and scaling, for the dimension summary.
    pub fn final_dimensions(&self, source: Option<(u32, u32)>) -> String {
        let Some((width, height)) = source else {
            return "Source dimensions unknown".to_owned();
        };
        let cropped = self.cropped((width, height));
        let target = self.output_size((width, height));
        let ratio = f64::from(target.0) / f64::from(target.1.max(1));
        format!(
            "{width}x{height} -> crop {}x{} -> {}x{} ({ratio:.2}:1)",
            cropped.0, cropped.1, target.0, target.1
        )
    }

    // ----- sources -----

    /// Output row for source audio track `index`, using the default encoder settings.
    pub fn source_audio_track(&self, meta: &MediaMetadata, index: usize) -> Option<AudioTrackForm> {
        let track = meta.audio_tracks.get(index)?;
        let info = format!(
            "{} {}",
            codec_label(&track.codec),
            channel_label(track.channels)
        );
        Some(AudioTrackForm {
            source_index: index,
            source: format!("{} \u{b7} {} {}", index + 1, track.language, info.trim()),
            source_meta: track.title.clone().unwrap_or_default(),
            encoder: self.default_audio_encoder,
            mixdown: self.default_audio_mixdown,
            sample_rate: 0,
            bitrate: self.default_audio_bitrate,
            gain: 0,
            drc: 0,
            name: None,
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

    /// Fills the track, chapter, size and output fields from a newly selected source.
    pub fn seed_from_source(&mut self, meta: &MediaMetadata, stem: &str, folder: &str) {
        self.audio_tracks = (0..meta.audio_tracks.len())
            .filter_map(|index| self.source_audio_track(meta, index))
            .collect();
        let imported: Vec<SubTrackForm> = self
            .sub_tracks
            .iter()
            .filter(|track| track.external.is_some())
            .cloned()
            .collect();
        self.sub_tracks = Vec::new();
        if self.foreign_scan {
            self.sub_tracks.push(SubTrackForm {
                source: "Foreign audio scan".to_owned(),
                kind: "search".to_owned(),
                language: "und".to_owned(),
                forced_only: true,
                burn: matches!(self.sub_burn, 1 | 3),
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
        self.sub_tracks.extend(imported);
        let dims = meta
            .video
            .as_ref()
            .and_then(|video| Some((video.width?, video.height?)));
        self.seed_scaled_size(dims);
        self.tag_title = stem.to_owned();
        self.save_as = format!("{stem}.{}", self.container.extension());
        if self.folder.is_empty() {
            self.folder = folder.to_owned();
        }
    }

    // ----- chapters -----

    /// Chapter titles that replace the source titles; empty when no row was renamed.
    pub fn chapter_names(&self) -> Vec<String> {
        if self.create_chapters
            && self
                .chapters
                .iter()
                .any(|chapter| chapter.name != chapter.original)
        {
            self.chapters
                .iter()
                .map(|chapter| chapter.name.clone())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Applies imported chapter names to the rows in order. The count must match the source.
    pub fn apply_chapter_names(&mut self, names: Vec<String>) -> Result<(), String> {
        if names.len() != self.chapters.len() {
            return Err(format!(
                "The file lists {} chapters but the source has {}",
                names.len(),
                self.chapters.len()
            ));
        }
        for (row, name) in self.chapters.iter_mut().zip(names) {
            row.name = name;
        }
        Ok(())
    }

    // ----- core mapping -----

    fn external_tracks(&self) -> Vec<ExternalTrack> {
        let charset = (self.charset > 0).then(|| CHARSETS[self.charset].to_owned());
        self.sub_tracks
            .iter()
            .filter_map(|row| {
                let path = row.external.clone()?;
                Some(ExternalTrack {
                    path,
                    kind: ExternalKind::Subtitle,
                    language: language_code(&row.language),
                    title: None,
                    default: row.default,
                    forced: row.forced_only,
                    offset_ms: i64::from(row.offset_ms),
                    charset: charset.clone(),
                    burn_in: row.burn,
                })
            })
            .collect()
    }

    fn video_encode(&self) -> VideoEncode {
        let rate = if self.is_constant_quality() {
            RateControl::Native(f32::from(self.rf))
        } else {
            RateControl::Bitrate(self.bitrate_kbps)
        };
        let frame_rate =
            match FRAME_RATES[self.frame_rate.min(FRAME_RATES.len() - 1)].parse::<f64>() {
                Ok(fps) => FrameRate::Fixed(fps),
                Err(_) => FrameRate::Source,
            };
        let (max_width, max_height) = self.size_box();
        let [top, bottom, left, right] = self.crop;
        let crop = match self.crop_mode {
            0 => Crop::Automatic,
            1 => Crop::Conservative,
            CROP_CUSTOM => Crop::Custom {
                top,
                bottom,
                left,
                right,
            },
            _ => Crop::None,
        };
        let anamorphic = match self.anamorphic {
            0 => Anamorphic::None,
            ANAMORPHIC_CUSTOM => {
                let (num, den) = parse_aspect(&self.pixel_aspect).unwrap_or((1, 1));
                Anamorphic::Custom { num, den }
            }
            _ => Anamorphic::Automatic,
        };
        let [border_top, border_bottom, border_left, border_right] = self.border;
        let border_fill = match self.border_fill {
            1 => BorderFill::Height,
            2 => BorderFill::Width,
            3 => BorderFill::Both,
            BORDER_CUSTOM => BorderFill::Custom {
                top: border_top,
                bottom: border_bottom,
                left: border_left,
                right: border_right,
            },
            _ => BorderFill::None,
        };
        let border_color = match self.border_color {
            0 => "black".to_owned(),
            1 => "white".to_owned(),
            _ => self.border_color_custom.trim().to_owned(),
        };
        let (deinterlace, interlace_detection) = match (self.deinterlace, self.interlace_detect) {
            (0, detect) => (Deinterlace::Off, detect != 0),
            (_, 0) => (Deinterlace::On, false),
            _ => (Deinterlace::Auto, true),
        };
        let presets = encoder_presets(&self.encoder);
        let encoder_preset = presets
            .contains(&self.preset_name.as_str())
            .then(|| self.preset_name.clone());
        let tune = encoder_tunes(&self.encoder)
            .contains(&self.tune.as_str())
            .then(|| self.tune.clone());
        let named = |value: &str| (!value.is_empty()).then(|| value.to_owned());
        VideoEncode {
            encoder: self.encoder.clone(),
            rate,
            max_height,
            max_width,
            frame_rate,
            deinterlace,
            crop,
            tone_map_to_sdr: self.hdr == 1,
            ten_bit: self.ten_bit,
            rotation: ROTATION_VALUES[self.rotation.min(ROTATION_VALUES.len() - 1)],
            flip_horizontal: self.flip == 1,
            anamorphic,
            modulus: MODULUS_VALUES[self.modulus.min(MODULUS_VALUES.len() - 1)],
            allow_upscaling: self.allow_upscale,
            border_fill,
            border_color,
            detelecine: if self.detelecine == 0 {
                Detelecine::Off
            } else {
                Detelecine::Default
            },
            deinterlace_filter: match self.deinterlace {
                0 => DeinterlaceFilter::default(),
                index => DEINTERLACE_FILTERS[(index - 1).min(2)],
            },
            deinterlace_preset: DEINTERLACE_PRESET_VALUES[self
                .deinterlace_preset
                .min(DEINTERLACE_PRESET_VALUES.len() - 1)],
            interlace_detection,
            denoise: if self.denoise == 0 {
                Denoise::Off
            } else {
                DENOISE_LEVELS[self.denoise_preset.min(DENOISE_LEVELS.len() - 1)]
            },
            denoise_filter: match self.denoise {
                0 => DenoiseFilter::default(),
                index => DENOISE_FILTERS[(index - 1).min(1)],
            },
            denoise_tune: DENOISE_TUNE_VALUES[self.denoise_tune.min(DENOISE_TUNE_VALUES.len() - 1)],
            chroma_smooth: STRENGTH_VALUES[self.chroma_smooth.min(STRENGTH_VALUES.len() - 1)],
            sharpen: SHARPEN_FILTERS[self.sharpen.min(SHARPEN_FILTERS.len() - 1)],
            sharpen_preset: STRENGTH_VALUES
                [(self.sharpen_preset + 1).min(STRENGTH_VALUES.len() - 1)],
            deblock: STRENGTH_VALUES[self.deblock.min(STRENGTH_VALUES.len() - 1)],
            colorspace: COLOURSPACE_VALUES[self.colourspace.min(COLOURSPACE_VALUES.len() - 1)],
            grayscale: self.grayscale,
            tone_map_operator: TONEMAP_VALUES[self.tonemap.min(TONEMAP_VALUES.len() - 1)],
            frame_rate_mode: FPS_MODE_VALUES[self.fps_mode.min(FPS_MODE_VALUES.len() - 1)],
            passes: if self.multipass
                && !self.is_constant_quality()
                && self.encoder.supports_multi_pass()
            {
                2
            } else {
                1
            },
            turbo_first_pass: self.turbo,
            encoder_preset,
            tune,
            profile: named(&self.profile)
                .filter(|p| profile_names(&self.encoder).contains(&p.as_str())),
            level: named(&self.level).filter(|l| level_names(&self.encoder).contains(&l.as_str())),
            fast_decode: self.fast_decode && self.encoder.supports_fast_decode(),
            advanced_options: if self.encoder.supports_advanced_options() {
                self.advanced.trim().to_owned()
            } else {
                String::new()
            },
            ..VideoEncode::default()
        }
    }

    /// Current form as core job settings.
    pub fn to_job_settings(&self) -> JobSettings {
        let languages = parse_languages(&self.languages);
        let tracks: Vec<AudioTrackSetting> = self
            .audio_tracks
            .iter()
            .map(|track| AudioTrackSetting {
                source: track.source_index,
                encoder: track.encoder,
                mixdown: MIXDOWN_VALUES[track.mixdown.min(MIXDOWN_VALUES.len() - 1)],
                sample_rate: SAMPLE_RATE_VALUES
                    [track.sample_rate.min(SAMPLE_RATE_VALUES.len() - 1)],
                bitrate_kbps: Some(AUDIO_BITRATES[track.bitrate.min(AUDIO_BITRATES.len() - 1)]),
                gain_db: track.gain as f32,
                drc: track.drc as f32,
                name: track.name.clone(),
            })
            .collect();
        let allowed_passthru: Vec<PassthruCodec> = PassthruCodec::ALL
            .iter()
            .zip(self.passthru_allowed)
            .filter_map(|(codec, allowed)| allowed.then_some(*codec))
            .collect();
        let mut subtitles = SubtitleSettings {
            languages,
            include: self.sub_selection != 0,
            selection: SUB_SELECTION_VALUES[self.sub_selection.min(SUB_SELECTION_VALUES.len() - 1)],
            forced_only: self.sub_forced_only,
            default_track: SUB_DEFAULT_VALUES[self.sub_default.min(SUB_DEFAULT_VALUES.len() - 1)],
            external: self.external_tracks(),
            ..SubtitleSettings::default()
        };
        subtitles.set_burn_behavior(SUB_BURN_VALUES[self.sub_burn.min(SUB_BURN_VALUES.len() - 1)]);
        let text = |value: &str| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_owned())
        };
        JobSettings {
            container: self.container,
            video: VideoSettings::Encode(self.video_encode()),
            audio: AudioSettings {
                languages: parse_languages(&self.languages),
                codec: audio_choice(self.default_audio_encoder),
                bitrate_kbps: AUDIO_BITRATES
                    [self.default_audio_bitrate.min(AUDIO_BITRATES.len() - 1)],
                mixdown: legacy_mixdown_from(
                    MIXDOWN_VALUES[self.default_audio_mixdown.min(MIXDOWN_VALUES.len() - 1)],
                ),
                tracks,
                selection: AUDIO_SELECTION_VALUES
                    [self.audio_selection.min(AUDIO_SELECTION_VALUES.len() - 1)],
                allowed_passthru,
                fallback_encoder: PASSTHRU_FALLBACK_VALUES[self
                    .passthru_fallback
                    .min(PASSTHRU_FALLBACK_VALUES.len() - 1)],
            },
            subtitles,
            keep_chapters: self.create_chapters,
            keep_metadata: self.passthru_metadata,
            title: text(&self.tag_title),
            web_optimized: self.web_optimized,
            align_av_start: self.align_av,
            keep_attachments: self.keep_attachments,
            chapter_names: self.chapter_names(),
            tags: MediaTags {
                actors: text(&self.tag_actors),
                director: text(&self.tag_director),
                release_date: text(&self.tag_date),
                genre: text(&self.tag_genre),
                description: text(&self.tag_description),
                comment: text(&self.tag_comment),
            },
            track_edits: Default::default(),
        }
    }

    /// Loads core settings (a preset or saved job) onto the form. Controls the settings do not
    /// carry (source-bound rows, output names) keep their values.
    pub fn apply_job_settings(&mut self, name: &str, settings: &JobSettings) {
        self.preset = name.to_owned();
        self.container = settings.container;
        if let VideoSettings::Encode(encode) = &settings.video {
            self.apply_video(encode);
        }
        self.apply_audio(&settings.audio);
        self.apply_subtitles(&settings.subtitles);
        self.create_chapters = settings.keep_chapters;
        self.passthru_metadata = settings.keep_metadata;
        self.web_optimized = settings.web_optimized;
        self.align_av = settings.align_av_start;
        self.keep_attachments = settings.keep_attachments;
        if !settings.chapter_names.is_empty() {
            for (row, name) in self.chapters.iter_mut().zip(&settings.chapter_names) {
                row.name.clone_from(name);
            }
        }
        if let Some(title) = &settings.title {
            self.tag_title.clone_from(title);
        }
        let tags = &settings.tags;
        let text = |value: &Option<String>| value.clone().unwrap_or_default();
        self.tag_actors = text(&tags.actors);
        self.tag_director = text(&tags.director);
        self.tag_date = text(&tags.release_date);
        self.tag_genre = text(&tags.genre);
        self.tag_description = text(&tags.description);
        self.tag_comment = text(&tags.comment);
        self.save_as = replace_extension(&self.save_as, self.container.extension());
    }

    fn apply_video(&mut self, encode: &VideoEncode) {
        self.encoder = encode.encoder.clone();
        self.ten_bit = encode.ten_bit;
        match &encode.rate {
            RateControl::Quality(quality) => {
                self.rate_mode = 0;
                self.rf = rf_from_quality(&encode.encoder, *quality);
            }
            RateControl::Native(value) => {
                self.rate_mode = 0;
                self.rf = value.round().clamp(0.0, f32::from(rf_max(&encode.encoder))) as u8;
            }
            RateControl::Bitrate(kbps) => {
                self.rate_mode = 1;
                self.bitrate_kbps = *kbps;
            }
        }
        self.multipass = encode.passes >= 2;
        self.turbo = encode.turbo_first_pass;
        self.preset_name = match &encode.encoder_preset {
            Some(name) => name.clone(),
            None => preset_for_speed(&encode.encoder, encode.speed),
        };
        self.tune = encode.tune.clone().unwrap_or_default();
        self.profile = encode.profile.clone().unwrap_or_default();
        self.level = encode.level.clone().unwrap_or_default();
        self.fast_decode = encode.fast_decode;
        self.advanced.clone_from(&encode.advanced_options);

        // Dimensions
        self.flip = usize::from(encode.flip_horizontal);
        self.rotation = ROTATION_VALUES
            .iter()
            .position(|rotation| *rotation == encode.rotation)
            .unwrap_or(0);
        match &encode.crop {
            Crop::Automatic => self.crop_mode = 0,
            Crop::Conservative => self.crop_mode = 1,
            Crop::None => {
                self.crop_mode = 2;
                self.crop = [0; 4];
            }
            Crop::Custom {
                top,
                bottom,
                left,
                right,
            } => {
                self.crop_mode = CROP_CUSTOM;
                self.crop = [*top, *bottom, *left, *right];
            }
        }
        match (encode.max_width, encode.max_height) {
            (None, height) => {
                self.optimal_size = true;
                self.res_limit = RES_LIMIT_HEIGHTS
                    .iter()
                    .position(|candidate| *candidate == height)
                    .unwrap_or(0);
            }
            (width, height) => {
                self.optimal_size = false;
                self.scaled_width = width.unwrap_or(0);
                self.scaled_height = height.unwrap_or(0);
            }
        }
        match encode.anamorphic {
            Anamorphic::None => self.anamorphic = 0,
            Anamorphic::Automatic => self.anamorphic = 1,
            Anamorphic::Custom { num, den } => {
                self.anamorphic = ANAMORPHIC_CUSTOM;
                self.pixel_aspect = format!("{num} : {den}");
            }
        }
        self.modulus = MODULUS_VALUES
            .iter()
            .position(|value| *value == encode.modulus)
            .unwrap_or(0);
        self.allow_upscale = encode.allow_upscaling;
        match &encode.border_fill {
            BorderFill::None => self.border_fill = 0,
            BorderFill::Height => self.border_fill = 1,
            BorderFill::Width => self.border_fill = 2,
            BorderFill::Both => self.border_fill = 3,
            BorderFill::Custom {
                top,
                bottom,
                left,
                right,
            } => {
                self.border_fill = BORDER_CUSTOM;
                self.border = [*top, *bottom, *left, *right];
            }
        }
        match encode.border_color.as_str() {
            "black" => self.border_color = 0,
            "white" => self.border_color = 1,
            other => {
                self.border_color = BORDER_COLOR_CUSTOM;
                self.border_color_custom = other.to_owned();
            }
        }

        // Filters
        self.detelecine = usize::from(encode.detelecine != Detelecine::Off);
        self.deinterlace_preset = DEINTERLACE_PRESET_VALUES
            .iter()
            .position(|value| *value == encode.deinterlace_preset)
            .unwrap_or(0);
        let filter_index = DEINTERLACE_FILTERS
            .iter()
            .position(|value| *value == encode.deinterlace_filter)
            .map_or(1, |index| index + 1);
        match encode.deinterlace {
            Deinterlace::Off => {
                self.deinterlace = 0;
                self.interlace_detect = usize::from(encode.interlace_detection);
            }
            Deinterlace::Auto => {
                self.deinterlace = filter_index;
                self.interlace_detect = 1;
            }
            Deinterlace::On => {
                self.deinterlace = filter_index;
                self.interlace_detect = 0;
            }
        }
        let denoise_filter = DENOISE_FILTERS
            .iter()
            .position(|value| *value == encode.denoise_filter)
            .map_or(1, |index| index + 1);
        match encode.denoise {
            Denoise::Off => self.denoise = 0,
            level => {
                self.denoise = denoise_filter;
                self.denoise_preset = DENOISE_LEVELS
                    .iter()
                    .position(|value| *value == level)
                    .unwrap_or(0);
            }
        }
        self.denoise_tune = DENOISE_TUNE_VALUES
            .iter()
            .position(|value| *value == encode.denoise_tune)
            .unwrap_or(0);
        let strength = |value: FilterStrength| {
            STRENGTH_VALUES
                .iter()
                .position(|candidate| *candidate == value)
                .unwrap_or(0)
        };
        self.chroma_smooth = strength(encode.chroma_smooth);
        self.sharpen = SHARPEN_FILTERS
            .iter()
            .position(|value| *value == encode.sharpen)
            .unwrap_or(0);
        self.sharpen_preset = strength(encode.sharpen_preset).saturating_sub(1);
        self.deblock = strength(encode.deblock);
        self.colourspace = COLOURSPACE_VALUES
            .iter()
            .position(|value| *value == encode.colorspace)
            .unwrap_or(0);
        self.grayscale = encode.grayscale;
        self.hdr = usize::from(encode.tone_map_to_sdr);
        self.tonemap = TONEMAP_VALUES
            .iter()
            .position(|value| *value == encode.tone_map_operator)
            .unwrap_or(0);

        // Video
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
        self.fps_mode = FPS_MODE_VALUES
            .iter()
            .position(|value| *value == encode.frame_rate_mode)
            .unwrap_or(0);
    }

    fn apply_audio(&mut self, audio: &AudioSettings) {
        self.default_audio_encoder = audio_encoder_for(&audio.codec);
        self.default_audio_mixdown = mixdown_index(legacy_mixdown(audio.mixdown));
        self.default_audio_bitrate = bitrate_index(audio.bitrate_kbps);
        self.languages = audio.languages.join(", ");
        self.audio_selection = AUDIO_SELECTION_VALUES
            .iter()
            .position(|value| *value == audio.selection)
            .unwrap_or(1);
        self.passthru_fallback = PASSTHRU_FALLBACK_VALUES
            .iter()
            .position(|value| *value == audio.fallback_encoder)
            .unwrap_or(0);
        for (allowed, codec) in self.passthru_allowed.iter_mut().zip(PassthruCodec::ALL) {
            *allowed = audio.allowed_passthru.contains(&codec);
        }
        if audio.tracks.is_empty() {
            for track in &mut self.audio_tracks {
                track.encoder = self.default_audio_encoder;
                track.mixdown = self.default_audio_mixdown;
                track.bitrate = self.default_audio_bitrate;
            }
        } else {
            self.audio_tracks = audio
                .tracks
                .iter()
                .map(|track| AudioTrackForm {
                    source_index: track.source,
                    source: format!("Track {}", track.source + 1),
                    source_meta: String::new(),
                    encoder: track.encoder,
                    mixdown: mixdown_index(track.mixdown),
                    sample_rate: SAMPLE_RATE_VALUES
                        .iter()
                        .position(|value| *value == track.sample_rate)
                        .unwrap_or(0),
                    bitrate: track
                        .bitrate_kbps
                        .map_or(self.default_audio_bitrate, bitrate_index),
                    gain: track.gain_db.round() as i32,
                    drc: track.drc.round() as i32,
                    name: track.name.clone(),
                })
                .collect();
        }
    }

    fn apply_subtitles(&mut self, subtitles: &SubtitleSettings) {
        self.sub_selection = if subtitles.include {
            SUB_SELECTION_VALUES
                .iter()
                .position(|value| *value == subtitles.selection)
                .filter(|index| *index != 0)
                .unwrap_or(1)
        } else {
            0
        };
        self.sub_burn = SUB_BURN_VALUES
            .iter()
            .position(|value| *value == subtitles.burn_behavior())
            .unwrap_or(0);
        self.sub_forced_only = subtitles.forced_only;
        self.sub_default = SUB_DEFAULT_VALUES
            .iter()
            .position(|value| *value == subtitles.default_track)
            .unwrap_or(0);
        for track in &mut self.sub_tracks {
            if track.kind == "search" {
                track.burn = matches!(self.sub_burn, 1 | 3);
            }
        }
        if !subtitles.external.is_empty() {
            self.sub_tracks.retain(|track| track.external.is_none());
            for track in &subtitles.external {
                let added = describe_external(&track.path);
                self.sub_tracks.push(SubTrackForm {
                    source: added.0,
                    kind: added.1,
                    language: track.language.clone().unwrap_or_else(|| "und".to_owned()),
                    forced_only: track.forced,
                    burn: track.burn_in,
                    default: track.default,
                    offset_ms: track.offset_ms as i32,
                    external: Some(track.path.clone()),
                });
            }
            if let Some(charset) = subtitles.external.iter().find_map(|t| t.charset.as_deref()) {
                self.charset = CHARSETS
                    .iter()
                    .position(|candidate| *candidate == charset)
                    .unwrap_or(0);
            }
        }
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

/// File name and kind label of an imported subtitle file.
pub fn describe_external(path: &std::path::Path) -> (String, String) {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_uppercase())
        .unwrap_or_default();
    (name, format!("{ext} (external)"))
}

/// Parses `1 : 1`, `32:27` or `32/27`.
fn parse_aspect(text: &str) -> Option<(u32, u32)> {
    let (num, den) = text.split_once([':', '/'])?;
    let (num, den) = (num.trim().parse().ok()?, den.trim().parse().ok()?);
    (num > 0 && den > 0).then_some((num, den))
}

/// Preset for core's legacy speed scale (0 slowest, 4 fastest).
fn preset_for_speed(encoder: &VideoEncoder, speed: u8) -> String {
    let presets = preset_options(encoder);
    if presets.is_empty() {
        return String::new();
    }
    let slowness = 1.0 - f64::from(speed.min(4)) / 4.0;
    presets[(slowness * (presets.len() - 1) as f64).round() as usize].to_owned()
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

/// Constant-quality value (CRF/CQ) core shows for a 0..=100 quality, parsed from its own label.
pub fn rf_from_quality(encoder: &VideoEncoder, quality: u8) -> u8 {
    quality_control_label(encoder, quality.min(100))
        .split_whitespace()
        .last()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(22)
        .min(rf_max(encoder))
}

/// Rough output size for a form against a source, used until a sample is measured.
pub fn heuristic_size(form: &TranscodeForm, source_bytes: u64, duration: Option<f64>) -> u64 {
    if !form.is_constant_quality() {
        let seconds = duration.unwrap_or(0.0);
        return (f64::from(form.bitrate_kbps) * 1000.0 / 8.0 * seconds) as u64;
    }
    // Calibrated against a 9.2 GB reference at CRF 18 for a 18.4 GB source (x265 scale).
    let rf = match form.encoder_family() {
        Family::Av1 | Family::Vp9 => i32::from(form.rf) * 51 / 63,
        _ => i32::from(form.rf),
    };
    let mut ratio = 0.5 * 0.89f64.powi(rf - 18);
    if form.encoder_family() == Family::Av1 {
        ratio *= 0.75;
    }
    if matches!(
        form.encoder,
        VideoEncoder::X264
            | VideoEncoder::Hardware {
                codec: HwCodec::H264,
                ..
            }
    ) {
        ratio *= 1.45;
    }
    (source_bytes as f64 * ratio.clamp(0.02, 1.0)) as u64
}

/// Rough encode time in minutes for a source of the given duration.
pub fn heuristic_minutes(form: &TranscodeForm, duration: Option<f64>) -> u64 {
    let hours = duration.unwrap_or(0.0) / 3600.0;
    let hardware = form.encoder_family() == Family::Hardware;
    let av1 = form.encoder_family() == Family::Av1;
    let slow = f64::from(form.preset_fraction()) * 9.0;
    let per_reference =
        (if hardware { 9.0 } else { 22.0 }) * (1.0 + slow * 0.35) * if av1 { 1.4 } else { 1.0 };
    (per_reference * hours / 2.19).round().max(1.0) as u64
}

#[cfg(test)]
mod tests;
