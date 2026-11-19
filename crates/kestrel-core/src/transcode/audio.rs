//! Audio track selection, per-track settings and their FFmpeg arguments (HandBrake Audio tab).

use super::{AudioChoice, JobSettings, Mixdown, disposition};
use crate::compat::{self, StreamPlan};
use crate::mkv::{self, MediaMetadata, TrackInfo};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;

/// How tracks are picked from the source before per-track settings apply.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackSelection {
    /// Every track in a selected language; an empty language list selects all tracks. Audio keeps
    /// all tracks when no language matches.
    #[default]
    AllMatching,
    /// The first track of the first language that matches (languages are tried in list order).
    /// Audio falls back to the first track when nothing matches.
    FirstMatching,
    All,
    None,
}

/// Codecs that can be copied without re-encoding.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PassthruCodec {
    Aac,
    Ac3,
    Eac3,
    Dts,
    /// ffprobe reports DTS-HD as `dts`, so DTS and DTS-HD match the same sources.
    DtsHd,
    TrueHd,
    Flac,
    Mp3,
    Opus,
}

impl PassthruCodec {
    pub const ALL: [PassthruCodec; 9] = [
        Self::Aac,
        Self::Ac3,
        Self::Eac3,
        Self::Dts,
        Self::DtsHd,
        Self::TrueHd,
        Self::Flac,
        Self::Mp3,
        Self::Opus,
    ];

    pub fn matches(self, codec: &str) -> bool {
        let codec = codec.trim().to_ascii_lowercase();
        match self {
            Self::Aac => codec == "aac",
            Self::Ac3 => codec == "ac3",
            Self::Eac3 => codec == "eac3",
            Self::Dts | Self::DtsHd => codec == "dts",
            Self::TrueHd => codec == "truehd",
            Self::Flac => codec == "flac",
            Self::Mp3 => codec == "mp3",
            Self::Opus => codec == "opus",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioEncoder {
    /// Copy when the source codec is in the allowed set and the container can store it.
    #[default]
    AutoPassthru,
    /// Copy only this codec; otherwise use the fallback encoder.
    Passthru(PassthruCodec),
    Aac,
    /// Needs an FFmpeg build with `libfdk_aac`; FFmpeg reports the missing encoder otherwise.
    HeAac,
    Opus,
    Ac3,
    Eac3,
    Flac,
    Mp3,
    Vorbis,
}

impl AudioEncoder {
    pub fn is_passthru(self) -> bool {
        matches!(self, Self::AutoPassthru | Self::Passthru(_))
    }

    /// FFmpeg encoder name; `None` for the passthru variants.
    pub fn ffmpeg_name(self) -> Option<&'static str> {
        match self {
            Self::AutoPassthru | Self::Passthru(_) => None,
            Self::Aac => Some("aac"),
            Self::HeAac => Some("libfdk_aac"),
            Self::Opus => Some("libopus"),
            Self::Ac3 => Some("ac3"),
            Self::Eac3 => Some("eac3"),
            Self::Flac => Some("flac"),
            Self::Mp3 => Some("libmp3lame"),
            Self::Vorbis => Some("libvorbis"),
        }
    }

    fn codec_id(self) -> &'static str {
        match self {
            Self::AutoPassthru | Self::Passthru(_) => "copy",
            Self::Aac | Self::HeAac => "aac",
            Self::Opus => "opus",
            Self::Ac3 => "ac3",
            Self::Eac3 => "eac3",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::Vorbis => "vorbis",
        }
    }

    fn is_lossless(self) -> bool {
        self == Self::Flac
    }
}

impl From<&AudioChoice> for AudioEncoder {
    fn from(choice: &AudioChoice) -> Self {
        match choice {
            AudioChoice::Passthrough => Self::AutoPassthru,
            AudioChoice::Aac => Self::Aac,
            AudioChoice::Opus => Self::Opus,
            AudioChoice::Ac3 => Self::Ac3,
            AudioChoice::Eac3 => Self::Eac3,
            AudioChoice::Flac => Self::Flac,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioMixdown {
    #[default]
    Source,
    Mono,
    Stereo,
    /// Matrix-encoded stereo from 5.1 (a `pan` approximation of Dolby Pro Logic II).
    DolbyProLogicII,
    Surround51,
    Surround71,
}

impl From<Mixdown> for AudioMixdown {
    fn from(mixdown: Mixdown) -> Self {
        match mixdown {
            Mixdown::Source => Self::Source,
            Mixdown::Stereo => Self::Stereo,
            Mixdown::Surround51 => Self::Surround51,
        }
    }
}

impl AudioMixdown {
    fn channels(self) -> Option<&'static str> {
        match self {
            Self::Source => None,
            Self::Mono => Some("1"),
            Self::Stereo | Self::DolbyProLogicII => Some("2"),
            Self::Surround51 => Some("6"),
            Self::Surround71 => Some("8"),
        }
    }
}

/// One explicit output audio track.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct AudioTrackSetting {
    /// Position of the source track in the file's audio tracks (0 is the first audio track).
    pub source: usize,
    pub encoder: AudioEncoder,
    pub mixdown: AudioMixdown,
    /// Output sample rate in Hz; `None` keeps the source rate.
    pub sample_rate: Option<u32>,
    /// `None` uses `AudioSettings::bitrate_kbps`.
    pub bitrate_kbps: Option<u32>,
    pub gain_db: f32,
    /// Dynamic range compression, 0 (off) to 4.
    pub drc: f32,
    pub name: Option<String>,
}

impl Default for AudioTrackSetting {
    fn default() -> Self {
        Self {
            source: 0,
            encoder: AudioEncoder::AutoPassthru,
            mixdown: AudioMixdown::Source,
            sample_rate: None,
            bitrate_kbps: None,
            gain_db: 0.0,
            drc: 0.0,
            name: None,
        }
    }
}

/// A fully resolved output audio track.
#[derive(Debug)]
pub struct AudioOutput<'a> {
    pub track: &'a TrackInfo,
    pub encoder: &'static str,
    pub copy: bool,
    pub bitrate_kbps: Option<u32>,
    pub mixdown: AudioMixdown,
    pub sample_rate: Option<u32>,
    pub gain_db: f32,
    pub drc: f32,
    pub name: Option<&'a str>,
}

/// Picks tracks by language and selection behaviour.
pub fn select_tracks<'a>(
    tracks: &'a [TrackInfo],
    languages: &[String],
    selection: TrackSelection,
    fallback: bool,
) -> Vec<&'a TrackInfo> {
    match selection {
        TrackSelection::None => Vec::new(),
        TrackSelection::All => tracks.iter().collect(),
        TrackSelection::AllMatching => {
            let selected = mkv::matching_tracks(tracks, languages);
            if selected.is_empty() && fallback {
                tracks.iter().collect()
            } else {
                selected
            }
        }
        TrackSelection::FirstMatching => {
            let mut ordered: Vec<String> = Vec::new();
            for language in languages
                .iter()
                .flat_map(|value| value.split(','))
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
            {
                if !ordered.contains(&language) {
                    ordered.push(language);
                }
            }
            let languages = ordered;
            let first = if languages.is_empty() {
                tracks.first()
            } else {
                languages.iter().find_map(|language| {
                    tracks
                        .iter()
                        .find(|track| track.language.eq_ignore_ascii_case(language))
                })
            };
            first
                .or_else(|| fallback.then(|| tracks.first()).flatten())
                .into_iter()
                .collect()
        }
    }
}

/// Audio tracks that reach the output, in output order.
pub fn selected_audio_tracks<'a>(
    metadata: &'a MediaMetadata,
    settings: &JobSettings,
) -> Result<Vec<&'a TrackInfo>, String> {
    if settings.audio.tracks.is_empty() {
        return Ok(select_tracks(
            &metadata.audio_tracks,
            &settings.audio.languages,
            settings.audio.selection,
            true,
        ));
    }
    settings
        .audio
        .tracks
        .iter()
        .map(|setting| {
            metadata.audio_tracks.get(setting.source).ok_or_else(|| {
                format!(
                    "Audio track {} was requested but the source has {} audio tracks",
                    setting.source + 1,
                    metadata.audio_tracks.len()
                )
            })
        })
        .collect()
}

fn source_codec(track: &TrackInfo) -> &str {
    if track.codec_id.is_empty() {
        track.codec.as_str()
    } else {
        track.codec_id.as_str()
    }
}

fn default_fallback(container: crate::formats::Container) -> &'static str {
    match compat::audio_copy_plan("aac", container) {
        StreamPlan::Convert(codec) => codec,
        _ => "aac",
    }
}

/// Chooses between copying and encoding for one track.
/// Returns `(copy, ffmpeg_encoder)`; the encoder is `"copy"` when copying.
fn resolve_encoder(
    track: &TrackInfo,
    encoder: AudioEncoder,
    settings: &JobSettings,
) -> Result<(bool, &'static str), String> {
    let container = settings.container;
    let source = source_codec(track);
    let explicit = |encoder: AudioEncoder| -> Result<(bool, &'static str), String> {
        let name = encoder.ffmpeg_name().expect("encoder is not passthru");
        if matches!(
            compat::audio_copy_plan(encoder.codec_id(), container),
            StreamPlan::Convert(_)
        ) {
            return Err(format!(
                "Audio encoder {name} is not compatible with {}",
                container.label()
            ));
        }
        Ok((false, name))
    };
    let wanted = match encoder {
        // A remux-only job copies whatever the container can hold, whatever the codec.
        AudioEncoder::AutoPassthru => {
            settings.is_remux_only()
                || settings
                    .audio
                    .allowed_passthru
                    .iter()
                    .any(|codec| codec.matches(source))
        }
        AudioEncoder::Passthru(codec) => codec.matches(source),
        other => return explicit(other),
    };
    if wanted {
        match compat::audio_copy_plan(source, container) {
            StreamPlan::Copy => return Ok((true, "copy")),
            StreamPlan::Drop(reason) => return Err(reason),
            StreamPlan::Convert(_) => {}
        }
    }
    match settings.audio.fallback_encoder {
        Some(fallback) if !fallback.is_passthru() => explicit(fallback),
        _ => Ok((false, default_fallback(container))),
    }
}

/// Resolves every output audio track with its encoder, bitrate and filters.
pub fn plan_audio<'a>(
    metadata: &'a MediaMetadata,
    settings: &'a JobSettings,
) -> Result<Vec<AudioOutput<'a>>, String> {
    let tracks = selected_audio_tracks(metadata, settings)?;
    // Remux-only jobs never re-encode: a track the container cannot hold is dropped, and the job
    // fails when that leaves no audio.
    let remux_only = settings.is_remux_only();
    let mut outputs = Vec::with_capacity(tracks.len());
    let mut dropped = Vec::new();
    for (index, track) in tracks.into_iter().enumerate() {
        if remux_only
            && settings
                .track_edits
                .edit_for(track.id)
                .is_some_and(|edit| !edit.keep)
        {
            continue;
        }
        if remux_only
            && let Some(reason) =
                compat::remux_audio_drop_reason(source_codec(track), settings.container)
        {
            dropped.push(reason);
            continue;
        }
        let explicit = settings.audio.tracks.get(index);
        let (encoder, mixdown, sample_rate, bitrate, gain_db, drc, name) = match explicit {
            Some(setting) => (
                setting.encoder,
                setting.mixdown,
                setting.sample_rate,
                setting.bitrate_kbps.unwrap_or(settings.audio.bitrate_kbps),
                setting.gain_db,
                setting.drc,
                setting.name.as_deref().filter(|name| !name.is_empty()),
            ),
            None => (
                AudioEncoder::from(&settings.audio.codec),
                settings.audio.mixdown.into(),
                None,
                settings.audio.bitrate_kbps,
                0.0,
                0.0,
                None,
            ),
        };
        let (copy, name_or_copy) = resolve_encoder(track, encoder, settings)?;
        outputs.push(AudioOutput {
            track,
            encoder: name_or_copy,
            copy,
            bitrate_kbps: (!(copy || explicit.is_some() && encoder.is_lossless()))
                .then_some(bitrate),
            mixdown,
            sample_rate,
            gain_db,
            drc,
            name,
        });
    }
    if outputs.is_empty() && !dropped.is_empty() {
        return Err(compat::remux_audio_failure(&dropped, settings.container));
    }
    Ok(outputs)
}

fn audio_filter(output: &AudioOutput<'_>, align_start: bool) -> Option<String> {
    let mut filters = Vec::new();
    if output.mixdown == AudioMixdown::DolbyProLogicII {
        filters.push(
            "aformat=channel_layouts=5.1(side),pan=stereo|FL=0.5*FL+0.354*FC-0.408*SL-0.289*SR|FR=0.5*FR+0.354*FC+0.289*SL+0.408*SR"
                .to_owned(),
        );
    }
    if output.gain_db != 0.0 {
        filters.push(format!("volume={}dB", output.gain_db));
    }
    if output.drc > 0.0 {
        filters.push(format!(
            "acompressor=threshold=0.1:ratio={}:attack=20:release=250",
            1.0 + output.drc.clamp(0.0, 4.0)
        ));
    }
    if align_start {
        filters.push("aresample=async=1:first_pts=0".to_owned());
    }
    (!filters.is_empty()).then(|| filters.join(","))
}

/// Appends the map, codec and filter arguments for output audio track `index`.
pub fn append_audio_output(
    arguments: &mut Vec<OsString>,
    index: usize,
    output: &AudioOutput<'_>,
    align_start: bool,
) -> Result<(), String> {
    let source_index = output
        .track
        .ffprobe_index
        .ok_or_else(|| "FFprobe stream indices are unavailable".to_owned())?;
    arguments.extend([
        "-map".into(),
        format!("0:{source_index}").into(),
        format!("-c:a:{index}").into(),
        output.encoder.into(),
    ]);
    if !output.copy {
        if let Some(bitrate) = output.bitrate_kbps {
            arguments.extend([format!("-b:a:{index}").into(), format!("{bitrate}k").into()]);
        }
        if let Some(rate) = output.sample_rate {
            arguments.extend([format!("-ar:a:{index}").into(), rate.to_string().into()]);
        }
        if let Some(channels) = output.mixdown.channels()
            && output.mixdown != AudioMixdown::DolbyProLogicII
        {
            arguments.extend([format!("-ac:a:{index}").into(), channels.into()]);
        }
        if let Some(filter) = audio_filter(output, align_start) {
            arguments.extend([format!("-filter:a:{index}").into(), filter.into()]);
        }
    }
    if let Some(name) = output.name {
        arguments.extend([
            format!("-metadata:s:a:{index}").into(),
            format!("title={name}").into(),
        ]);
    }
    arguments.extend([
        format!("-disposition:a:{index}").into(),
        disposition(index == 0, output.track.forced).into(),
    ]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::Container;

    fn track(language: &str, codec: &str, index: u64) -> TrackInfo {
        TrackInfo {
            id: index,
            ffprobe_index: Some(index),
            language: language.to_owned(),
            codec: codec.to_owned(),
            codec_id: codec.to_owned(),
            ..TrackInfo::default()
        }
    }

    fn metadata() -> MediaMetadata {
        MediaMetadata {
            audio_tracks: vec![
                track("eng", "dts", 1),
                track("jpn", "aac", 2),
                track("eng", "truehd", 3),
            ],
            ..MediaMetadata::default()
        }
    }

    #[test]
    fn selection_behaviours_pick_the_expected_tracks() {
        let tracks = metadata().audio_tracks;
        let languages = vec!["jpn".to_owned(), "eng".to_owned()];
        let ids = |selected: Vec<&TrackInfo>| selected.iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(
            ids(select_tracks(
                &tracks,
                &languages,
                TrackSelection::AllMatching,
                true
            )),
            [1, 2, 3]
        );
        assert_eq!(
            ids(select_tracks(
                &tracks,
                &languages,
                TrackSelection::FirstMatching,
                true
            )),
            [2]
        );
        assert_eq!(
            ids(select_tracks(
                &tracks,
                &["fra".to_owned()],
                TrackSelection::AllMatching,
                true
            )),
            [1, 2, 3]
        );
        assert!(
            select_tracks(
                &tracks,
                &["fra".to_owned()],
                TrackSelection::AllMatching,
                false
            )
            .is_empty()
        );
        assert_eq!(
            ids(select_tracks(&tracks, &[], TrackSelection::All, true)),
            [1, 2, 3]
        );
        assert!(select_tracks(&tracks, &[], TrackSelection::None, true).is_empty());
    }

    #[test]
    fn remux_only_drops_unstorable_audio_and_fails_when_none_is_left() {
        // Tracks: 0 dts (eng), 1 aac (jpn), 2 truehd (eng).
        let metadata = metadata();
        let mut settings = JobSettings {
            container: Container::Mp4,
            ..JobSettings::default()
        };
        assert!(settings.is_remux_only());
        let plan = plan_audio(&metadata, &settings).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!((plan[0].track.id, plan[0].copy), (2, true));

        let dts_only = MediaMetadata {
            audio_tracks: vec![metadata.audio_tracks[0].clone()],
            ..MediaMetadata::default()
        };
        assert_eq!(
            plan_audio(&dts_only, &settings).unwrap_err(),
            "DTS audio cannot be stored in MP4; choose MKV or use Transcode"
        );
        // MKV holds everything, including codecs outside the passthru list.
        settings.container = Container::Mkv;
        let mut pcm = dts_only;
        pcm.audio_tracks[0].codec = "pcm_s16le".to_owned();
        pcm.audio_tracks[0].codec_id = "pcm_s16le".to_owned();
        let plan = plan_audio(&pcm, &settings).unwrap();
        assert_eq!((plan[0].copy, plan[0].encoder), (true, "copy"));
    }

    #[test]
    fn passthru_falls_back_when_not_allowed_or_not_storable() {
        let metadata = metadata();
        let mut settings = JobSettings {
            container: Container::Mp4,
            video: crate::transcode::VideoSettings::Encode(Default::default()),
            ..JobSettings::default()
        };
        settings.audio.tracks = vec![
            AudioTrackSetting {
                source: 0,
                ..AudioTrackSetting::default()
            },
            AudioTrackSetting {
                source: 1,
                ..AudioTrackSetting::default()
            },
        ];
        let plan = plan_audio(&metadata, &settings).unwrap();
        assert_eq!((plan[0].copy, plan[0].encoder), (false, "aac"));
        assert_eq!((plan[1].copy, plan[1].encoder), (true, "copy"));

        settings.audio.allowed_passthru = vec![PassthruCodec::Ac3];
        settings.audio.fallback_encoder = Some(AudioEncoder::Opus);
        let plan = plan_audio(&metadata, &settings).unwrap();
        assert_eq!((plan[1].copy, plan[1].encoder), (false, "libopus"));
    }

    #[test]
    fn explicit_tracks_emit_bitrate_rate_mixdown_gain_and_drc() {
        let metadata = metadata();
        let mut settings = JobSettings::default();
        settings.audio.tracks = vec![AudioTrackSetting {
            source: 1,
            encoder: AudioEncoder::Opus,
            mixdown: AudioMixdown::Stereo,
            sample_rate: Some(48000),
            bitrate_kbps: Some(96),
            gain_db: 3.0,
            drc: 1.0,
            name: Some("Main".to_owned()),
        }];
        let plan = plan_audio(&metadata, &settings).unwrap();
        let mut arguments = Vec::new();
        append_audio_output(&mut arguments, 0, &plan[0], false).unwrap();
        let text = arguments
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        for pair in [
            ["-map", "0:2"],
            ["-c:a:0", "libopus"],
            ["-b:a:0", "96k"],
            ["-ar:a:0", "48000"],
            ["-ac:a:0", "2"],
            ["-metadata:s:a:0", "title=Main"],
        ] {
            assert!(
                text.windows(2).any(|w| w == pair),
                "missing {pair:?} in {text:?}"
            );
        }
        let filter = text.windows(2).find(|w| w[0] == "-filter:a:0").unwrap();
        assert_eq!(
            filter[1],
            "volume=3dB,acompressor=threshold=0.1:ratio=2:attack=20:release=250"
        );
    }

    #[test]
    fn explicit_encoder_incompatible_with_the_container_is_rejected() {
        let mut settings = JobSettings {
            container: Container::Mp4,
            ..JobSettings::default()
        };
        settings.audio.tracks = vec![AudioTrackSetting {
            encoder: AudioEncoder::Vorbis,
            ..AudioTrackSetting::default()
        }];
        assert_eq!(
            plan_audio(&metadata(), &settings).unwrap_err(),
            "Audio encoder libvorbis is not compatible with MP4"
        );
    }

    #[test]
    fn out_of_range_source_track_is_an_error() {
        let mut settings = JobSettings::default();
        settings.audio.tracks = vec![AudioTrackSetting {
            source: 9,
            ..AudioTrackSetting::default()
        }];
        assert!(plan_audio(&metadata(), &settings).is_err());
    }
}
