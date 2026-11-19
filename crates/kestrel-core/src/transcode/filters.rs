//! Video filter settings and their FFmpeg filter strings (HandBrake Dimensions and Filters tabs).
//!
//! Control names and numeric preset values follow HandBrake's documented presets; the FFmpeg
//! filter arguments are Kestrel's own translation. Where FFmpeg has no exact counterpart the
//! doc comment of the variant says what it is approximated by.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Clockwise rotation applied after cropping.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Rotation {
    pub fn filters(self) -> &'static [&'static str] {
        match self {
            Self::Deg0 => &[],
            Self::Deg90 => &["transpose=1"],
            Self::Deg180 => &["hflip", "vflip"],
            Self::Deg270 => &["transpose=2"],
        }
    }

    pub const fn degrees(self) -> u32 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }
}

/// Pixel aspect ratio handling.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Anamorphic {
    /// Keep the stored resolution and the source pixel aspect ratio (no filter).
    #[default]
    Automatic,
    /// Resample to square pixels (`scale=iw*sar:ih,setsar=1`).
    None,
    /// Keep the stored resolution and force this pixel aspect ratio.
    Custom { num: u32, den: u32 },
}

/// Which borders are added so the output fills the resolution limit.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderFill {
    #[default]
    None,
    /// Pad the height up to the maximum height (letterbox).
    Height,
    /// Pad the width up to the maximum width (pillarbox).
    Width,
    /// Pad both up to the maximum width and height.
    Both,
    /// Add fixed borders in pixels.
    Custom {
        top: u32,
        bottom: u32,
        left: u32,
        right: u32,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Detelecine {
    #[default]
    Off,
    /// `fieldmatch` followed by `decimate`.
    Default,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeinterlaceFilter {
    #[default]
    Bwdif,
    Yadif,
    /// Approximation of HandBrake's Decomb: `fieldmatch` followed by a selective `yadif`.
    Decomb,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeinterlacePreset {
    /// One frame per frame with the spatial check.
    #[default]
    Default,
    /// One frame per frame without the spatial check.
    Fast,
    /// One frame per field (doubles the frame rate).
    Bob,
}

impl DeinterlacePreset {
    fn mode(self, filter: DeinterlaceFilter) -> &'static str {
        match (self, filter) {
            (Self::Default, _) => "send_frame",
            (Self::Fast, DeinterlaceFilter::Bwdif) => "send_frame",
            (Self::Fast, _) => "send_frame_nospatial",
            (Self::Bob, _) => "send_field",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DenoiseFilter {
    #[default]
    Hqdn3d,
    NlMeans,
}

/// NLMeans tune (HandBrake names).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DenoiseTune {
    #[default]
    None,
    Film,
    Grain,
    HighMotion,
    Animation,
    Tape,
    Sprite,
}

/// Strength presets shared by chroma smooth, sharpen and deblock.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterStrength {
    #[default]
    Off,
    UltraLight,
    Light,
    Medium,
    Strong,
    Stronger,
    VeryStrong,
}

impl FilterStrength {
    fn index(self) -> Option<usize> {
        match self {
            Self::Off => None,
            Self::UltraLight => Some(0),
            Self::Light => Some(1),
            Self::Medium => Some(2),
            Self::Strong => Some(3),
            Self::Stronger => Some(4),
            Self::VeryStrong => Some(5),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SharpenFilter {
    #[default]
    Off,
    Unsharp,
    /// HandBrake's LapSharp, approximated by FFmpeg's contrast adaptive sharpen (`cas`).
    LapSharp,
}

/// Target colour space for a colourspace conversion.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpaceTarget {
    #[default]
    Keep,
    Bt709,
    Bt601,
    Bt2020,
}

impl ColorSpaceTarget {
    /// FFmpeg `-colorspace`, `-color_primaries` and `-color_trc` values for the target.
    pub fn tags(self) -> Option<(&'static str, &'static str, &'static str)> {
        match self {
            Self::Keep => None,
            Self::Bt709 => Some(("bt709", "bt709", "bt709")),
            Self::Bt601 => Some(("smpte170m", "smpte170m", "smpte170m")),
            Self::Bt2020 => Some(("bt2020nc", "bt2020", "bt2020-10")),
        }
    }

    fn zscale_target(self) -> Option<&'static str> {
        match self {
            Self::Keep => None,
            Self::Bt709 => Some("m=709:p=709:t=709"),
            Self::Bt601 => Some("m=170m:p=170m:t=601"),
            Self::Bt2020 => Some("m=2020_ncl:p=2020:t=2020_10"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToneMapOperator {
    #[default]
    Hable,
    Mobius,
    Reinhard,
    /// Needs FFmpeg's `libplacebo` filter and a Vulkan device.
    Bt2390,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameRateMode {
    /// Leave frame timing to the container (no `-fps_mode`).
    #[default]
    Auto,
    Cfr,
    /// Peak frame rate: variable timing, capped at the chosen rate.
    Pfr,
    Vfr,
}

pub fn detelecine_filter(mode: Detelecine) -> Option<&'static str> {
    match mode {
        Detelecine::Off => None,
        Detelecine::Default => Some("fieldmatch=order=auto:combmatch=full,decimate"),
    }
}

/// Interlace removal filter for the Yadif/Bwdif/Decomb choice.
/// `selective` limits the filter to frames flagged as interlaced.
pub fn deinterlace_filter(
    filter: DeinterlaceFilter,
    preset: DeinterlacePreset,
    selective: bool,
) -> String {
    let mode = preset.mode(filter);
    let deint = if selective {
        ":parity=auto:deint=interlaced"
    } else {
        ""
    };
    match filter {
        DeinterlaceFilter::Bwdif => format!("bwdif=mode={mode}{deint}"),
        DeinterlaceFilter::Yadif => format!("yadif=mode={mode}{deint}"),
        DeinterlaceFilter::Decomb => {
            format!("fieldmatch=order=auto:combmatch=full,yadif=mode={mode}:deint=interlaced")
        }
    }
}

/// NLMeans parameters `(strength, patch, research)` per tune and preset (0 light, 1 medium, 2 strong).
fn nlmeans_values(tune: DenoiseTune, preset: usize) -> (f64, u32, u32) {
    let (strength, patch, range): ([f64; 3], u32, u32) = match tune {
        DenoiseTune::None => ([3.0, 6.0, 10.0], 7, 3),
        DenoiseTune::Film | DenoiseTune::HighMotion => ([3.0, 6.0, 8.0], 7, 3),
        DenoiseTune::Grain => ([3.5, 6.0, 8.0], 7, 3),
        DenoiseTune::Animation => ([3.0, 5.0, 10.0], 5, 7),
        DenoiseTune::Tape => ([2.0, 3.0, 3.5], 3, 5),
        DenoiseTune::Sprite => ([2.0, 3.0, 3.0], 5, 5),
    };
    (strength[preset], patch, 2 * range + 1)
}

pub fn nlmeans_filter(tune: DenoiseTune, preset: usize) -> String {
    let (strength, patch, research) = nlmeans_values(tune, preset);
    format!("nlmeans=s={strength}:p={patch}:r={research}")
}

pub fn chroma_smooth_filter(strength: FilterStrength) -> Option<String> {
    let value = [0.4, 0.8, 1.2, 1.6, 2.0, 2.4][strength.index()?];
    Some(format!("hqdn3d=0:{value}:0:{value}"))
}

pub fn sharpen_filter(filter: SharpenFilter, strength: FilterStrength) -> Option<String> {
    let index = strength.index()?;
    match filter {
        SharpenFilter::Off => None,
        SharpenFilter::Unsharp => {
            let amount = [0.05, 0.15, 0.25, 0.5, 0.8, 1.2][index];
            Some(format!("unsharp=7:7:{amount}:7:7:{amount}"))
        }
        SharpenFilter::LapSharp => {
            let amount = [0.05, 0.1, 0.2, 0.3, 0.5, 1.0][index];
            Some(format!("cas=strength={amount}"))
        }
    }
}

pub fn deblock_filter(strength: FilterStrength) -> Option<String> {
    let index = strength.index()?;
    let kind = if index < 2 { "weak" } else { "strong" };
    let threshold = [20.0_f64, 50.0, 20.0, 50.0, 75.0, 100.0][index];
    let alpha = 0.098 * threshold / 50.0;
    let beta = alpha / 2.0;
    Some(format!(
        "deblock=filter={kind}:block=8:alpha={alpha:.3}:beta={beta:.3}"
    ))
}

/// Colourspace conversion with the source assumed to be BT.709 for HD and BT.601 below 720 lines
/// (the input is untagged in most files, and `zscale` needs an explicit input description).
pub fn colorspace_filter(target: ColorSpaceTarget, source_height: Option<u32>) -> Option<String> {
    let output = target.zscale_target()?;
    let input = if source_height.is_some_and(|height| height < 720) {
        "min=170m:pin=170m:tin=601"
    } else {
        "min=709:pin=709:tin=709"
    };
    Some(format!("zscale={input}:{output}:r=tv"))
}

pub fn tone_map_filter(operator: ToneMapOperator) -> String {
    match operator {
        ToneMapOperator::Hable => {
            "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p"
                .to_owned()
        }
        ToneMapOperator::Mobius | ToneMapOperator::Reinhard => {
            let name = if operator == ToneMapOperator::Mobius {
                "mobius"
            } else {
                "reinhard"
            };
            format!(
                "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap={name}:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p"
            )
        }
        ToneMapOperator::Bt2390 => {
            "libplacebo=tonemapping=bt.2390:colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv:format=yuv420p"
                .to_owned()
        }
    }
}

/// A border colour usable inside a filter argument: a colour name or `#RRGGBB`/`0xRRGGBB`.
pub fn is_valid_color(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '#' | '@' | '.'))
}

/// Escapes a file path for the `filename` argument of a filter inside an FFmpeg filtergraph.
///
/// Applies both escaping levels (filter option, then filtergraph) so drive-letter colons,
/// apostrophes, commas and brackets survive on Windows. Backslashes become forward slashes.
pub fn escape_filter_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    escape_chars(
        &escape_chars(&text, &['\\', '\'', ':']),
        &['\\', '\'', '[', ']', ',', ';'],
    )
}

fn escape_chars(text: &str, special: &[char]) -> String {
    let mut escaped = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        if special.contains(&ch) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_maps_to_transpose_and_flip_filters() {
        assert_eq!(Rotation::Deg90.filters(), ["transpose=1"]);
        assert_eq!(Rotation::Deg180.filters(), ["hflip", "vflip"]);
        assert_eq!(Rotation::Deg270.filters(), ["transpose=2"]);
        assert!(Rotation::Deg0.filters().is_empty());
    }

    #[test]
    fn deinterlace_filters_cover_each_engine_and_preset() {
        assert_eq!(
            deinterlace_filter(DeinterlaceFilter::Bwdif, DeinterlacePreset::Default, true),
            "bwdif=mode=send_frame:parity=auto:deint=interlaced"
        );
        assert_eq!(
            deinterlace_filter(DeinterlaceFilter::Yadif, DeinterlacePreset::Fast, false),
            "yadif=mode=send_frame_nospatial"
        );
        assert_eq!(
            deinterlace_filter(DeinterlaceFilter::Yadif, DeinterlacePreset::Bob, false),
            "yadif=mode=send_field"
        );
        assert!(
            deinterlace_filter(DeinterlaceFilter::Decomb, DeinterlacePreset::Default, false)
                .starts_with("fieldmatch=")
        );
    }

    #[test]
    fn nlmeans_presets_follow_the_tune() {
        assert_eq!(nlmeans_filter(DenoiseTune::None, 1), "nlmeans=s=6:p=7:r=7");
        assert_eq!(
            nlmeans_filter(DenoiseTune::Animation, 2),
            "nlmeans=s=10:p=5:r=15"
        );
        assert_eq!(nlmeans_filter(DenoiseTune::Tape, 0), "nlmeans=s=2:p=3:r=11");
    }

    #[test]
    fn sharpen_chroma_and_deblock_presets_map_to_ffmpeg_filters() {
        assert_eq!(
            sharpen_filter(SharpenFilter::Unsharp, FilterStrength::Medium).unwrap(),
            "unsharp=7:7:0.25:7:7:0.25"
        );
        assert_eq!(
            sharpen_filter(SharpenFilter::LapSharp, FilterStrength::VeryStrong).unwrap(),
            "cas=strength=1"
        );
        assert_eq!(
            chroma_smooth_filter(FilterStrength::Light).unwrap(),
            "hqdn3d=0:0.8:0:0.8"
        );
        assert!(
            deblock_filter(FilterStrength::Light)
                .unwrap()
                .contains("weak")
        );
        assert!(
            deblock_filter(FilterStrength::Strong)
                .unwrap()
                .contains("strong")
        );
        assert_eq!(
            sharpen_filter(SharpenFilter::Unsharp, FilterStrength::Off),
            None
        );
        assert_eq!(deblock_filter(FilterStrength::Off), None);
    }

    #[test]
    fn tone_map_operators_select_the_filter() {
        assert!(tone_map_filter(ToneMapOperator::Hable).contains("tonemap=hable"));
        assert!(tone_map_filter(ToneMapOperator::Mobius).contains("tonemap=mobius"));
        assert!(tone_map_filter(ToneMapOperator::Reinhard).contains("tonemap=reinhard"));
        assert!(tone_map_filter(ToneMapOperator::Bt2390).starts_with("libplacebo="));
    }

    #[test]
    fn filter_paths_are_escaped_for_windows_and_special_characters() {
        assert_eq!(
            escape_filter_path(Path::new(r"C:\Users\Bob's, [test]\a;b.srt")),
            r"C\\:/Users/Bob\\\'s\, \[test\]/a\;b.srt"
        );
        assert_eq!(
            escape_filter_path(Path::new("/tmp/a b.srt")),
            "/tmp/a b.srt"
        );
    }

    #[test]
    fn colorspace_conversion_names_the_assumed_input() {
        assert_eq!(
            colorspace_filter(ColorSpaceTarget::Bt709, Some(480)).unwrap(),
            "zscale=min=170m:pin=170m:tin=601:m=709:p=709:t=709:r=tv"
        );
        assert!(colorspace_filter(ColorSpaceTarget::Keep, None).is_none());
    }
}
