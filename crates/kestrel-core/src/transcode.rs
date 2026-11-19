use crate::compat::{self, StreamPlan};
use crate::encoders::EncoderSet;
use crate::external::ExternalTrack;
use crate::formats::Container;
use crate::mkv::{MediaMetadata, TrackInfo};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub mod audio;
pub mod chapters;
pub mod filters;
pub mod subs;

pub use audio::{
    AudioEncoder, AudioMixdown, AudioOutput, AudioTrackSetting, PassthruCodec, TrackSelection,
};
pub use filters::{
    Anamorphic, BorderFill, ColorSpaceTarget, DeinterlaceFilter, DeinterlacePreset, DenoiseFilter,
    DenoiseTune, Detelecine, FilterStrength, FrameRateMode, Rotation, SharpenFilter,
    ToneMapOperator,
};
pub use subs::{BurnBehavior, SubtitleDefault};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct JobSettings {
    pub container: Container,
    pub video: VideoSettings,
    pub audio: AudioSettings,
    pub subtitles: SubtitleSettings,
    pub keep_chapters: bool,
    /// Passes common source metadata through (`-map_metadata 0`).
    pub keep_metadata: bool,
    pub title: Option<String>,
    /// MP4/MOV: move the index to the front of the file (`-movflags +faststart`).
    pub web_optimized: bool,
    /// Pad or trim the start of re-encoded audio so it begins with the video.
    pub align_av_start: bool,
    /// MKV: keep attachments such as fonts and cover art.
    pub keep_attachments: bool,
    /// Chapter names that replace the source titles, in order; empty keeps the source titles.
    pub chapter_names: Vec<String>,
    pub tags: MediaTags,
    /// Track edits (drop, retitle, flags, external files in `track_edits.external`) for remux
    /// (stream-copy) jobs only. Transcode jobs use `subtitles.external` for imported subtitles.
    pub track_edits: crate::remux_edit::TrackEdits,
}

impl Default for JobSettings {
    fn default() -> Self {
        Self {
            container: Container::Mkv,
            video: VideoSettings::Copy,
            audio: AudioSettings::default(),
            subtitles: SubtitleSettings::default(),
            keep_chapters: true,
            keep_metadata: true,
            title: None,
            web_optimized: true,
            align_av_start: false,
            keep_attachments: true,
            chapter_names: Vec::new(),
            tags: MediaTags::default(),
            track_edits: crate::remux_edit::TrackEdits::default(),
        }
    }
}

impl JobSettings {
    /// True when no stream is re-encoded or burned into the video.
    pub fn is_remux_only(&self) -> bool {
        matches!(&self.video, VideoSettings::Copy)
            && self.audio.codec == AudioChoice::Passthrough
            && self
                .audio
                .tracks
                .iter()
                .all(|track| track.encoder.is_passthru())
            && self.subtitles.burn_in == BurnIn::None
            && !self.subtitles.external.iter().any(|track| track.burn_in)
    }

    /// True when the job uses a control that only the FFmpeg path implements (per-track audio,
    /// imported subtitles, tags, chapter names, selection behaviours). The MKVToolNix path
    /// handles language selection only.
    pub fn needs_ffmpeg_muxing(&self) -> bool {
        let audio = &self.audio;
        let subtitles = &self.subtitles;
        !audio.tracks.is_empty()
            || audio.selection != TrackSelection::AllMatching
            || audio.fallback_encoder.is_some()
            || audio.allowed_passthru != PassthruCodec::ALL
            || subtitles.selection != TrackSelection::AllMatching
            || subtitles.forced_only
            || subtitles.default_track != SubtitleDefault::First
            || !subtitles.external.is_empty()
            || !self.chapter_names.is_empty()
            || self.align_av_start
            || !self.keep_attachments
            || !self.tags.is_empty()
    }
}

/// Descriptive tags written with `-metadata`. The title lives in `JobSettings::title`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct MediaTags {
    /// Actors or artist (written as `artist`).
    pub actors: Option<String>,
    pub director: Option<String>,
    pub release_date: Option<String>,
    pub genre: Option<String>,
    pub description: Option<String>,
    pub comment: Option<String>,
}

impl MediaTags {
    pub fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }

    fn entries(&self) -> Vec<(&'static str, &str)> {
        [
            ("artist", &self.actors),
            ("director", &self.director),
            ("date", &self.release_date),
            ("genre", &self.genre),
            ("description", &self.description),
            ("comment", &self.comment),
        ]
        .into_iter()
        .filter_map(|(key, value)| {
            value
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| (key, value))
        })
        .collect()
    }
}

// `Encode` is not boxed: the UI crates construct and match `VideoSettings::Encode(VideoEncode)`.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSettings {
    #[default]
    Copy,
    Encode(VideoEncode),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct VideoEncode {
    pub encoder: VideoEncoder,
    pub rate: RateControl,
    pub speed: u8,
    /// Resolution limit: the output fits inside `max_width` x `max_height`, keeping the aspect.
    pub max_height: Option<u32>,
    pub max_width: Option<u32>,
    pub frame_rate: FrameRate,
    pub deinterlace: Deinterlace,
    pub denoise: Denoise,
    pub crop: Crop,
    pub tone_map_to_sdr: bool,
    pub ten_bit: TenBit,

    // Dimensions
    pub rotation: Rotation,
    pub flip_horizontal: bool,
    pub anamorphic: Anamorphic,
    /// Output width and height are multiples of this (2, 4, 8 or 16).
    pub modulus: u32,
    pub allow_upscaling: bool,
    pub border_fill: BorderFill,
    /// Colour of added borders: an FFmpeg colour name or `#RRGGBB`.
    pub border_color: String,

    // Filters
    pub detelecine: Detelecine,
    pub deinterlace_filter: DeinterlaceFilter,
    pub deinterlace_preset: DeinterlacePreset,
    /// Measure interlacing with `idet` before encoding when the source does not say.
    pub interlace_detection: bool,
    pub denoise_filter: DenoiseFilter,
    pub denoise_tune: DenoiseTune,
    pub chroma_smooth: FilterStrength,
    pub sharpen: SharpenFilter,
    pub sharpen_preset: FilterStrength,
    pub deblock: FilterStrength,
    pub colorspace: ColorSpaceTarget,
    pub grayscale: bool,
    pub tone_map_operator: ToneMapOperator,

    // Video
    pub frame_rate_mode: FrameRateMode,
    /// 2 runs a first analysis pass when `rate` is a bitrate.
    pub passes: u8,
    /// First pass uses the encoder's fast analysis (off adds `slow-firstpass=1` for x264/x265).
    pub turbo_first_pass: bool,
    /// Encoder preset by name; overrides `speed`.
    pub encoder_preset: Option<String>,
    pub tune: Option<String>,
    pub profile: Option<String>,
    pub level: Option<String>,
    pub fast_decode: bool,
    /// Extra encoder options (`key=value:key=value`) for x264, x265 or SVT-AV1.
    pub advanced_options: String,
}

impl Default for VideoEncode {
    fn default() -> Self {
        Self {
            encoder: VideoEncoder::X264,
            rate: RateControl::default(),
            speed: 2,
            max_height: None,
            max_width: None,
            frame_rate: FrameRate::Source,
            deinterlace: Deinterlace::default(),
            denoise: Denoise::default(),
            crop: Crop::default(),
            tone_map_to_sdr: false,
            ten_bit: TenBit::default(),
            rotation: Rotation::default(),
            flip_horizontal: false,
            anamorphic: Anamorphic::default(),
            modulus: 2,
            allow_upscaling: false,
            border_fill: BorderFill::default(),
            border_color: "black".to_owned(),
            detelecine: Detelecine::default(),
            deinterlace_filter: DeinterlaceFilter::default(),
            deinterlace_preset: DeinterlacePreset::default(),
            interlace_detection: false,
            denoise_filter: DenoiseFilter::default(),
            denoise_tune: DenoiseTune::default(),
            chroma_smooth: FilterStrength::default(),
            sharpen: SharpenFilter::default(),
            sharpen_preset: FilterStrength::Medium,
            deblock: FilterStrength::default(),
            colorspace: ColorSpaceTarget::default(),
            grayscale: false,
            tone_map_operator: ToneMapOperator::default(),
            frame_rate_mode: FrameRateMode::default(),
            passes: 1,
            turbo_first_pass: true,
            encoder_preset: None,
            tune: None,
            profile: None,
            level: None,
            fast_decode: false,
            advanced_options: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoEncoder {
    #[default]
    X264,
    X265,
    SvtAv1,
    AomAv1,
    Vp9,
    Hardware {
        vendor: HwVendor,
        codec: HwCodec,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HwVendor {
    Nvenc,
    Qsv,
    Amf,
    VideoToolbox,
    Vaapi,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HwCodec {
    H264,
    Hevc,
    Av1,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RateControl {
    /// Quality slider, 0 (smallest) to 100 (best), mapped onto each encoder's own scale.
    Quality(u8),
    /// The encoder's own constant-quality value (CRF, CQ, global quality or QP).
    Native(f32),
    /// Average bitrate in kbit/s; `VideoEncode::passes` selects one or two passes.
    Bitrate(u32),
}

impl Default for RateControl {
    fn default() -> Self {
        Self::Quality(60)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameRate {
    #[default]
    Source,
    Fixed(f64),
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Deinterlace {
    Off,
    #[default]
    Auto,
    On,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Denoise {
    #[default]
    Off,
    Light,
    Medium,
    Strong,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Crop {
    #[default]
    None,
    /// Detected with `cropdetect` before the job runs (see `analysis::resolve_automatic_settings`).
    Automatic,
    /// Like `Automatic` with each detected edge pulled in by 4 pixels, so less is removed.
    Conservative,
    Custom {
        top: u32,
        bottom: u32,
        left: u32,
        right: u32,
    },
}

/// The conservative variant of a detected crop: 4 pixels less on each cropped edge, kept even.
pub fn conservative_crop(crop: &Crop) -> Crop {
    match crop {
        Crop::Custom {
            top,
            bottom,
            left,
            right,
        } => {
            let shrink = |side: &u32| side.saturating_sub(4) & !1;
            let result = Crop::Custom {
                top: shrink(top),
                bottom: shrink(bottom),
                left: shrink(left),
                right: shrink(right),
            };
            if matches!(
                result,
                Crop::Custom {
                    top: 0,
                    bottom: 0,
                    left: 0,
                    right: 0
                }
            ) {
                Crop::None
            } else {
                result
            }
        }
        other => other.clone(),
    }
}

/// Output bit depth. `Twelve` is only available with x265.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenBit {
    #[default]
    MatchSource,
    Off,
    On,
    Twelve,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioChoice {
    #[default]
    Passthrough,
    Aac,
    Opus,
    Ac3,
    Eac3,
    Flac,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mixdown {
    #[default]
    Source,
    Stereo,
    Surround51,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct AudioSettings {
    pub languages: Vec<String>,
    pub codec: AudioChoice,
    pub bitrate_kbps: u32,
    pub mixdown: Mixdown,
    /// Explicit output tracks. When non-empty they replace language selection and `codec`.
    pub tracks: Vec<AudioTrackSetting>,
    pub selection: TrackSelection,
    /// Codecs that auto passthru may copy.
    pub allowed_passthru: Vec<PassthruCodec>,
    /// Encoder used when passthru is not allowed or the container cannot store the codec.
    /// `None` picks AAC (Opus for WebM).
    pub fallback_encoder: Option<AudioEncoder>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            languages: Vec::new(),
            codec: AudioChoice::Passthrough,
            bitrate_kbps: 160,
            mixdown: Mixdown::Source,
            tracks: Vec::new(),
            selection: TrackSelection::default(),
            allowed_passthru: PassthruCodec::ALL.to_vec(),
            fallback_encoder: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct SubtitleSettings {
    pub languages: Vec<String>,
    pub burn_in: BurnIn,
    pub include: bool,
    pub selection: TrackSelection,
    /// Keep only forced tracks among the soft subtitles.
    pub forced_only: bool,
    pub default_track: SubtitleDefault,
    /// With `BurnIn::ForcedOnly`: burn the first selected track when no forced track exists.
    /// See `burn_behavior`/`set_burn_behavior` for the four-way view.
    pub burn_in_fallback_first: bool,
    /// Subtitle files imported into transcode jobs. Remux jobs use `track_edits.external`.
    pub external: Vec<ExternalTrack>,
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            languages: Vec::new(),
            burn_in: BurnIn::None,
            include: true,
            selection: TrackSelection::default(),
            forced_only: false,
            default_track: SubtitleDefault::default(),
            burn_in_fallback_first: false,
            external: Vec::new(),
        }
    }
}

/// Which embedded subtitle is burned in. `ForcedOnly` is the foreign-audio search described on
/// [`BurnBehavior::ForeignAudio`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BurnIn {
    #[default]
    None,
    ForcedOnly,
    FirstSelected,
}

/// The subtitle rendered into the video, if any.
enum BurnSource<'a> {
    EmbeddedText,
    EmbeddedImage(usize),
    ExternalText(&'a ExternalTrack),
    ExternalImage,
}

fn burn_source<'a>(
    metadata: &'a MediaMetadata,
    settings: &'a JobSettings,
) -> Result<Option<BurnSource<'a>>, String> {
    if !settings.subtitles.include {
        return Ok(None);
    }
    subs::validate_external(&settings.subtitles.external)?;
    if let Some(track) = settings
        .subtitles
        .external
        .iter()
        .find(|track| track.burn_in)
    {
        let image = subs::is_image_codec(subs::external_codec_id(&track.path)?);
        return Ok(Some(if image {
            BurnSource::ExternalImage
        } else {
            BurnSource::ExternalText(track)
        }));
    }
    Ok(
        selected_burn_in_subtitle(metadata, settings).map(|(index, track)| {
            if is_text_subtitle(&track.codec_id) {
                BurnSource::EmbeddedText
            } else {
                BurnSource::EmbeddedImage(index)
            }
        }),
    )
}

fn crop_filter(crop: &Crop) -> Result<Option<String>, String> {
    match crop {
        Crop::None => Ok(None),
        Crop::Automatic | Crop::Conservative => Err("internal: crop was not resolved".to_owned()),
        Crop::Custom {
            top,
            bottom,
            left,
            right,
        } => Ok((*top != 0 || *bottom != 0 || *left != 0 || *right != 0)
            .then(|| format!("crop=iw-{left}-{right}:ih-{top}-{bottom}:{left}:{top}"))),
    }
}

/// Scale filter for the resolution limit, modulus and upscaling settings.
fn scale_filter(encode: &VideoEncode) -> Result<Option<String>, String> {
    let modulus = encode.modulus;
    if !matches!(modulus, 2 | 4 | 8 | 16) {
        return Err(format!("Modulus must be 2, 4, 8 or 16, not {modulus}"));
    }
    if encode.max_width == Some(0) || encode.max_height == Some(0) {
        return Err("The resolution limit must be larger than zero".to_owned());
    }
    Ok(
        match (encode.max_width, encode.max_height, encode.allow_upscaling) {
            (None, None, _) => (modulus != 2).then(|| {
                format!("scale=trunc(iw/{modulus})*{modulus}:trunc(ih/{modulus})*{modulus}")
            }),
            (None, Some(height), false) => Some(format!("scale=-{modulus}:'min(ih,{height})'")),
            (None, Some(height), true) => Some(format!("scale=-{modulus}:{height}")),
            (Some(width), None, false) => Some(format!("scale='min(iw,{width})':-{modulus}")),
            (Some(width), None, true) => Some(format!("scale={width}:-{modulus}")),
            (Some(width), Some(height), false) => Some(format!(
                "scale=w='min(iw,{width})':h='min(ih,{height})':force_original_aspect_ratio=decrease:force_divisible_by={modulus}"
            )),
            (Some(width), Some(height), true) => Some(format!(
                "scale=w={width}:h={height}:force_original_aspect_ratio=decrease:force_divisible_by={modulus}"
            )),
        },
    )
}

fn pad_filter(encode: &VideoEncode) -> Result<Option<String>, String> {
    let color = &encode.border_color;
    if !filters::is_valid_color(color) {
        return Err(format!("Invalid border colour: {color}"));
    }
    let need = |value: Option<u32>, name: &str| {
        value.ok_or_else(|| format!("Border fill needs a maximum {name}"))
    };
    Ok(match encode.border_fill {
        BorderFill::None => None,
        BorderFill::Height => Some(format!(
            "pad=iw:{}:0:(oh-ih)/2:color={color}",
            need(encode.max_height, "height")?
        )),
        BorderFill::Width => Some(format!(
            "pad={}:ih:(ow-iw)/2:0:color={color}",
            need(encode.max_width, "width")?
        )),
        BorderFill::Both => Some(format!(
            "pad={}:{}:(ow-iw)/2:(oh-ih)/2:color={color}",
            need(encode.max_width, "width")?,
            need(encode.max_height, "height")?
        )),
        BorderFill::Custom {
            top,
            bottom,
            left,
            right,
        } => (top | bottom | left | right != 0)
            .then(|| format!("pad=iw+{left}+{right}:ih+{top}+{bottom}:{left}:{top}:color={color}")),
    })
}

/// The `fps` filter for a fixed frame rate, honouring CFR/PFR/VFR.
fn frame_rate_filter(
    encode: &VideoEncode,
    metadata: &MediaMetadata,
) -> Result<Option<String>, String> {
    let FrameRate::Fixed(rate) = &encode.frame_rate else {
        return Ok(None);
    };
    if !rate.is_finite() || *rate <= 0.0 {
        return Err("The frame rate must be larger than zero".to_owned());
    }
    Ok(match encode.frame_rate_mode {
        FrameRateMode::Auto | FrameRateMode::Cfr => Some(format!("fps={rate}")),
        FrameRateMode::Vfr => None,
        FrameRateMode::Pfr => {
            let within_peak = metadata
                .video
                .as_ref()
                .and_then(|video| video.fps)
                .is_some_and(|fps| fps <= *rate + 0.01);
            (!within_peak).then(|| format!("fps={rate}"))
        }
    })
}

fn fps_mode(encode: &VideoEncode) -> Option<&'static str> {
    match encode.frame_rate_mode {
        FrameRateMode::Auto => None,
        FrameRateMode::Cfr => Some("cfr"),
        FrameRateMode::Pfr | FrameRateMode::Vfr => Some("vfr"),
    }
}

fn denoise_filter(encode: &VideoEncode) -> Option<String> {
    let preset = match encode.denoise {
        Denoise::Off => return None,
        Denoise::Light => 0,
        Denoise::Medium => 1,
        Denoise::Strong => 2,
    };
    Some(match encode.denoise_filter {
        DenoiseFilter::NlMeans => filters::nlmeans_filter(encode.denoise_tune, preset),
        DenoiseFilter::Hqdn3d => {
            ["hqdn3d=2:1.5:3:2.25", "hqdn3d=4:3:6:4.5", "hqdn3d=8:6:12:9"][preset].to_owned()
        }
    })
}

fn pixel_format_filters(encode: &VideoEncode) -> Result<Vec<String>, String> {
    let is_hardware = matches!(&encode.encoder, VideoEncoder::Hardware { .. });
    let mut filters = Vec::new();
    match encode.ten_bit {
        TenBit::MatchSource if matches!(&encode.encoder, VideoEncoder::X264) => {
            filters.push("format=yuv420p".to_owned());
        }
        TenBit::MatchSource => {}
        TenBit::Off => filters.push("format=yuv420p".to_owned()),
        TenBit::On if is_hardware => filters.push("format=p010le".to_owned()),
        TenBit::On => filters.push("format=yuv420p10le".to_owned()),
        TenBit::Twelve if encode.encoder == VideoEncoder::X265 => {
            filters.push("format=yuv420p12le".to_owned());
        }
        TenBit::Twelve => return Err("12-bit output is only available with x265".to_owned()),
    }
    if let VideoEncoder::Hardware {
        vendor: HwVendor::Vaapi,
        ..
    } = &encode.encoder
    {
        let format = if encode.ten_bit == TenBit::On {
            "p010"
        } else {
            "nv12"
        };
        filters.push(format!("format={format},hwupload"));
    }
    Ok(filters)
}

pub fn video_filter_chain(
    _input: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    text_burn_in_file: Option<&str>,
) -> Result<Vec<String>, String> {
    let burn = burn_source(metadata, settings)?;
    if matches!(burn, Some(BurnSource::EmbeddedText)) && text_burn_in_file.is_none() {
        return Err("internal: subtitle file not prepared".to_owned());
    }
    let Some(encode) = (match &settings.video {
        VideoSettings::Copy => None,
        VideoSettings::Encode(encode) => Some(encode),
    }) else {
        return Ok(Vec::new());
    };

    let mut filters = Vec::new();
    filters.extend(filters::detelecine_filter(encode.detelecine).map(str::to_owned));
    match encode.deinterlace {
        Deinterlace::Off => {}
        Deinterlace::On => filters.push(filters::deinterlace_filter(
            encode.deinterlace_filter,
            encode.deinterlace_preset,
            false,
        )),
        Deinterlace::Auto => {
            if metadata
                .video
                .as_ref()
                .is_none_or(|video| video.interlaced != Some(false))
            {
                filters.push(filters::deinterlace_filter(
                    encode.deinterlace_filter,
                    encode.deinterlace_preset,
                    true,
                ));
            }
        }
    }

    filters.extend(crop_filter(&encode.crop)?);
    filters.extend(
        encode
            .rotation
            .filters()
            .iter()
            .map(|filter| (*filter).to_owned()),
    );
    if encode.flip_horizontal {
        filters.push("hflip".to_owned());
    }

    if encode.tone_map_to_sdr && metadata.video.as_ref().is_some_and(|video| video.hdr) {
        filters.push(filters::tone_map_filter(encode.tone_map_operator));
    }
    let source_height = metadata.video.as_ref().and_then(|video| video.height);
    filters.extend(filters::colorspace_filter(encode.colorspace, source_height));

    if encode.anamorphic == Anamorphic::None {
        filters.push("scale=trunc(iw*sar/2)*2:ih,setsar=1".to_owned());
    }
    filters.extend(scale_filter(encode)?);
    filters.extend(pad_filter(encode)?);
    if let Anamorphic::Custom { num, den } = encode.anamorphic {
        if num == 0 || den == 0 {
            return Err("The pixel aspect ratio must be larger than zero".to_owned());
        }
        filters.push(format!("setsar={num}/{den}"));
    }
    filters.extend(frame_rate_filter(encode, metadata)?);
    filters.extend(denoise_filter(encode));
    filters.extend(filters::chroma_smooth_filter(encode.chroma_smooth));
    filters.extend(filters::sharpen_filter(
        encode.sharpen,
        encode.sharpen_preset,
    ));
    filters.extend(filters::deblock_filter(encode.deblock));
    if encode.grayscale {
        filters.push("hue=s=0".to_owned());
    }

    match burn {
        Some(BurnSource::EmbeddedText) => {
            let filename = text_burn_in_file.expect("validated above");
            filters.push(format!("subtitles=filename={filename}"));
        }
        Some(BurnSource::ExternalText(track)) => {
            let mut filter = format!(
                "subtitles=filename={}",
                filters::escape_filter_path(&subtitles_absolute(&track.path))
            );
            if let Some(charset) = &track.charset {
                filter.push_str(&format!(":charenc={charset}"));
            }
            if track.offset_ms != 0 {
                // The subtitles filter has no offset option: shift the video clock instead.
                let shift = subs::offset_seconds(track.offset_ms);
                filters.push(format!("setpts=PTS-({shift})/TB"));
                filters.push(filter);
                filters.push(format!("setpts=PTS+({shift})/TB"));
            } else {
                filters.push(filter);
            }
        }
        _ => {}
    }

    filters.extend(pixel_format_filters(encode)?);
    Ok(filters)
}

fn subtitles_absolute(path: &Path) -> PathBuf {
    crate::subtitles::absolute_path(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Resolves unavailable video encoders without changing the requested codec family.
pub fn resolve_job_settings(
    settings: &JobSettings,
    encoders: &EncoderSet,
) -> Result<JobSettings, String> {
    let mut settings = settings.clone();
    if let VideoSettings::Encode(encode) = &mut settings.video {
        encode.encoder = encoders.resolve(&encode.encoder)?;
    }
    Ok(settings)
}

impl VideoEncoder {
    /// Codec family shared by interchangeable software and hardware encoders.
    pub fn codec_family(&self) -> &'static str {
        match self {
            Self::X264
            | Self::Hardware {
                codec: HwCodec::H264,
                ..
            } => "H.264",
            Self::X265
            | Self::Hardware {
                codec: HwCodec::Hevc,
                ..
            } => "HEVC",
            Self::SvtAv1
            | Self::AomAv1
            | Self::Hardware {
                codec: HwCodec::Av1,
                ..
            } => "AV1",
            Self::Vp9 => "VP9",
        }
    }

    /// Short display name such as `x264` or `NVENC H.264`.
    pub fn label(&self) -> String {
        match self {
            Self::X264 => "H.264 (x264)".to_owned(),
            Self::X265 => "H.265 (x265)".to_owned(),
            Self::SvtAv1 => "AV1 (SVT-AV1)".to_owned(),
            Self::AomAv1 => "AV1 (libaom)".to_owned(),
            Self::Vp9 => "VP9 (libvpx)".to_owned(),
            Self::Hardware { vendor, codec } => {
                let vendor = match vendor {
                    HwVendor::Nvenc => "NVENC",
                    HwVendor::Qsv => "QSV",
                    HwVendor::Amf => "AMF",
                    HwVendor::VideoToolbox => "VideoToolbox",
                    HwVendor::Vaapi => "VAAPI",
                };
                let codec = match codec {
                    HwCodec::H264 => "H.264",
                    HwCodec::Hevc => "H.265",
                    HwCodec::Av1 => "AV1",
                };
                format!("{codec} ({vendor})")
            }
        }
    }

    fn is_x26x(&self) -> bool {
        matches!(self, Self::X264 | Self::X265)
    }

    /// Whether the encoder accepts the `fast_decode` option.
    pub fn supports_fast_decode(&self) -> bool {
        matches!(self, Self::X264 | Self::X265 | Self::SvtAv1)
    }

    /// Whether the encoder takes an advanced options string.
    pub fn supports_advanced_options(&self) -> bool {
        matches!(self, Self::X264 | Self::X265 | Self::SvtAv1)
    }

    /// Whether two-pass average-bitrate encoding is available.
    pub fn supports_multi_pass(&self) -> bool {
        !matches!(self, Self::Hardware { .. })
    }
}

/// One selectable entry of the encoder list: an encoder with a bit depth.
#[derive(Clone, Debug, PartialEq)]
pub struct EncoderChoice {
    pub label: String,
    pub encoder: VideoEncoder,
    pub ten_bit: TenBit,
}

/// Every encoder and bit-depth combination Kestrel can drive, in display order. Filter with
/// [`EncoderSet::supports`] to show only what the installed FFmpeg provides.
pub fn encoder_choices() -> Vec<EncoderChoice> {
    let choice = |encoder: VideoEncoder, ten_bit: TenBit, depth: &str| EncoderChoice {
        label: format!("{} {depth}", encoder.label()),
        encoder,
        ten_bit,
    };
    let mut choices = vec![
        choice(VideoEncoder::X264, TenBit::Off, "8-bit"),
        choice(VideoEncoder::X264, TenBit::On, "10-bit"),
        choice(VideoEncoder::X265, TenBit::Off, "8-bit"),
        choice(VideoEncoder::X265, TenBit::On, "10-bit"),
        choice(VideoEncoder::X265, TenBit::Twelve, "12-bit"),
        choice(VideoEncoder::SvtAv1, TenBit::Off, "8-bit"),
        choice(VideoEncoder::SvtAv1, TenBit::On, "10-bit"),
        choice(VideoEncoder::AomAv1, TenBit::Off, "8-bit"),
        choice(VideoEncoder::AomAv1, TenBit::On, "10-bit"),
        choice(VideoEncoder::Vp9, TenBit::Off, "8-bit"),
        choice(VideoEncoder::Vp9, TenBit::On, "10-bit"),
    ];
    for vendor in [
        HwVendor::Nvenc,
        HwVendor::Qsv,
        HwVendor::Amf,
        HwVendor::VideoToolbox,
        HwVendor::Vaapi,
    ] {
        for codec in [HwCodec::H264, HwCodec::Hevc, HwCodec::Av1] {
            let encoder = VideoEncoder::Hardware { vendor, codec };
            choices.push(choice(encoder.clone(), TenBit::Off, "8-bit"));
            if codec != HwCodec::H264 {
                choices.push(choice(encoder, TenBit::On, "10-bit"));
            }
        }
    }
    choices
}

impl VideoEncode {
    pub fn apply_choice(&mut self, choice: &EncoderChoice) {
        self.encoder = choice.encoder.clone();
        self.ten_bit = choice.ten_bit;
    }
}

/// Files the worker prepares for a job before the arguments are built.
#[derive(Clone, Debug, Default)]
pub struct JobResources {
    /// File name (relative to the working directory) of the extracted text subtitle to burn in.
    pub text_burn_in_file: Option<String>,
    /// FFMETADATA file that renames chapters (see [`chapters::prepare_chapter_file`]).
    pub chapters_file: Option<PathBuf>,
    /// Path prefix for the two-pass statistics files.
    pub pass_log_prefix: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PassKind {
    Single,
    First,
    Second,
}

/// The FFmpeg invocations a job needs: one normally, two for two-pass average-bitrate encodes.
/// Run them in order; the last writes the output.
pub fn build_ffmpeg_job_passes(
    input: &Path,
    output_tmp: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    encoders: &EncoderSet,
    resources: &JobResources,
) -> Result<Vec<Vec<OsString>>, String> {
    let resolved_settings = resolve_job_settings(settings, encoders)?;
    let settings = &resolved_settings;
    let multi_pass = matches!(
        &settings.video,
        VideoSettings::Encode(encode)
            if encode.passes >= 2 && matches!(encode.rate, RateControl::Bitrate(_))
    );
    if multi_pass {
        if let VideoSettings::Encode(encode) = &settings.video
            && !encode.encoder.supports_multi_pass()
        {
            return Err(format!(
                "Two-pass encoding is not available for {}",
                video_encoder_name_for_detection(&encode.encoder)
            ));
        }
        if resources.pass_log_prefix.is_none() {
            return Err("internal: pass log location not prepared".to_owned());
        }
        Ok(vec![
            build_pass(
                input,
                output_tmp,
                metadata,
                settings,
                encoders,
                resources,
                PassKind::First,
            )?,
            build_pass(
                input,
                output_tmp,
                metadata,
                settings,
                encoders,
                resources,
                PassKind::Second,
            )?,
        ])
    } else {
        Ok(vec![build_pass(
            input,
            output_tmp,
            metadata,
            settings,
            encoders,
            resources,
            PassKind::Single,
        )?])
    }
}

/// Builds the single-pass argument list (two-pass settings run as one pass here, which is what
/// size estimation and previews need).
pub fn build_ffmpeg_job_arguments(
    input: &Path,
    output_tmp: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    encoders: &EncoderSet,
    text_burn_in_file: Option<&str>,
) -> Result<Vec<OsString>, String> {
    let resolved_settings = resolve_job_settings(settings, encoders)?;
    let resources = JobResources {
        text_burn_in_file: text_burn_in_file.map(str::to_owned),
        ..JobResources::default()
    };
    build_pass(
        input,
        output_tmp,
        metadata,
        &resolved_settings,
        encoders,
        &resources,
        PassKind::Single,
    )
}

fn build_pass(
    input: &Path,
    output_tmp: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    encoders: &EncoderSet,
    resources: &JobResources,
    pass: PassKind,
) -> Result<Vec<OsString>, String> {
    let first_pass = pass == PassKind::First;
    let audio_outputs = audio::plan_audio(metadata, settings)?;
    let subtitles = if settings.subtitles.include {
        soft_subtitle_tracks(metadata, settings)
    } else {
        Vec::new()
    };
    let burn = burn_source(metadata, settings)?;
    let video_filters = video_filter_chain(
        input,
        metadata,
        settings,
        resources.text_burn_in_file.as_deref(),
    )?;
    if burn.is_some() && matches!(&settings.video, VideoSettings::Copy) {
        return Err("Subtitle burn-in requires video re-encoding".to_owned());
    }

    let mut arguments = Vec::new();
    if matches!(
        &settings.video,
        VideoSettings::Encode(VideoEncode {
            encoder: VideoEncoder::Hardware {
                vendor: HwVendor::Vaapi,
                ..
            },
            ..
        })
    ) {
        arguments.extend(["-vaapi_device".into(), "/dev/dri/renderD128".into()]);
    }
    arguments.extend(["-i".into(), input.as_os_str().to_owned()]);

    // Extra inputs: imported subtitle files (soft or image burn-in), then the chapter file.
    let mut extra_inputs = 0_usize;
    let mut external_soft = Vec::new();
    let mut external_burn_image_input = None;
    if settings.subtitles.include {
        for track in &settings.subtitles.external {
            let codec = subs::external_codec_id(&track.path)?;
            if track.burn_in {
                if subs::is_image_codec(codec) {
                    external_burn_image_input = Some(extra_inputs + 1);
                    push_external_input(&mut arguments, track);
                    extra_inputs += 1;
                }
                continue;
            }
            if first_pass {
                continue;
            }
            let plan = compat::subtitle_plan(codec, settings.container);
            if matches!(plan, StreamPlan::Drop(_)) {
                continue;
            }
            external_soft.push((extra_inputs + 1, track, plan));
            push_external_input(&mut arguments, track);
            extra_inputs += 1;
        }
    }
    let chapters_input = match (&resources.chapters_file, first_pass) {
        (Some(file), false) if settings.keep_chapters => {
            arguments.extend([
                "-f".into(),
                "ffmetadata".into(),
                "-i".into(),
                file.as_os_str().to_owned(),
            ]);
            extra_inputs += 1;
            Some(extra_inputs)
        }
        _ => None,
    };

    let image_burn = match &burn {
        Some(BurnSource::EmbeddedImage(index)) => Some(format!("0:s:{index}")),
        Some(BurnSource::ExternalImage) => {
            external_burn_image_input.map(|input_index| format!("{input_index}:s:0"))
        }
        _ => None,
    };
    if let Some(subtitle_stream) = image_burn {
        if !matches!(&settings.video, VideoSettings::Encode(_)) {
            return Err("Subtitle burn-in requires video re-encoding".to_owned());
        }
        let chain = video_filters.join(",");
        let filter_complex = if chain.is_empty() {
            format!("[0:v:0][{subtitle_stream}]overlay=eof_action=pass[vout]")
        } else {
            format!("[0:v:0][{subtitle_stream}]overlay=eof_action=pass[ov];[ov]{chain}[vout]")
        };
        arguments.extend(["-filter_complex".into(), filter_complex.into()]);
        arguments.extend(["-map".into(), "[vout]".into()]);
    } else {
        arguments.extend(["-map".into(), "0:v:0".into()]);
        if !video_filters.is_empty() {
            if !matches!(&settings.video, VideoSettings::Encode(_)) {
                return Err("Video filters require video re-encoding".to_owned());
            }
            arguments.extend(["-vf".into(), video_filters.join(",").into()]);
        }
    }

    let video_codec = match &settings.video {
        VideoSettings::Copy => match metadata
            .video
            .as_ref()
            .map(|video| video.codec_id.as_str())
            .or(metadata.video_codec.as_deref())
        {
            Some(codec_id) => match compat::video_copy_plan(codec_id, settings.container) {
                StreamPlan::Copy => "copy".to_owned(),
                StreamPlan::Drop(_) => {
                    return Err(format!(
                        "Video must be re-encoded for {}",
                        settings.container.label()
                    ));
                }
                StreamPlan::Convert(encoder) => encoder.to_owned(),
            },
            None => return Err("Video codec metadata is unavailable".to_owned()),
        },
        VideoSettings::Encode(encode) => {
            if !encoders.supports(&encode.encoder) {
                return Err(format!(
                    "Video encoder {} is unavailable",
                    video_encoder_name_for_detection(&encode.encoder)
                ));
            }
            video_encoder_name_for_detection(&encode.encoder).to_owned()
        }
    };
    if let VideoSettings::Encode(encode) = &settings.video {
        append_video_rate_arguments(&mut arguments, &encode.encoder, &encode.rate);
        match &encode.encoder_preset {
            Some(preset) => append_explicit_preset(&mut arguments, &encode.encoder, preset)?,
            None => append_video_speed_arguments(&mut arguments, &encode.encoder, encode.speed),
        }
        append_video_option_arguments(&mut arguments, encode, metadata, pass)?;
    }
    arguments.extend(["-c:v".into(), video_codec.into()]);
    if let VideoSettings::Encode(encode) = &settings.video {
        if let Some(mode) = fps_mode(encode) {
            push_pair(&mut arguments, "-fps_mode", mode);
        }
        if let Some((space, primaries, transfer)) = encode.colorspace.tags() {
            push_pair(&mut arguments, "-colorspace", space);
            push_pair(&mut arguments, "-color_primaries", primaries);
            push_pair(&mut arguments, "-color_trc", transfer);
        }
    }

    if pass != PassKind::Single {
        let prefix = resources
            .pass_log_prefix
            .as_ref()
            .ok_or_else(|| "internal: pass log location not prepared".to_owned())?;
        push_pair(&mut arguments, "-pass", if first_pass { "1" } else { "2" });
        push_pair(
            &mut arguments,
            "-passlogfile",
            prefix.as_os_str().to_owned(),
        );
    }
    if first_pass {
        arguments.extend([
            "-an".into(),
            "-sn".into(),
            "-dn".into(),
            "-progress".into(),
            "pipe:1".into(),
            "-nostats".into(),
            "-y".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ]);
        return Ok(arguments);
    }

    for (output_index, output) in audio_outputs.iter().enumerate() {
        audio::append_audio_output(
            &mut arguments,
            output_index,
            output,
            settings.align_av_start,
        )?;
    }

    append_subtitle_outputs(
        &mut arguments,
        metadata,
        settings,
        &subtitles,
        &burn,
        &external_soft,
    )?;

    if settings.container == Container::Mkv && settings.keep_attachments && burn.is_none() {
        arguments.extend(["-map".into(), "0:t?".into()]);
    }
    arguments.extend([
        "-map_metadata".into(),
        if settings.keep_metadata {
            "0".into()
        } else {
            "-1".into()
        },
        "-map_chapters".into(),
        if !settings.keep_chapters {
            "-1".into()
        } else if let Some(index) = chapters_input {
            index.to_string().into()
        } else {
            "0".into()
        },
    ]);
    if let Some(title) = settings.title.as_deref().filter(|title| !title.is_empty()) {
        arguments.extend(["-metadata".into(), format!("title={title}").into()]);
    }
    for (key, value) in settings.tags.entries() {
        arguments.extend(["-metadata".into(), format!("{key}={value}").into()]);
    }
    if settings.align_av_start {
        push_pair(&mut arguments, "-avoid_negative_ts", "make_zero");
    }
    arguments.extend([
        "-max_muxing_queue_size".into(),
        "4096".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        "-y".into(),
        "-f".into(),
        settings.container.ffmpeg_muxer().into(),
    ]);
    if settings.web_optimized && matches!(settings.container, Container::Mp4 | Container::Mov) {
        arguments.extend(["-movflags".into(), "+faststart".into()]);
    }
    arguments.push(output_tmp.as_os_str().to_owned());
    Ok(arguments)
}

fn push_external_input(arguments: &mut Vec<OsString>, track: &ExternalTrack) {
    if track.offset_ms != 0 {
        push_pair(
            arguments,
            "-itsoffset",
            subs::offset_seconds(track.offset_ms),
        );
    }
    if let Some(charset) = &track.charset {
        push_pair(arguments, "-sub_charenc", charset.as_str());
    }
    push_pair(arguments, "-i", track.path.as_os_str().to_owned());
}

fn append_subtitle_outputs(
    arguments: &mut Vec<OsString>,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    subtitles: &[&TrackInfo],
    burn: &Option<BurnSource<'_>>,
    external_soft: &[(usize, &ExternalTrack, StreamPlan)],
) -> Result<(), String> {
    let burned_track = match burn {
        Some(BurnSource::EmbeddedText | BurnSource::EmbeddedImage(_)) => {
            selected_burn_in_subtitle(metadata, settings).map(|(_, track)| track)
        }
        _ => None,
    };
    let mut embedded = Vec::new();
    for track in subtitles {
        if burned_track.is_some_and(|burned| std::ptr::eq(*track, burned)) {
            continue;
        }
        if matches!(
            compat::subtitle_plan(&track.codec_id, settings.container),
            StreamPlan::Drop(_)
        ) {
            continue;
        }
        embedded.push(*track);
    }

    // Default flag: an imported track marked default wins, otherwise the configured rule.
    let forced_flags = embedded
        .iter()
        .map(|track| track.forced)
        .chain(external_soft.iter().map(|(_, track, _)| track.forced))
        .collect::<Vec<_>>();
    let default_index = external_soft
        .iter()
        .position(|(_, track, _)| track.default)
        .map(|position| embedded.len() + position)
        .or_else(|| match settings.subtitles.default_track {
            SubtitleDefault::First => (!forced_flags.is_empty()).then_some(0),
            SubtitleDefault::None => None,
            SubtitleDefault::Forced => forced_flags.iter().position(|forced| *forced),
        });

    for (output_index, track) in embedded.iter().enumerate() {
        let source_index = track
            .ffprobe_index
            .ok_or_else(|| "FFprobe stream indices are unavailable".to_owned())?;
        arguments.extend([
            "-map".into(),
            format!("0:{source_index}").into(),
            format!("-c:s:{output_index}").into(),
        ]);
        match compat::subtitle_plan(&track.codec_id, settings.container) {
            StreamPlan::Copy => arguments.push("copy".into()),
            StreamPlan::Convert(codec) => arguments.push(codec.into()),
            StreamPlan::Drop(_) => continue,
        }
        arguments.extend([
            format!("-disposition:s:{output_index}").into(),
            disposition(default_index == Some(output_index), track.forced).into(),
        ]);
    }

    for (position, (input_index, track, plan)) in external_soft.iter().enumerate() {
        let output_index = embedded.len() + position;
        arguments.extend([
            "-map".into(),
            format!("{input_index}:s:0").into(),
            format!("-c:s:{output_index}").into(),
            match plan {
                StreamPlan::Copy => "copy".into(),
                StreamPlan::Convert(codec) => (*codec).into(),
                StreamPlan::Drop(_) => continue,
            },
        ]);
        if let Some(language) = track.language.as_deref().filter(|value| !value.is_empty()) {
            arguments.extend([
                format!("-metadata:s:s:{output_index}").into(),
                format!("language={language}").into(),
            ]);
        }
        if let Some(title) = track.title.as_deref().filter(|value| !value.is_empty()) {
            arguments.extend([
                format!("-metadata:s:s:{output_index}").into(),
                format!("title={title}").into(),
            ]);
        }
        arguments.extend([
            format!("-disposition:s:{output_index}").into(),
            disposition(default_index == Some(output_index), track.forced).into(),
        ]);
    }
    Ok(())
}

/// Subtitle tracks kept as soft subtitles before the burned track is removed.
pub fn soft_subtitle_tracks<'a>(
    metadata: &'a MediaMetadata,
    settings: &JobSettings,
) -> Vec<&'a TrackInfo> {
    let mut tracks = selected_subtitle_tracks(metadata, settings);
    if settings.subtitles.forced_only {
        tracks.retain(|track| track.forced);
    }
    tracks
}

fn selected_subtitle_tracks<'a>(
    metadata: &'a MediaMetadata,
    settings: &JobSettings,
) -> Vec<&'a TrackInfo> {
    audio::select_tracks(
        &metadata.subtitle_tracks,
        &settings.subtitles.languages,
        settings.subtitles.selection,
        false,
    )
}

/// Language of the first selected audio track.
fn primary_audio_language(metadata: &MediaMetadata, settings: &JobSettings) -> Option<String> {
    audio::selected_audio_tracks(metadata, settings)
        .ok()?
        .first()
        .map(|track| track.language.to_ascii_lowercase())
}

/// The embedded subtitle that is burned into the video, as `(index in the subtitle list, track)`.
///
/// Returns `None` when an imported subtitle is burned instead.
pub fn selected_burn_in_subtitle<'a>(
    metadata: &'a MediaMetadata,
    settings: &JobSettings,
) -> Option<(usize, &'a TrackInfo)> {
    if !settings.subtitles.include
        || settings.subtitles.burn_in == BurnIn::None
        || settings
            .subtitles
            .external
            .iter()
            .any(|track| track.burn_in)
    {
        return None;
    }
    let selected = selected_subtitle_tracks(metadata, settings);
    let first_selected = selected.first().copied();
    let chosen = match settings.subtitles.burn_in {
        BurnIn::None => None,
        BurnIn::FirstSelected => first_selected,
        BurnIn::ForcedOnly => {
            let primary = primary_audio_language(metadata, settings);
            let forced_in_primary = selected.iter().find(|track| {
                track.forced
                    && primary
                        .as_deref()
                        .is_some_and(|language| track.language.eq_ignore_ascii_case(language))
            });
            forced_in_primary
                .or_else(|| selected.iter().find(|track| track.forced))
                .copied()
                .or_else(|| {
                    settings
                        .subtitles
                        .burn_in_fallback_first
                        .then_some(first_selected)
                        .flatten()
                })
        }
    };
    chosen.and_then(|track| {
        metadata
            .subtitle_tracks
            .iter()
            .position(|candidate| std::ptr::eq(candidate, track))
            .map(|index| (index, track))
    })
}

pub fn is_text_subtitle(codec_id: &str) -> bool {
    matches!(
        codec_id.to_ascii_lowercase().as_str(),
        "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "text" | "mov_text"
    )
}

pub fn video_encoder_name_for_detection(encoder: &VideoEncoder) -> &'static str {
    match encoder {
        VideoEncoder::X264 => "libx264",
        VideoEncoder::X265 => "libx265",
        VideoEncoder::SvtAv1 => "libsvtav1",
        VideoEncoder::AomAv1 => "libaom-av1",
        VideoEncoder::Vp9 => "libvpx-vp9",
        VideoEncoder::Hardware { vendor, codec } => match (codec, vendor) {
            (HwCodec::H264, HwVendor::Nvenc) => "h264_nvenc",
            (HwCodec::Hevc, HwVendor::Nvenc) => "hevc_nvenc",
            (HwCodec::Av1, HwVendor::Nvenc) => "av1_nvenc",
            (HwCodec::H264, HwVendor::Qsv) => "h264_qsv",
            (HwCodec::Hevc, HwVendor::Qsv) => "hevc_qsv",
            (HwCodec::Av1, HwVendor::Qsv) => "av1_qsv",
            (HwCodec::H264, HwVendor::Amf) => "h264_amf",
            (HwCodec::Hevc, HwVendor::Amf) => "hevc_amf",
            (HwCodec::Av1, HwVendor::Amf) => "av1_amf",
            (HwCodec::H264, HwVendor::VideoToolbox) => "h264_videotoolbox",
            (HwCodec::Hevc, HwVendor::VideoToolbox) => "hevc_videotoolbox",
            (HwCodec::Av1, HwVendor::VideoToolbox) => "av1_videotoolbox",
            (HwCodec::H264, HwVendor::Vaapi) => "h264_vaapi",
            (HwCodec::Hevc, HwVendor::Vaapi) => "hevc_vaapi",
            (HwCodec::Av1, HwVendor::Vaapi) => "av1_vaapi",
        },
    }
}

fn append_video_rate_arguments(
    arguments: &mut Vec<OsString>,
    encoder: &VideoEncoder,
    rate: &RateControl,
) {
    match rate {
        RateControl::Quality(quality) => {
            let quality = quality.min(&100);
            let value = match encoder {
                VideoEncoder::X264 | VideoEncoder::X265 => quality_value(*quality, 30, 14),
                VideoEncoder::SvtAv1 | VideoEncoder::AomAv1 => quality_value(*quality, 50, 18),
                VideoEncoder::Vp9 => quality_value(*quality, 50, 15),
                VideoEncoder::Hardware { vendor, .. } => match vendor {
                    HwVendor::VideoToolbox => quality_value(*quality, 30, 85),
                    _ => quality_value(*quality, 35, 16),
                },
            };
            append_quality_arguments(arguments, encoder, &value);
        }
        RateControl::Native(value) => {
            let text = if encoder.is_x26x() {
                format!("{}", (value * 100.0).round() / 100.0)
            } else {
                format!("{}", value.round())
            };
            append_quality_arguments(arguments, encoder, &text);
        }
        RateControl::Bitrate(bitrate) => {
            push_pair(arguments, "-b:v", format!("{bitrate}k"));
            if !matches!(
                encoder,
                VideoEncoder::Vp9 | VideoEncoder::SvtAv1 | VideoEncoder::AomAv1
            ) {
                push_pair(
                    arguments,
                    "-maxrate",
                    format!("{}k", (*bitrate as f64 * 1.5).round() as u32),
                );
                push_pair(
                    arguments,
                    "-bufsize",
                    format!("{}k", bitrate.saturating_mul(2)),
                );
            }
        }
    }
}

/// Constant-quality arguments with the encoder's own value (CRF, CQ, global quality or QP).
fn append_quality_arguments(arguments: &mut Vec<OsString>, encoder: &VideoEncoder, value: &str) {
    match encoder {
        VideoEncoder::X264 | VideoEncoder::X265 | VideoEncoder::SvtAv1 => {
            push_pair(arguments, "-crf", value);
        }
        VideoEncoder::AomAv1 | VideoEncoder::Vp9 => {
            push_pair(arguments, "-crf", value);
            push_pair(arguments, "-b:v", "0");
        }
        VideoEncoder::Hardware { vendor, .. } => match vendor {
            HwVendor::Nvenc => {
                push_pair(arguments, "-rc", "vbr");
                push_pair(arguments, "-cq", value);
                push_pair(arguments, "-b:v", "0");
            }
            HwVendor::Qsv => push_pair(arguments, "-global_quality", value),
            HwVendor::Amf => {
                push_pair(arguments, "-rc", "cqp");
                push_pair(arguments, "-qp_i", value);
                push_pair(arguments, "-qp_p", value);
                push_pair(arguments, "-qp_b", value);
            }
            HwVendor::VideoToolbox => push_pair(arguments, "-q:v", value),
            HwVendor::Vaapi => {
                push_pair(arguments, "-rc_mode", "CQP");
                push_pair(arguments, "-qp", value);
            }
        },
    }
}

fn append_video_speed_arguments(arguments: &mut Vec<OsString>, encoder: &VideoEncoder, speed: u8) {
    let speed = speed.min(4) as usize;
    match encoder {
        VideoEncoder::X264 | VideoEncoder::X265 => {
            push_pair(
                arguments,
                "-preset",
                ["slow", "medium", "fast", "faster", "veryfast"][speed],
            );
        }
        VideoEncoder::SvtAv1 => {
            push_pair(arguments, "-preset", [4, 6, 8, 10, 12][speed].to_string());
        }
        VideoEncoder::AomAv1 => {
            push_pair(arguments, "-cpu-used", [4, 5, 6, 7, 8][speed].to_string());
            push_pair(arguments, "-row-mt", "1");
        }
        VideoEncoder::Vp9 => {
            push_pair(arguments, "-deadline", "good");
            push_pair(arguments, "-row-mt", "1");
            push_pair(arguments, "-cpu-used", (speed + 1).to_string());
        }
        VideoEncoder::Hardware { vendor, .. } => match vendor {
            HwVendor::Nvenc => {
                push_pair(arguments, "-preset", ["p7", "p6", "p5", "p3", "p1"][speed])
            }
            HwVendor::Qsv => push_pair(
                arguments,
                "-preset",
                ["veryslow", "slow", "medium", "fast", "veryfast"][speed],
            ),
            HwVendor::Amf => push_pair(
                arguments,
                "-quality",
                ["quality", "quality", "balanced", "speed", "speed"][speed],
            ),
            HwVendor::VideoToolbox | HwVendor::Vaapi => {}
        },
    }
}

const X26X_PRESETS: [&str; 10] = [
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
const SVT_PRESETS: [&str; 14] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13",
];
const CPU_USED: [&str; 9] = ["0", "1", "2", "3", "4", "5", "6", "7", "8"];
const NVENC_PRESETS: [&str; 7] = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"];
const QSV_PRESETS: [&str; 7] = [
    "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow",
];
const AMF_PRESETS: [&str; 3] = ["speed", "balanced", "quality"];

/// Preset names the encoder accepts, slowest last where that is meaningful.
pub fn encoder_presets(encoder: &VideoEncoder) -> &'static [&'static str] {
    match encoder {
        VideoEncoder::X264 | VideoEncoder::X265 => &X26X_PRESETS,
        VideoEncoder::SvtAv1 => &SVT_PRESETS,
        VideoEncoder::AomAv1 | VideoEncoder::Vp9 => &CPU_USED,
        VideoEncoder::Hardware { vendor, .. } => match vendor {
            HwVendor::Nvenc => &NVENC_PRESETS,
            HwVendor::Qsv => &QSV_PRESETS,
            HwVendor::Amf => &AMF_PRESETS,
            HwVendor::VideoToolbox | HwVendor::Vaapi => &[],
        },
    }
}

/// Tune names the encoder accepts.
pub fn encoder_tunes(encoder: &VideoEncoder) -> &'static [&'static str] {
    match encoder {
        VideoEncoder::X264 => &[
            "film",
            "animation",
            "grain",
            "stillimage",
            "psnr",
            "ssim",
            "fastdecode",
            "zerolatency",
        ],
        VideoEncoder::X265 => &[
            "psnr",
            "ssim",
            "grain",
            "zerolatency",
            "fastdecode",
            "animation",
        ],
        VideoEncoder::SvtAv1 => &["vq", "psnr", "ssim"],
        VideoEncoder::Hardware {
            vendor: HwVendor::Nvenc,
            ..
        } => &["hq", "uhq", "ll", "ull", "lossless"],
        _ => &[],
    }
}

fn append_explicit_preset(
    arguments: &mut Vec<OsString>,
    encoder: &VideoEncoder,
    preset: &str,
) -> Result<(), String> {
    if !encoder_presets(encoder).contains(&preset) {
        return Err(format!(
            "Preset {preset} is not available for {}",
            video_encoder_name_for_detection(encoder)
        ));
    }
    match encoder {
        VideoEncoder::AomAv1 => {
            push_pair(arguments, "-cpu-used", preset);
            push_pair(arguments, "-row-mt", "1");
        }
        VideoEncoder::Vp9 => {
            push_pair(arguments, "-deadline", "good");
            push_pair(arguments, "-row-mt", "1");
            push_pair(arguments, "-cpu-used", preset);
        }
        VideoEncoder::Hardware {
            vendor: HwVendor::Amf,
            ..
        } => push_pair(arguments, "-quality", preset),
        _ => push_pair(arguments, "-preset", preset),
    }
    Ok(())
}

fn is_plain_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

/// Tune, profile, level, fast decode, advanced options, HDR10 and two-pass options.
fn append_video_option_arguments(
    arguments: &mut Vec<OsString>,
    encode: &VideoEncode,
    metadata: &MediaMetadata,
    pass: PassKind,
) -> Result<(), String> {
    let encoder = &encode.encoder;
    let name = video_encoder_name_for_detection(encoder);
    let mut params: Vec<String> = Vec::new();

    let tune = encode
        .tune
        .as_deref()
        .map(str::trim)
        .filter(|tune| !tune.is_empty());
    if let Some(tune) = tune
        && !encoder_tunes(encoder).contains(&tune)
    {
        return Err(format!("Tune {tune} is not available for {name}"));
    }
    if encode.fast_decode && !encoder.supports_fast_decode() {
        return Err(format!("Fast decode is not available for {name}"));
    }
    match encoder {
        VideoEncoder::X264 => {
            let mut tunes = Vec::new();
            tunes.extend(tune);
            if encode.fast_decode && !tunes.contains(&"fastdecode") {
                tunes.push("fastdecode");
            }
            if !tunes.is_empty() {
                push_pair(arguments, "-tune", tunes.join(","));
            }
        }
        VideoEncoder::X265 => match (tune, encode.fast_decode) {
            (Some(tune), true) if tune != "fastdecode" => {
                return Err(
                    "x265 accepts a single tune; turn off fast decode or the tune".to_owned(),
                );
            }
            (Some(tune), _) => push_pair(arguments, "-tune", tune),
            (None, true) => push_pair(arguments, "-tune", "fastdecode"),
            (None, false) => {}
        },
        VideoEncoder::SvtAv1 => {
            if let Some(tune) = tune {
                let value = ["vq", "psnr", "ssim"]
                    .iter()
                    .position(|t| *t == tune)
                    .unwrap_or(0);
                params.push(format!("tune={value}"));
            }
            if encode.fast_decode {
                params.push("fast-decode=1".to_owned());
            }
        }
        VideoEncoder::Hardware { .. } => {
            if let Some(tune) = tune {
                push_pair(arguments, "-tune", tune);
            }
        }
        VideoEncoder::AomAv1 | VideoEncoder::Vp9 => {}
    }

    for (flag, value) in [("-profile:v", &encode.profile), ("-level:v", &encode.level)] {
        if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            if !is_plain_token(value) {
                return Err(format!("Invalid value for {flag}: {value}"));
            }
            push_pair(arguments, flag, value);
        }
    }

    if encode.encoder == VideoEncoder::X265
        && !encode.tone_map_to_sdr
        && metadata.video.as_ref().is_some_and(|video| video.hdr)
        && encode.ten_bit != TenBit::Off
    {
        // Keep HDR10: signal the HDR10 transfer in the stream headers.
        params.extend([
            "hdr10=1".to_owned(),
            "hdr10-opt=1".to_owned(),
            "repeat-headers=1".to_owned(),
        ]);
    }
    if pass == PassKind::First && !encode.turbo_first_pass && encode.encoder.is_x26x() {
        params.push("slow-firstpass=1".to_owned());
    }

    let advanced = encode.advanced_options.trim();
    if !advanced.is_empty() {
        if !encoder.supports_advanced_options() {
            return Err(format!("Advanced options are not available for {name}"));
        }
        if advanced
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '"')
        {
            return Err("Advanced options must be key=value pairs separated by colons".to_owned());
        }
        params.push(advanced.trim_matches(':').to_owned());
    }
    if !params.is_empty() {
        let flag = match encoder {
            VideoEncoder::X264 => "-x264-params",
            VideoEncoder::X265 => "-x265-params",
            _ => "-svtav1-params",
        };
        push_pair(arguments, flag, params.join(":"));
    }
    Ok(())
}

fn quality_value(quality: u8, worst: u32, best: u32) -> String {
    let quality = u32::from(quality.min(100));
    let value = worst as f64 + (best as f64 - worst as f64) * quality as f64 / 100.0;
    value.round().to_string()
}

pub fn quality_control_label(encoder: &VideoEncoder, quality: u8) -> String {
    let (name, value) = match encoder {
        VideoEncoder::X264 | VideoEncoder::X265 => ("CRF", quality_value(quality, 30, 14)),
        VideoEncoder::SvtAv1 | VideoEncoder::AomAv1 => ("CRF", quality_value(quality, 50, 18)),
        VideoEncoder::Vp9 => ("CRF", quality_value(quality, 50, 15)),
        VideoEncoder::Hardware { vendor, .. } => match vendor {
            HwVendor::Nvenc => ("CQ", quality_value(quality, 35, 16)),
            HwVendor::Qsv => ("Quality", quality_value(quality, 35, 16)),
            HwVendor::Amf => ("QP", quality_value(quality, 35, 16)),
            HwVendor::VideoToolbox => ("Quality", quality_value(quality, 30, 85)),
            HwVendor::Vaapi => ("QP", quality_value(quality, 35, 16)),
        },
    };
    format!("{name} {value}")
}

fn push_pair(arguments: &mut Vec<OsString>, flag: &str, value: impl Into<OsString>) {
    arguments.push(flag.into());
    arguments.push(value.into());
}

fn disposition(is_default: bool, is_forced: bool) -> String {
    let value = [
        is_default.then_some("default"),
        is_forced.then_some("forced"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("+");
    if value.is_empty() {
        "0".to_owned()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoders::EncoderSet;
    use crate::mkv::{TrackInfo, VideoInfo};

    fn fixture_metadata() -> MediaMetadata {
        MediaMetadata {
            audio_tracks: vec![
                TrackInfo {
                    id: 1,
                    ffprobe_index: Some(1),
                    language: "eng".to_owned(),
                    codec: "aac".to_owned(),
                    codec_id: "aac".to_owned(),
                    ..TrackInfo::default()
                },
                TrackInfo {
                    id: 2,
                    ffprobe_index: Some(2),
                    language: "jpn".to_owned(),
                    codec: "ac3".to_owned(),
                    codec_id: "ac3".to_owned(),
                    ..TrackInfo::default()
                },
            ],
            subtitle_tracks: vec![
                TrackInfo {
                    id: 3,
                    ffprobe_index: Some(3),
                    language: "eng".to_owned(),
                    codec: "subrip".to_owned(),
                    codec_id: "subrip".to_owned(),
                    ..TrackInfo::default()
                },
                TrackInfo {
                    id: 4,
                    ffprobe_index: Some(4),
                    language: "eng".to_owned(),
                    codec: "hdmv_pgs_subtitle".to_owned(),
                    codec_id: "hdmv_pgs_subtitle".to_owned(),
                    forced: true,
                    ..TrackInfo::default()
                },
            ],
            video_codec: Some("h264".to_owned()),
            video: Some(VideoInfo {
                codec_id: "h264".to_owned(),
                width: Some(1920),
                height: Some(1080),
                interlaced: Some(false),
                ffprobe_index: Some(0),
                ..VideoInfo::default()
            }),
            ..MediaMetadata::default()
        }
    }

    fn all_software_encoders() -> EncoderSet {
        EncoderSet {
            software: vec![
                VideoEncoder::X264,
                VideoEncoder::X265,
                VideoEncoder::SvtAv1,
                VideoEncoder::Vp9,
            ],
            hardware: Vec::new(),
        }
    }

    fn args(settings: &JobSettings) -> Vec<String> {
        build_ffmpeg_job_arguments(
            Path::new("input.mkv"),
            Path::new("output.mkv"),
            &fixture_metadata(),
            settings,
            &all_software_encoders(),
            None,
        )
        .unwrap()
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect()
    }

    #[test]
    fn emits_remux_defaults_and_uses_all_audio_when_languages_do_not_match() {
        let settings = JobSettings::default();
        let arguments = args(&settings);
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:1"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:2"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:3"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:4"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:t?"]));
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-map_chapters", "0"])
        );
        assert!(arguments.iter().any(|arg| arg == "-max_muxing_queue_size"));

        let mut unmatched = settings;
        unmatched.audio.languages = vec!["fra".to_owned()];
        let arguments = args(&unmatched);
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:1"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:2"]));
    }

    #[test]
    fn builds_container_specific_audio_subtitle_and_faststart_flags() {
        let settings = JobSettings {
            container: Container::Mp4,
            video: VideoSettings::Encode(VideoEncode {
                encoder: VideoEncoder::X264,
                ..VideoEncode::default()
            }),
            audio: AudioSettings {
                codec: AudioChoice::Aac,
                mixdown: Mixdown::Stereo,
                ..AudioSettings::default()
            },
            subtitles: SubtitleSettings {
                languages: vec!["eng".to_owned()],
                ..SubtitleSettings::default()
            },
            ..JobSettings::default()
        };
        let args = args(&settings);
        assert!(args.windows(2).any(|pair| pair == ["-c:s:0", "mov_text"]));
        assert!(args.iter().any(|arg| arg == "+faststart"));
        assert!(args.windows(2).any(|pair| pair == ["-ac:a:0", "2"]));
        assert!(args.windows(2).any(|pair| pair == ["-f", "mp4"]));
    }

    #[test]
    fn filters_follow_the_specified_order_and_hdr_gate() {
        let metadata = fixture_metadata();
        let settings = JobSettings {
            video: VideoSettings::Encode(VideoEncode {
                encoder: VideoEncoder::X265,
                max_height: Some(720),
                frame_rate: FrameRate::Fixed(24.0),
                denoise: Denoise::Light,
                crop: Crop::Custom {
                    top: 2,
                    bottom: 4,
                    left: 6,
                    right: 8,
                },
                tone_map_to_sdr: true,
                ..VideoEncode::default()
            }),
            ..JobSettings::default()
        };
        let filters =
            video_filter_chain(Path::new("input.mkv"), &metadata, &settings, None).unwrap();
        assert_eq!(
            filters,
            [
                "crop=iw-6-8:ih-2-4:6:2",
                "scale=-2:'min(ih,720)'",
                "fps=24",
                "hqdn3d=2:1.5:3:2.25"
            ]
        );

        let mut hdr = metadata;
        hdr.video.as_mut().unwrap().hdr = true;
        let filters = video_filter_chain(Path::new("input.mkv"), &hdr, &settings, None).unwrap();
        assert!(filters[1].starts_with("zscale=t=linear"));
    }

    #[test]
    fn x264_uses_eight_bit_420_for_match_source_and_off() {
        for ten_bit in [TenBit::MatchSource, TenBit::Off] {
            let settings = JobSettings {
                video: VideoSettings::Encode(VideoEncode {
                    encoder: VideoEncoder::X264,
                    ten_bit,
                    ..VideoEncode::default()
                }),
                ..JobSettings::default()
            };
            let filters = video_filter_chain(
                Path::new("source.mkv"),
                &fixture_metadata(),
                &settings,
                None,
            )
            .unwrap();
            assert!(filters.iter().any(|filter| filter == "format=yuv420p"));
        }
    }

    #[test]
    fn quality_mapping_covers_best_and_worst_values() {
        assert_eq!(quality_value(0, 30, 14), "30");
        assert_eq!(quality_value(60, 30, 14), "20");
        assert_eq!(quality_value(100, 30, 14), "14");
        assert_eq!(quality_value(0, 50, 18), "50");
        assert_eq!(quality_value(100, 50, 18), "18");
        assert_eq!(quality_value(0, 30, 85), "30");
        assert_eq!(quality_value(100, 30, 85), "85");
        for (worst, best) in [(35, 16), (50, 18), (50, 15), (30, 85)] {
            assert_eq!(quality_value(0, worst, best), worst.to_string());
            assert_eq!(quality_value(60, worst, best), {
                let expected = (worst as f64 + (best as f64 - worst as f64) * 0.6).round();
                expected.to_string()
            });
            assert_eq!(quality_value(100, worst, best), best.to_string());
        }
    }

    #[test]
    fn builds_arguments_for_each_builtin_software_preset() {
        for preset in crate::presets::builtins(&EncoderSet::default()) {
            let arguments = args(&preset.settings);
            match preset.id.as_str() {
                "remux" => assert!(arguments.windows(2).any(|pair| pair == ["-c:v", "copy"])),
                "smaller" => assert!(arguments.windows(2).any(|pair| pair == ["-c:v", "libx265"])),
                "anywhere" => {
                    assert!(arguments.iter().any(|argument| argument == "+faststart"));
                    assert!(arguments.windows(2).any(|pair| pair == ["-c:a:0", "aac"]));
                }
                "archive" => assert!(
                    arguments
                        .windows(2)
                        .any(|pair| pair == ["-c:v", "libsvtav1"])
                ),
                "web" => {
                    assert!(
                        arguments
                            .windows(2)
                            .any(|pair| pair == ["-c:v", "libvpx-vp9"])
                    );
                    assert!(
                        arguments
                            .windows(2)
                            .any(|pair| pair == ["-c:a:0", "libopus"])
                    );
                }
                _ => panic!("unexpected software preset: {}", preset.id),
            }
        }
    }

    #[test]
    fn emits_text_subtitle_burn_in_filter() {
        let settings = JobSettings {
            video: VideoSettings::Encode(VideoEncode {
                max_height: Some(120),
                crop: Crop::Custom {
                    top: 2,
                    bottom: 2,
                    left: 4,
                    right: 4,
                },
                ..VideoEncode::default()
            }),
            subtitles: SubtitleSettings {
                burn_in: BurnIn::FirstSelected,
                ..SubtitleSettings::default()
            },
            ..JobSettings::default()
        };
        let input = Path::new("/tmp/Bob's, [test]; x:y/movie.mkv");
        let arguments = build_ffmpeg_job_arguments(
            input,
            Path::new("output.mp4"),
            &fixture_metadata(),
            &settings,
            &all_software_encoders(),
            Some("kestrel-sub-1-0.ass"),
        )
        .unwrap();
        let filter =
            arguments.windows(2).find(|pair| pair[0] == "-vf").unwrap()[1].to_string_lossy();
        assert!(filter.contains("subtitles=filename=kestrel-sub-1-0.ass"));
        assert!(!filter.contains("/tmp/Bob's, [test]; x:y"));
    }

    #[test]
    fn text_burn_in_requires_a_prepared_subtitle_file() {
        let settings = JobSettings {
            video: VideoSettings::Encode(VideoEncode::default()),
            subtitles: SubtitleSettings {
                burn_in: BurnIn::FirstSelected,
                ..SubtitleSettings::default()
            },
            ..JobSettings::default()
        };
        assert_eq!(
            video_filter_chain(Path::new("input.mkv"), &fixture_metadata(), &settings, None)
                .unwrap_err(),
            "internal: subtitle file not prepared"
        );
    }

    #[test]
    fn converts_mov_text_when_remuxing_to_matroska() {
        let mut metadata = fixture_metadata();
        metadata.subtitle_tracks[0].codec_id = "mov_text".to_owned();
        metadata.subtitle_tracks[0].codec = "mov_text".to_owned();
        let arguments = build_ffmpeg_job_arguments(
            Path::new("input.mp4"),
            Path::new("output.mkv"),
            &metadata,
            &JobSettings::default(),
            &all_software_encoders(),
            None,
        )
        .unwrap()
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
        assert!(arguments.windows(2).any(|pair| pair == ["-c:s:0", "srt"]));
    }

    #[test]
    fn burn_in_omits_the_burned_track_and_uses_image_overlay() {
        let settings = JobSettings {
            container: Container::Mp4,
            video: VideoSettings::Encode(VideoEncode {
                max_height: Some(120),
                crop: Crop::Custom {
                    top: 2,
                    bottom: 2,
                    left: 4,
                    right: 4,
                },
                ..VideoEncode::default()
            }),
            audio: AudioSettings {
                languages: vec!["eng".to_owned()],
                ..AudioSettings::default()
            },
            subtitles: SubtitleSettings {
                languages: vec!["eng".to_owned()],
                burn_in: BurnIn::ForcedOnly,
                ..SubtitleSettings::default()
            },
            ..JobSettings::default()
        };
        let args = args(&settings);
        let complex = args
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .unwrap()[1]
            .clone();
        assert!(complex.contains("[0:s:1]"));
        assert!(
            complex.starts_with("[0:v:0][0:s:1]overlay=eof_action=pass[ov];[ov]crop=iw-4-4:ih-2-2:4:2,scale=-2:'min(ih,120)'"),
            "unexpected image-subtitle filtergraph: {complex}"
        );
        assert!(args.windows(2).any(|pair| pair == ["-map", "0:3"]));
        assert!(!args.windows(2).any(|pair| pair == ["-map", "0:4"]));
    }
    // ---- HandBrake parity: argument-vector tests ----

    fn has(arguments: &[String], pair: [&str; 2]) -> bool {
        arguments.windows(2).any(|window| window == pair)
    }

    fn encode_settings(encode: VideoEncode) -> JobSettings {
        JobSettings {
            video: VideoSettings::Encode(encode),
            ..JobSettings::default()
        }
    }

    fn vf(settings: &JobSettings) -> String {
        args(settings)
            .windows(2)
            .find(|pair| pair[0] == "-vf")
            .map(|pair| pair[1].clone())
            .unwrap_or_default()
    }

    #[test]
    fn old_saved_settings_still_load_with_new_defaults() {
        let old = r#"{"container":"mp4","video":{"encode":{"encoder":"x265","rate":{"quality":60},
            "speed":2,"max_height":720,"frame_rate":"source","deinterlace":"auto","denoise":"light",
            "crop":{"custom":{"top":2,"bottom":2,"left":0,"right":0}},"tone_map_to_sdr":false,
            "ten_bit":"match_source"}},
            "audio":{"languages":["eng"],"codec":"aac","bitrate_kbps":128,"mixdown":"stereo"},
            "subtitles":{"languages":[],"burn_in":"forced_only","include":true},
            "keep_chapters":true,"keep_metadata":true,"title":"Old"}"#;
        let settings: JobSettings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.container, Container::Mp4);
        assert_eq!(settings.title.as_deref(), Some("Old"));
        assert!(settings.web_optimized && settings.keep_attachments);
        assert!(!settings.align_av_start);
        assert!(settings.audio.tracks.is_empty());
        assert_eq!(settings.audio.allowed_passthru, PassthruCodec::ALL);
        assert!(settings.subtitles.external.is_empty());
        let VideoSettings::Encode(encode) = &settings.video else {
            panic!("expected an encode");
        };
        assert_eq!(encode.max_height, Some(720));
        assert_eq!(encode.modulus, 2);
        assert_eq!(encode.passes, 1);
        assert!(encode.turbo_first_pass);
        assert_eq!(encode.border_color, "black");
        assert_eq!(
            settings.subtitles.burn_behavior(),
            BurnBehavior::ForeignAudio
        );
    }

    #[test]
    fn full_settings_round_trip_through_json_and_ron() {
        let mut settings = encode_settings(VideoEncode {
            rotation: Rotation::Deg90,
            border_fill: BorderFill::Custom {
                top: 1,
                bottom: 2,
                left: 3,
                right: 4,
            },
            anamorphic: Anamorphic::Custom { num: 4, den: 3 },
            rate: RateControl::Native(21.5),
            ..VideoEncode::default()
        });
        settings.audio.tracks = vec![AudioTrackSetting {
            encoder: AudioEncoder::Passthru(PassthruCodec::Dts),
            ..AudioTrackSetting::default()
        }];
        settings.subtitles.external = vec![ExternalTrack {
            path: "a.srt".into(),
            language: Some("eng".to_owned()),
            ..ExternalTrack::default()
        }];
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<JobSettings>(&json).unwrap(),
            settings
        );
        let ron_text = ron::to_string(&settings).unwrap();
        assert_eq!(ron::from_str::<JobSettings>(&ron_text).unwrap(), settings);
    }

    #[test]
    fn is_remux_only_accounts_for_tracks_and_burned_imports() {
        let mut settings = JobSettings::default();
        assert!(settings.is_remux_only());
        settings.audio.tracks = vec![AudioTrackSetting::default()];
        assert!(settings.is_remux_only());
        settings.audio.tracks[0].encoder = AudioEncoder::Aac;
        assert!(!settings.is_remux_only());
        let mut settings = JobSettings::default();
        settings.subtitles.external = vec![ExternalTrack {
            path: "a.srt".into(),
            burn_in: true,
            ..ExternalTrack::default()
        }];
        assert!(!settings.is_remux_only());
        assert!(settings.needs_ffmpeg_muxing());
    }

    #[test]
    fn summary_flags_map_to_muxer_arguments() {
        let mut settings = JobSettings {
            container: Container::Mov,
            ..JobSettings::default()
        };
        let arguments = args(&settings);
        assert!(has(&arguments, ["-f", "mov"]));
        assert!(arguments.contains(&"+faststart".to_owned()));

        settings.web_optimized = false;
        settings.align_av_start = true;
        let arguments = args(&settings);
        assert!(!arguments.contains(&"+faststart".to_owned()));
        assert!(has(&arguments, ["-avoid_negative_ts", "make_zero"]));

        let mkv = JobSettings {
            keep_attachments: false,
            ..JobSettings::default()
        };
        assert!(!has(&args(&mkv), ["-map", "0:t?"]));
    }

    #[test]
    fn tags_and_title_become_metadata_arguments() {
        let settings = JobSettings {
            title: Some("Movie".to_owned()),
            tags: MediaTags {
                actors: Some("A, B".to_owned()),
                director: Some("D".to_owned()),
                release_date: Some("2026".to_owned()),
                genre: Some("Drama".to_owned()),
                description: Some("Plot".to_owned()),
                comment: Some("  ".to_owned()),
            },
            ..JobSettings::default()
        };
        let arguments = args(&settings);
        for value in [
            "title=Movie",
            "artist=A, B",
            "director=D",
            "date=2026",
            "genre=Drama",
            "description=Plot",
        ] {
            assert!(has(&arguments, ["-metadata", value]), "missing {value}");
        }
        assert!(!arguments.iter().any(|a| a.starts_with("comment=")));
    }

    #[test]
    fn chapter_names_read_a_prepared_metadata_input() {
        let settings = JobSettings {
            chapter_names: vec!["One".to_owned()],
            ..JobSettings::default()
        };
        let resources = JobResources {
            chapters_file: Some(PathBuf::from("chapters.ffmeta")),
            ..JobResources::default()
        };
        let passes = build_ffmpeg_job_passes(
            Path::new("input.mkv"),
            Path::new("output.mkv"),
            &fixture_metadata(),
            &settings,
            &all_software_encoders(),
            &resources,
        )
        .unwrap();
        let arguments = passes[0]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(has(&arguments, ["-f", "ffmetadata"]));
        assert!(has(&arguments, ["-i", "chapters.ffmeta"]));
        assert!(has(&arguments, ["-map_chapters", "1"]));
    }

    #[test]
    fn dimension_filters_are_ordered_crop_rotate_scale_pad() {
        let settings = encode_settings(VideoEncode {
            crop: Crop::Custom {
                top: 10,
                bottom: 10,
                left: 0,
                right: 0,
            },
            rotation: Rotation::Deg90,
            flip_horizontal: true,
            max_width: Some(100),
            max_height: Some(200),
            modulus: 4,
            border_fill: BorderFill::Both,
            border_color: "#112233".to_owned(),
            deinterlace: Deinterlace::Off,
            ten_bit: TenBit::Off,
            ..VideoEncode::default()
        });
        assert_eq!(
            vf(&settings),
            "crop=iw-0-0:ih-10-10:0:10,transpose=1,hflip,scale=w='min(iw,100)':h='min(ih,200)':force_original_aspect_ratio=decrease:force_divisible_by=4,pad=100:200:(ow-iw)/2:(oh-ih)/2:color=#112233,format=yuv420p"
        );
    }

    #[test]
    fn resolution_limit_variants() {
        let filter = |encode: VideoEncode| {
            let mut encode = encode;
            encode.deinterlace = Deinterlace::Off;
            encode.ten_bit = TenBit::MatchSource;
            encode.encoder = VideoEncoder::X265;
            vf(&encode_settings(encode))
        };
        assert_eq!(
            filter(VideoEncode {
                max_height: Some(720),
                ..VideoEncode::default()
            }),
            "scale=-2:'min(ih,720)'"
        );
        assert_eq!(
            filter(VideoEncode {
                max_width: Some(640),
                allow_upscaling: true,
                ..VideoEncode::default()
            }),
            "scale=640:-2"
        );
        assert_eq!(
            filter(VideoEncode {
                max_width: Some(640),
                max_height: Some(360),
                allow_upscaling: true,
                modulus: 8,
                ..VideoEncode::default()
            }),
            "scale=w=640:h=360:force_original_aspect_ratio=decrease:force_divisible_by=8"
        );
        assert_eq!(
            filter(VideoEncode {
                modulus: 16,
                ..VideoEncode::default()
            }),
            "scale=trunc(iw/16)*16:trunc(ih/16)*16"
        );
    }

    #[test]
    fn anamorphic_and_border_settings_are_validated() {
        let settings = encode_settings(VideoEncode {
            anamorphic: Anamorphic::None,
            deinterlace: Deinterlace::Off,
            encoder: VideoEncoder::X265,
            ..VideoEncode::default()
        });
        assert_eq!(vf(&settings), "scale=trunc(iw*sar/2)*2:ih,setsar=1");
        let settings = encode_settings(VideoEncode {
            anamorphic: Anamorphic::Custom { num: 16, den: 11 },
            deinterlace: Deinterlace::Off,
            encoder: VideoEncoder::X265,
            ..VideoEncode::default()
        });
        assert_eq!(vf(&settings), "setsar=16/11");
        let bad = |encode: VideoEncode| {
            build_ffmpeg_job_arguments(
                Path::new("i.mkv"),
                Path::new("o.mkv"),
                &fixture_metadata(),
                &encode_settings(encode),
                &all_software_encoders(),
                None,
            )
            .unwrap_err()
        };
        assert!(
            bad(VideoEncode {
                modulus: 3,
                ..VideoEncode::default()
            })
            .contains("Modulus")
        );
        assert!(
            bad(VideoEncode {
                border_fill: BorderFill::Both,
                ..VideoEncode::default()
            })
            .contains("maximum")
        );
        assert!(
            bad(VideoEncode {
                border_fill: BorderFill::Custom {
                    top: 1,
                    bottom: 1,
                    left: 0,
                    right: 0
                },
                border_color: "red;drawbox".to_owned(),
                ..VideoEncode::default()
            })
            .contains("border colour")
        );
        assert!(
            bad(VideoEncode {
                crop: Crop::Automatic,
                ..VideoEncode::default()
            })
            .contains("crop was not resolved")
        );
    }

    #[test]
    fn crop_modes_conservative_is_smaller() {
        let detected = Crop::Custom {
            top: 60,
            bottom: 61,
            left: 0,
            right: 5,
        };
        assert_eq!(
            conservative_crop(&detected),
            Crop::Custom {
                top: 56,
                bottom: 56,
                left: 0,
                right: 0
            }
        );
        assert_eq!(
            conservative_crop(&Crop::Custom {
                top: 4,
                bottom: 2,
                left: 0,
                right: 0
            }),
            Crop::None
        );
    }

    #[test]
    fn filter_tab_settings_produce_the_documented_filters() {
        let settings = encode_settings(VideoEncode {
            encoder: VideoEncoder::X265,
            ten_bit: TenBit::MatchSource,
            detelecine: Detelecine::Default,
            deinterlace: Deinterlace::On,
            deinterlace_filter: DeinterlaceFilter::Yadif,
            deinterlace_preset: DeinterlacePreset::Bob,
            denoise: Denoise::Medium,
            denoise_filter: DenoiseFilter::NlMeans,
            denoise_tune: DenoiseTune::Film,
            chroma_smooth: FilterStrength::Medium,
            sharpen: SharpenFilter::LapSharp,
            sharpen_preset: FilterStrength::Light,
            deblock: FilterStrength::Strong,
            colorspace: ColorSpaceTarget::Bt709,
            grayscale: true,
            ..VideoEncode::default()
        });
        let filters = vf(&settings)
            .split(',')
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .join(",");
        let expected = [
            "fieldmatch=order=auto:combmatch=full,decimate",
            "yadif=mode=send_field",
            "zscale=min=709:pin=709:tin=709:m=709:p=709:t=709:r=tv",
            "nlmeans=s=6:p=7:r=7",
            "hqdn3d=0:1.2:0:1.2",
            "cas=strength=0.1",
            "deblock=filter=strong:block=8:alpha=0.098:beta=0.049",
            "hue=s=0",
        ];
        let mut cursor = 0;
        for part in expected {
            let found = filters[cursor..]
                .find(part)
                .unwrap_or_else(|| panic!("{part} missing or out of order in {filters}"));
            cursor += found + part.len();
        }
    }

    #[test]
    fn colorspace_conversion_tags_the_output_stream() {
        let settings = encode_settings(VideoEncode {
            colorspace: ColorSpaceTarget::Bt2020,
            ..VideoEncode::default()
        });
        let arguments = args(&settings);
        assert!(has(&arguments, ["-colorspace", "bt2020nc"]));
        assert!(has(&arguments, ["-color_primaries", "bt2020"]));
        assert!(has(&arguments, ["-color_trc", "bt2020-10"]));
    }

    #[test]
    fn tone_mapping_uses_the_chosen_operator_only_for_hdr_sources() {
        let settings = encode_settings(VideoEncode {
            tone_map_to_sdr: true,
            tone_map_operator: ToneMapOperator::Mobius,
            deinterlace: Deinterlace::Off,
            ..VideoEncode::default()
        });
        assert!(!vf(&settings).contains("tonemap"));
        let mut metadata = fixture_metadata();
        metadata.video.as_mut().unwrap().hdr = true;
        let chain = video_filter_chain(Path::new("i.mkv"), &metadata, &settings, None).unwrap();
        assert!(chain[0].contains("tonemap=tonemap=mobius"));
    }

    #[test]
    fn frame_rate_modes_map_to_fps_filter_and_fps_mode() {
        let settings = |frame_rate, frame_rate_mode| {
            encode_settings(VideoEncode {
                frame_rate,
                frame_rate_mode,
                deinterlace: Deinterlace::Off,
                encoder: VideoEncoder::X265,
                ..VideoEncode::default()
            })
        };
        let cfr = settings(FrameRate::Fixed(30.0), FrameRateMode::Cfr);
        assert_eq!(vf(&cfr), "fps=30");
        assert!(has(&args(&cfr), ["-fps_mode", "cfr"]));
        let vfr = settings(FrameRate::Fixed(30.0), FrameRateMode::Vfr);
        assert_eq!(vf(&vfr), "");
        assert!(has(&args(&vfr), ["-fps_mode", "vfr"]));
        let source = settings(FrameRate::Source, FrameRateMode::Auto);
        assert!(!args(&source).contains(&"-fps_mode".to_owned()));
        // Peak: the fixture has no frame rate, so the cap is applied.
        assert_eq!(
            vf(&settings(FrameRate::Fixed(24.0), FrameRateMode::Pfr)),
            "fps=24"
        );
        let mut metadata = fixture_metadata();
        metadata.video.as_mut().unwrap().fps = Some(23.976);
        let pfr = settings(FrameRate::Fixed(30.0), FrameRateMode::Pfr);
        assert!(
            video_filter_chain(Path::new("i.mkv"), &metadata, &pfr, None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn video_options_map_to_encoder_arguments() {
        let settings = encode_settings(VideoEncode {
            encoder: VideoEncoder::X264,
            rate: RateControl::Native(18.5),
            encoder_preset: Some("slower".to_owned()),
            tune: Some("film".to_owned()),
            fast_decode: true,
            profile: Some("high".to_owned()),
            level: Some("4.1".to_owned()),
            advanced_options: "ref=4:bframes=5".to_owned(),
            ten_bit: TenBit::On,
            ..VideoEncode::default()
        });
        let arguments = args(&settings);
        assert!(has(&arguments, ["-crf", "18.5"]));
        assert!(has(&arguments, ["-preset", "slower"]));
        assert!(has(&arguments, ["-tune", "film,fastdecode"]));
        assert!(has(&arguments, ["-profile:v", "high"]));
        assert!(has(&arguments, ["-level:v", "4.1"]));
        assert!(has(&arguments, ["-x264-params", "ref=4:bframes=5"]));

        let x265 = encode_settings(VideoEncode {
            encoder: VideoEncoder::X265,
            ten_bit: TenBit::Twelve,
            advanced_options: "limit-sao=1".to_owned(),
            ..VideoEncode::default()
        });
        let arguments = args(&x265);
        assert!(has(&arguments, ["-x265-params", "limit-sao=1"]));
        assert!(vf(&x265).ends_with("format=yuv420p12le"));

        let svt = encode_settings(VideoEncode {
            encoder: VideoEncoder::SvtAv1,
            tune: Some("psnr".to_owned()),
            fast_decode: true,
            advanced_options: "film-grain=8".to_owned(),
            ..VideoEncode::default()
        });
        assert!(has(
            &args(&svt),
            ["-svtav1-params", "tune=1:fast-decode=1:film-grain=8"]
        ));
    }

    #[test]
    fn invalid_video_options_are_rejected_with_the_encoder_named() {
        let error = |encode: VideoEncode| {
            build_ffmpeg_job_arguments(
                Path::new("i.mkv"),
                Path::new("o.mkv"),
                &fixture_metadata(),
                &encode_settings(encode),
                &all_software_encoders(),
                None,
            )
            .unwrap_err()
        };
        assert!(
            error(VideoEncode {
                encoder_preset: Some("turbo".to_owned()),
                ..VideoEncode::default()
            })
            .contains("Preset turbo")
        );
        assert!(
            error(VideoEncode {
                encoder: VideoEncoder::Vp9,
                tune: Some("film".to_owned()),
                ..VideoEncode::default()
            })
            .contains("libvpx-vp9")
        );
        assert!(
            error(VideoEncode {
                encoder: VideoEncoder::X265,
                tune: Some("grain".to_owned()),
                fast_decode: true,
                ..VideoEncode::default()
            })
            .contains("single tune")
        );
        assert!(
            error(VideoEncode {
                ten_bit: TenBit::Twelve,
                ..VideoEncode::default()
            })
            .contains("12-bit")
        );
        assert!(
            error(VideoEncode {
                advanced_options: "a=1 b=2".to_owned(),
                ..VideoEncode::default()
            })
            .contains("key=value")
        );
    }

    #[test]
    fn two_pass_bitrate_builds_an_analysis_pass_and_a_final_pass() {
        let settings = encode_settings(VideoEncode {
            encoder: VideoEncoder::X265,
            rate: RateControl::Bitrate(1500),
            passes: 2,
            turbo_first_pass: false,
            ..VideoEncode::default()
        });
        let resources = JobResources {
            pass_log_prefix: Some(PathBuf::from("logs/pass")),
            ..JobResources::default()
        };
        let passes = build_ffmpeg_job_passes(
            Path::new("input.mkv"),
            Path::new("output.mkv"),
            &fixture_metadata(),
            &settings,
            &all_software_encoders(),
            &resources,
        )
        .unwrap();
        assert_eq!(passes.len(), 2);
        let text = |pass: &Vec<OsString>| {
            pass.iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let first = text(&passes[0]);
        let second = text(&passes[1]);
        assert!(has(&first, ["-pass", "1"]) && has(&second, ["-pass", "2"]));
        assert!(has(&first, ["-passlogfile", "logs/pass"]));
        assert!(has(&second, ["-passlogfile", "logs/pass"]));
        assert!(has(&first, ["-x265-params", "slow-firstpass=1"]));
        assert!(!second.iter().any(|a| a.contains("slow-firstpass")));
        assert!(first.contains(&"-an".to_owned()));
        assert_eq!(first.last().map(String::as_str), Some("-"));
        assert!(has(&first, ["-f", "null"]));
        assert!(has(&second, ["-b:v", "1500k"]));
        assert_eq!(second.last().map(String::as_str), Some("output.mkv"));
        assert!(second.contains(&"-map".to_owned()));

        let missing = build_ffmpeg_job_passes(
            Path::new("input.mkv"),
            Path::new("output.mkv"),
            &fixture_metadata(),
            &settings,
            &all_software_encoders(),
            &JobResources::default(),
        );
        assert!(missing.is_err());
        // Single-pass builder (size estimates, previews) never needs pass logs.
        assert!(!args(&settings).contains(&"-passlogfile".to_owned()));
    }

    #[test]
    fn explicit_audio_tracks_override_language_selection() {
        let mut settings = JobSettings {
            container: Container::Mp4,
            ..JobSettings::default()
        };
        settings.audio.languages = vec!["jpn".to_owned()];
        settings.audio.tracks = vec![AudioTrackSetting {
            source: 0,
            encoder: AudioEncoder::Aac,
            mixdown: AudioMixdown::Mono,
            bitrate_kbps: Some(64),
            ..AudioTrackSetting::default()
        }];
        let arguments = args(&settings);
        assert!(has(&arguments, ["-map", "0:1"]));
        assert!(!has(&arguments, ["-map", "0:2"]));
        assert!(has(&arguments, ["-c:a:0", "aac"]));
        assert!(has(&arguments, ["-b:a:0", "64k"]));
        assert!(has(&arguments, ["-ac:a:0", "1"]));
    }

    #[test]
    fn audio_selection_none_drops_all_audio() {
        let mut settings = JobSettings::default();
        settings.audio.selection = TrackSelection::None;
        let arguments = args(&settings);
        assert!(!arguments.iter().any(|a| a.starts_with("-c:a")));
    }

    #[test]
    fn soft_imported_subtitles_use_offset_charset_language_and_container_codec() {
        let mut settings = JobSettings {
            container: Container::Mp4,
            ..JobSettings::default()
        };
        settings.subtitles.languages = vec!["xxx".to_owned()];
        settings.subtitles.external = vec![ExternalTrack {
            path: "sub/en.srt".into(),
            language: Some("eng".to_owned()),
            title: Some("English".to_owned()),
            default: true,
            charset: Some("CP1252".to_owned()),
            offset_ms: 1500,
            ..ExternalTrack::default()
        }];
        let arguments = args(&settings);
        let position = arguments.iter().position(|a| a == "-itsoffset").unwrap();
        assert_eq!(
            arguments[position..position + 6],
            [
                "-itsoffset",
                "1.5",
                "-sub_charenc",
                "CP1252",
                "-i",
                "sub/en.srt"
            ]
        );
        assert!(has(&arguments, ["-map", "1:s:0"]));
        assert!(has(&arguments, ["-c:s:0", "mov_text"]));
        assert!(has(&arguments, ["-metadata:s:s:0", "language=eng"]));
        assert!(has(&arguments, ["-metadata:s:s:0", "title=English"]));
        assert!(has(&arguments, ["-disposition:s:0", "default"]));
    }

    #[test]
    fn imported_picture_subtitles_are_dropped_for_mp4_and_kept_for_mkv() {
        let external = vec![ExternalTrack {
            path: "movie.sup".into(),
            ..ExternalTrack::default()
        }];
        let mut settings = JobSettings {
            container: Container::Mp4,
            ..JobSettings::default()
        };
        settings.subtitles.languages = vec!["xxx".to_owned()];
        settings.subtitles.external = external.clone();
        assert!(!args(&settings).contains(&"movie.sup".to_owned()));
        let warnings = crate::compat::job_warnings(&fixture_metadata(), &settings);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("movie.sup") && w.contains("Picture-based")),
            "{warnings:?}"
        );
        settings.container = Container::Mkv;
        let arguments = args(&settings);
        assert!(has(&arguments, ["-i", "movie.sup"]));
        assert!(has(&arguments, ["-c:s:0", "copy"]));
    }

    #[test]
    fn burned_imported_text_subtitle_escapes_windows_paths_and_applies_offset() {
        let settings = JobSettings {
            container: Container::Mp4,
            video: VideoSettings::Encode(VideoEncode {
                deinterlace: Deinterlace::Off,
                encoder: VideoEncoder::X265,
                ..VideoEncode::default()
            }),
            subtitles: SubtitleSettings {
                external: vec![ExternalTrack {
                    path: r"C:\Users\Bob's, [test]\en.srt".into(),
                    burn_in: true,
                    charset: Some("CP1252".to_owned()),
                    offset_ms: -500,
                    ..ExternalTrack::default()
                }],
                ..SubtitleSettings::default()
            },
            ..JobSettings::default()
        };
        let filter = vf(&settings);
        assert_eq!(
            filter,
            r"setpts=PTS-(-0.5)/TB,subtitles=filename=C\\:/Users/Bob\\\'s\, \[test\]/en.srt:charenc=CP1252,setpts=PTS+(-0.5)/TB"
        );
        // Burned imports never become soft inputs.
        assert!(!args(&settings).contains(&"-itsoffset".to_owned()));
    }

    #[test]
    fn burned_imported_picture_subtitle_overlays_the_second_input() {
        let settings = JobSettings {
            video: VideoSettings::Encode(VideoEncode {
                deinterlace: Deinterlace::Off,
                ..VideoEncode::default()
            }),
            subtitles: SubtitleSettings {
                external: vec![ExternalTrack {
                    path: "movie.sup".into(),
                    burn_in: true,
                    ..ExternalTrack::default()
                }],
                ..SubtitleSettings::default()
            },
            ..JobSettings::default()
        };
        let arguments = args(&settings);
        let complex = arguments
            .windows(2)
            .find(|pair| pair[0] == "-filter_complex")
            .unwrap()[1]
            .clone();
        assert!(complex.starts_with("[0:v:0][1:s:0]overlay=eof_action=pass[ov];[ov]"));
        assert!(has(&arguments, ["-i", "movie.sup"]));
    }

    #[test]
    fn subtitle_selection_forced_only_and_default_rules() {
        let mut settings = JobSettings::default();
        settings.subtitles.forced_only = true;
        let arguments = args(&settings);
        assert!(has(&arguments, ["-map", "0:4"]));
        assert!(!has(&arguments, ["-map", "0:3"]));

        let mut settings = JobSettings::default();
        settings.subtitles.default_track = SubtitleDefault::Forced;
        let arguments = args(&settings);
        assert!(has(&arguments, ["-disposition:s:0", "0"]));
        assert!(has(&arguments, ["-disposition:s:1", "default+forced"]));

        let mut settings = JobSettings::default();
        settings.subtitles.default_track = SubtitleDefault::None;
        assert!(
            !args(&settings)
                .windows(2)
                .any(|w| w[0].starts_with("-disposition:s") && w[1].contains("default"))
        );

        let mut settings = JobSettings::default();
        settings.subtitles.selection = TrackSelection::None;
        assert!(!args(&settings).iter().any(|a| a.starts_with("-c:s")));
    }

    #[test]
    fn foreign_audio_burn_prefers_the_primary_audio_language() {
        let mut metadata = fixture_metadata();
        metadata.subtitle_tracks.push(TrackInfo {
            id: 5,
            ffprobe_index: Some(5),
            language: "jpn".to_owned(),
            codec: "subrip".to_owned(),
            codec_id: "subrip".to_owned(),
            forced: true,
            ..TrackInfo::default()
        });
        let mut settings = JobSettings::default();
        settings
            .subtitles
            .set_burn_behavior(BurnBehavior::ForeignAudio);
        // Primary audio is the first track (eng): the eng forced PGS track wins over jpn.
        let (index, track) = selected_burn_in_subtitle(&metadata, &settings).unwrap();
        assert_eq!((index, track.id), (1, 4));
        settings.audio.languages = vec!["jpn".to_owned()];
        let (_, track) = selected_burn_in_subtitle(&metadata, &settings).unwrap();
        assert_eq!(track.id, 5);

        // No forced track: only the "then first" behaviour burns something.
        metadata
            .subtitle_tracks
            .iter_mut()
            .for_each(|t| t.forced = false);
        assert!(selected_burn_in_subtitle(&metadata, &settings).is_none());
        settings
            .subtitles
            .set_burn_behavior(BurnBehavior::ForeignThenFirst);
        assert_eq!(
            selected_burn_in_subtitle(&metadata, &settings)
                .unwrap()
                .1
                .id,
            3
        );
    }

    #[test]
    fn encoder_choices_cover_the_depths_each_encoder_supports() {
        let choices = encoder_choices();
        let find = |encoder: &VideoEncoder, depth: TenBit| {
            choices
                .iter()
                .any(|choice| &choice.encoder == encoder && choice.ten_bit == depth)
        };
        assert!(find(&VideoEncoder::X264, TenBit::On));
        assert!(find(&VideoEncoder::X265, TenBit::Twelve));
        assert!(!find(&VideoEncoder::X264, TenBit::Twelve));
        assert!(find(&VideoEncoder::SvtAv1, TenBit::On));
        assert!(find(
            &VideoEncoder::Hardware {
                vendor: HwVendor::Qsv,
                codec: HwCodec::Hevc
            },
            TenBit::On
        ));
        let mut encode = VideoEncode::default();
        encode.apply_choice(
            choices
                .iter()
                .find(|c| c.ten_bit == TenBit::Twelve)
                .unwrap(),
        );
        assert_eq!(
            (encode.encoder, encode.ten_bit),
            (VideoEncoder::X265, TenBit::Twelve)
        );
    }
}

#[cfg(test)]
mod aom_tests {
    use super::*;
    #[test]
    fn aom_quality_and_speed_use_supported_ffmpeg_options() {
        let mut args = Vec::new();
        append_video_rate_arguments(&mut args, &VideoEncoder::AomAv1, &RateControl::Quality(65));
        append_video_speed_arguments(&mut args, &VideoEncoder::AomAv1, 1);
        assert_eq!(
            args,
            ["-crf", "29", "-b:v", "0", "-cpu-used", "5", "-row-mt", "1"].map(OsString::from)
        );
    }
}
