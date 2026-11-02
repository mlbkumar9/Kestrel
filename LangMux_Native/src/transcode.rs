use crate::compat::{self, StreamPlan};
use crate::encoders::EncoderSet;
use crate::formats::Container;
use crate::mkv::{self, MediaMetadata, TrackInfo};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::Path;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct JobSettings {
    pub container: Container,
    pub video: VideoSettings,
    pub audio: AudioSettings,
    pub subtitles: SubtitleSettings,
    pub keep_chapters: bool,
    pub keep_metadata: bool,
    pub title: Option<String>,
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
        }
    }
}

impl JobSettings {
    pub fn is_remux_only(&self) -> bool {
        matches!(&self.video, VideoSettings::Copy)
            && self.audio.codec == AudioChoice::Passthrough
            && self.subtitles.burn_in == BurnIn::None
    }
}

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
    pub max_height: Option<u32>,
    pub frame_rate: FrameRate,
    pub deinterlace: Deinterlace,
    pub denoise: Denoise,
    pub crop: Crop,
    pub tone_map_to_sdr: bool,
    pub ten_bit: TenBit,
}

impl Default for VideoEncode {
    fn default() -> Self {
        Self {
            encoder: VideoEncoder::X264,
            rate: RateControl::default(),
            speed: 2,
            max_height: None,
            frame_rate: FrameRate::Source,
            deinterlace: Deinterlace::default(),
            denoise: Denoise::default(),
            crop: Crop::default(),
            tone_map_to_sdr: false,
            ten_bit: TenBit::default(),
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
    Quality(u8),
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
    Custom {
        top: u32,
        bottom: u32,
        left: u32,
        right: u32,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TenBit {
    #[default]
    MatchSource,
    Off,
    On,
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
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            languages: Vec::new(),
            codec: AudioChoice::Passthrough,
            bitrate_kbps: 160,
            mixdown: Mixdown::Source,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct SubtitleSettings {
    pub languages: Vec<String>,
    pub burn_in: BurnIn,
    pub include: bool,
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            languages: Vec::new(),
            burn_in: BurnIn::None,
            include: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BurnIn {
    #[default]
    None,
    ForcedOnly,
    FirstSelected,
}

pub fn video_filter_chain(
    _input: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    text_burn_in_file: Option<&str>,
) -> Result<Vec<String>, String> {
    if selected_burn_in_subtitle(metadata, settings)
        .is_some_and(|(_, track)| is_text_subtitle(&track.codec_id))
        && text_burn_in_file.is_none()
    {
        return Err("internal: subtitle file not prepared".to_owned());
    }
    let Some(encode) = (match &settings.video {
        VideoSettings::Copy => None,
        VideoSettings::Encode(encode) => Some(encode),
    }) else {
        return Ok(Vec::new());
    };

    let mut filters = Vec::new();
    match encode.deinterlace {
        Deinterlace::Off => {}
        Deinterlace::On => filters.push("bwdif=mode=send_frame".to_owned()),
        Deinterlace::Auto => {
            if metadata
                .video
                .as_ref()
                .is_none_or(|video| video.interlaced != Some(false))
            {
                filters.push("bwdif=mode=send_frame:parity=auto:deint=interlaced".to_owned());
            }
        }
    }

    if let Crop::Custom {
        top,
        bottom,
        left,
        right,
    } = &encode.crop
        && (*top != 0 || *bottom != 0 || *left != 0 || *right != 0)
    {
        filters.push(format!(
            "crop=iw-{left}-{right}:ih-{top}-{bottom}:{left}:{top}"
        ));
    }

    if encode.tone_map_to_sdr && metadata.video.as_ref().is_some_and(|video| video.hdr) {
        filters.push(
            "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p"
                .to_owned(),
        );
    }

    if let Some(max_height) = encode.max_height {
        filters.push(format!("scale=-2:'min(ih,{max_height})'"));
    }
    if let FrameRate::Fixed(frame_rate) = &encode.frame_rate {
        filters.push(format!("fps={frame_rate}"));
    }
    match encode.denoise {
        Denoise::Off => {}
        Denoise::Light => filters.push("hqdn3d=2:1.5:3:2.25".to_owned()),
        Denoise::Medium => filters.push("hqdn3d=4:3:6:4.5".to_owned()),
        Denoise::Strong => filters.push("hqdn3d=8:6:12:9".to_owned()),
    }

    if let Some((_, track)) = selected_burn_in_subtitle(metadata, settings)
        && is_text_subtitle(&track.codec_id)
    {
        let filename = text_burn_in_file.expect("validated above");
        filters.push(format!("subtitles=filename={filename}"));
    }

    let is_hardware = matches!(&encode.encoder, VideoEncoder::Hardware { .. });
    match encode.ten_bit {
        TenBit::MatchSource if matches!(&encode.encoder, VideoEncoder::X264) => {
            filters.push("format=yuv420p".to_owned());
        }
        TenBit::MatchSource => {}
        TenBit::Off if !matches!(&encode.encoder, VideoEncoder::X264) => {
            filters.push("format=yuv420p".to_owned());
        }
        TenBit::Off => filters.push("format=yuv420p".to_owned()),
        TenBit::On if is_hardware => filters.push("format=p010le".to_owned()),
        TenBit::On => filters.push("format=yuv420p10le".to_owned()),
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

pub fn build_ffmpeg_job_arguments(
    input: &Path,
    output_tmp: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    encoders: &EncoderSet,
    text_burn_in_file: Option<&str>,
) -> Result<Vec<OsString>, String> {
    let audio_tracks = selected_audio_tracks(metadata, settings);
    let subtitles = if settings.subtitles.include {
        selected_subtitle_tracks(metadata, settings)
    } else {
        Vec::new()
    };
    let burn_in = selected_burn_in_subtitle(metadata, settings);
    let (burn_in_index, burn_track) = burn_in
        .map(|(index, track)| (Some(index), Some(track)))
        .unwrap_or((None, None));
    let video_filters = video_filter_chain(input, metadata, settings, text_burn_in_file)?;
    if burn_track.is_some() && matches!(&settings.video, VideoSettings::Copy) {
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

    let image_burn_in = burn_track
        .filter(|track| !is_text_subtitle(&track.codec_id))
        .map(|_| burn_in_index.unwrap());
    let mut output_subtitles = Vec::new();
    for (relative_index, track) in subtitles.iter().enumerate() {
        if burn_track.is_some_and(|burn_track| std::ptr::eq(*track, burn_track)) {
            continue;
        }
        if matches!(
            compat::subtitle_plan(&track.codec_id, settings.container),
            StreamPlan::Drop(_)
        ) {
            continue;
        }
        output_subtitles.push((relative_index, *track));
    }

    if let Some(subtitle_index) = image_burn_in {
        if !matches!(&settings.video, VideoSettings::Encode(_)) {
            return Err("Subtitle burn-in requires video re-encoding".to_owned());
        }
        let chain = video_filters.join(",");
        let filter_complex = if chain.is_empty() {
            format!("[0:v:0][0:s:{subtitle_index}]overlay=eof_action=pass[vout]")
        } else {
            format!("[0:v:0][0:s:{subtitle_index}]overlay=eof_action=pass[ov];[ov]{chain}[vout]")
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
            if !matches!(
                settings.subtitles.burn_in,
                BurnIn::None | BurnIn::ForcedOnly | BurnIn::FirstSelected
            ) {
                return Err("Unsupported subtitle burn-in mode".to_owned());
            }
            video_encoder_name_for_detection(&encode.encoder).to_owned()
        }
    };
    if let VideoSettings::Encode(encode) = &settings.video {
        append_video_rate_arguments(&mut arguments, &encode.encoder, &encode.rate);
        append_video_speed_arguments(&mut arguments, &encode.encoder, encode.speed);
    }
    arguments.extend(["-c:v".into(), video_codec.into()]);

    for (output_index, track) in audio_tracks.iter().enumerate() {
        let source_codec = if track.codec_id.is_empty() {
            track.codec.as_str()
        } else {
            track.codec_id.as_str()
        };
        let (codec, copy) = if settings.audio.codec == AudioChoice::Passthrough {
            match compat::audio_copy_plan(source_codec, settings.container) {
                StreamPlan::Copy => ("copy", true),
                StreamPlan::Convert(codec) => (codec, false),
                StreamPlan::Drop(reason) => return Err(reason),
            }
        } else {
            let codec = audio_encoder_name(&settings.audio.codec);
            let codec_id = match &settings.audio.codec {
                AudioChoice::Opus => "opus",
                other => audio_encoder_name(other),
            };
            if matches!(
                compat::audio_copy_plan(codec_id, settings.container),
                StreamPlan::Convert(_)
            ) {
                return Err(format!(
                    "Audio encoder {codec} is not compatible with {}",
                    settings.container.label()
                ));
            }
            (codec, false)
        };
        let source_index = track
            .ffprobe_index
            .ok_or_else(|| "FFprobe stream indices are unavailable".to_owned())?;
        arguments.extend([
            "-map".into(),
            format!("0:{source_index}").into(),
            format!("-c:a:{output_index}").into(),
            codec.into(),
        ]);
        if !copy {
            arguments.extend([
                format!("-b:a:{output_index}").into(),
                format!("{}k", settings.audio.bitrate_kbps).into(),
            ]);
            if let Some(channels) = mixdown_channels(settings.audio.mixdown) {
                arguments.extend([format!("-ac:a:{output_index}").into(), channels.into()]);
            }
        }
        arguments.extend([
            format!("-disposition:a:{output_index}").into(),
            disposition(output_index == 0, track.forced).into(),
        ]);
    }

    for (output_index, (relative_index, track)) in output_subtitles.iter().enumerate() {
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
            disposition(output_index == 0, track.forced).into(),
        ]);
        let _ = relative_index;
    }

    if settings.container == Container::Mkv
        && matches!(&settings.video, VideoSettings::Copy)
        && settings.subtitles.burn_in == BurnIn::None
    {
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
        if settings.keep_chapters {
            "0".into()
        } else {
            "-1".into()
        },
    ]);
    if let Some(title) = settings.title.as_deref().filter(|title| !title.is_empty()) {
        arguments.extend(["-metadata".into(), format!("title={title}").into()]);
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
    if matches!(settings.container, Container::Mp4 | Container::Mov) {
        arguments.extend(["-movflags".into(), "+faststart".into()]);
    }
    arguments.push(output_tmp.as_os_str().to_owned());
    Ok(arguments)
}

fn selected_audio_tracks<'a>(
    metadata: &'a MediaMetadata,
    settings: &JobSettings,
) -> Vec<&'a TrackInfo> {
    let languages = mkv::normalize_languages(&settings.audio.languages);
    let selected = metadata
        .audio_tracks
        .iter()
        .filter(|track| {
            languages.is_empty() || languages.contains(&track.language.to_ascii_lowercase())
        })
        .collect::<Vec<_>>();
    if selected.is_empty() && !metadata.audio_tracks.is_empty() {
        metadata.audio_tracks.iter().collect()
    } else {
        selected
    }
}

fn selected_subtitle_tracks<'a>(
    metadata: &'a MediaMetadata,
    settings: &JobSettings,
) -> Vec<&'a TrackInfo> {
    let languages = mkv::normalize_languages(&settings.subtitles.languages);
    metadata
        .subtitle_tracks
        .iter()
        .filter(|track| {
            languages.is_empty() || languages.contains(&track.language.to_ascii_lowercase())
        })
        .collect()
}

pub fn selected_burn_in_subtitle<'a>(
    metadata: &'a MediaMetadata,
    settings: &JobSettings,
) -> Option<(usize, &'a TrackInfo)> {
    if !settings.subtitles.include || settings.subtitles.burn_in == BurnIn::None {
        return None;
    }
    let selected = selected_subtitle_tracks(metadata, settings);
    selected
        .iter()
        .find(|track| settings.subtitles.burn_in != BurnIn::ForcedOnly || track.forced)
        .and_then(|track| {
            metadata
                .subtitle_tracks
                .iter()
                .position(|candidate| std::ptr::eq(candidate, *track))
                .map(|index| (index, *track))
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
            match encoder {
                VideoEncoder::X264 | VideoEncoder::X265 => {
                    push_pair(arguments, "-crf", quality_value(*quality, 30, 14));
                }
                VideoEncoder::SvtAv1 => {
                    push_pair(arguments, "-crf", quality_value(*quality, 50, 18));
                }
                VideoEncoder::Vp9 => {
                    push_pair(arguments, "-crf", quality_value(*quality, 50, 15));
                    push_pair(arguments, "-b:v", "0");
                }
                VideoEncoder::Hardware { vendor, .. } => match vendor {
                    HwVendor::Nvenc => {
                        push_pair(arguments, "-rc", "vbr");
                        push_pair(arguments, "-cq", quality_value(*quality, 35, 16));
                        push_pair(arguments, "-b:v", "0");
                    }
                    HwVendor::Qsv => {
                        push_pair(
                            arguments,
                            "-global_quality",
                            quality_value(*quality, 35, 16),
                        );
                    }
                    HwVendor::Amf => {
                        push_pair(arguments, "-rc", "cqp");
                        let value = quality_value(*quality, 35, 16);
                        push_pair(arguments, "-qp_i", &value);
                        push_pair(arguments, "-qp_p", &value);
                        push_pair(arguments, "-qp_b", &value);
                    }
                    HwVendor::VideoToolbox => {
                        push_pair(arguments, "-q:v", quality_value(*quality, 30, 85));
                    }
                    HwVendor::Vaapi => {
                        push_pair(arguments, "-rc_mode", "CQP");
                        push_pair(arguments, "-qp", quality_value(*quality, 35, 16));
                    }
                },
            }
        }
        RateControl::Bitrate(bitrate) => {
            push_pair(arguments, "-b:v", format!("{bitrate}k"));
            if !matches!(encoder, VideoEncoder::Vp9 | VideoEncoder::SvtAv1) {
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

fn quality_value(quality: u8, worst: u32, best: u32) -> String {
    let quality = u32::from(quality.min(100));
    let value = worst as f64 + (best as f64 - worst as f64) * quality as f64 / 100.0;
    value.round().to_string()
}

pub fn quality_control_label(encoder: &VideoEncoder, quality: u8) -> String {
    let (name, value) = match encoder {
        VideoEncoder::X264 | VideoEncoder::X265 => ("CRF", quality_value(quality, 30, 14)),
        VideoEncoder::SvtAv1 => ("CRF", quality_value(quality, 50, 18)),
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

fn audio_encoder_name(choice: &AudioChoice) -> &'static str {
    match choice {
        AudioChoice::Passthrough => "copy",
        AudioChoice::Aac => "aac",
        AudioChoice::Opus => "libopus",
        AudioChoice::Ac3 => "ac3",
        AudioChoice::Eac3 => "eac3",
        AudioChoice::Flac => "flac",
    }
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

fn mixdown_channels(mixdown: Mixdown) -> Option<&'static str> {
    match mixdown {
        Mixdown::Source => None,
        Mixdown::Stereo => Some("2"),
        Mixdown::Surround51 => Some("6"),
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
            Some("langmux-sub-1-0.ass"),
        )
        .unwrap();
        let filter =
            arguments.windows(2).find(|pair| pair[0] == "-vf").unwrap()[1].to_string_lossy();
        assert!(filter.contains("subtitles=filename=langmux-sub-1-0.ass"));
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
}
