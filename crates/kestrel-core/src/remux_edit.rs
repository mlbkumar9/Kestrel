//! Per-track remux edits and externally added subtitle/audio files.
//!
//! [TrackEdits] sits on [crate::transcode::JobSettings] and refines the language-based track
//! selection of a stream-copy remux. An empty value changes nothing. [plan] resolves the edits
//! into one ordered track list that both engines turn into arguments:
//! [build_mkvmerge_arguments] for MKVToolNix and [build_ffmpeg_remux_arguments] for FFmpeg.
//!
//! Rules, in the order they are applied:
//! 1. Language selection runs first (as without edits); `keep: false` then removes tracks from
//!    that selection. An edit never resurrects a track the language selection excluded.
//! 2. Source tracks are ordered by track id. Ids in `order` move to the front in the listed
//!    order; external files always follow the source tracks, in list order.
//! 3. Per type (audio, subtitle) at most one track is default. If any track is explicitly
//!    set default (an edit with `default: Some(true)` or an external with `default: true`),
//!    the last such track in output order wins and every other track of that type is cleared.
//!    Without an explicit default, the first source default is kept and later ones are cleared.
//!    If the default track is dropped, no replacement is promoted.
//! 4. Language overrides must be ISO 639-2 codes (three ASCII letters, `und` allowed) and are
//!    normalised to lowercase; anything else fails the job before the tool starts.
//! 5. Under FFmpeg, subtitles the target container cannot hold (picture subtitles in
//!    MP4/MOV/WebM) are skipped with a warning; the job still runs.

use crate::compat::{self, StreamPlan};
use crate::external::{ExternalKind, ExternalTrack};
use crate::formats::Container;
use crate::mkv::{self, MediaMetadata, MkvRemuxSettings, TrackInfo};
use crate::transcode::JobSettings;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::Path;

/// Overrides for one source track, matched by [TrackInfo::id].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct TrackEdit {
    pub id: u64,
    /// `false` drops the track from the output.
    pub keep: bool,
    /// `None` keeps the source flag.
    pub default: Option<bool>,
    /// `None` keeps the source flag.
    pub forced: Option<bool>,
    /// ISO 639-2 code; `None` keeps the source language.
    pub language: Option<String>,
    /// Track name; `None` keeps the source name, an empty string clears it.
    pub title: Option<String>,
}

impl Default for TrackEdit {
    fn default() -> Self {
        Self {
            id: 0,
            keep: true,
            default: None,
            forced: None,
            language: None,
            title: None,
        }
    }
}

/// Track edits and added files of one remux job. The default value is a no-op.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct TrackEdits {
    pub tracks: Vec<TrackEdit>,
    /// Source track ids placed first in the output, in this order.
    pub order: Vec<u64>,
    /// External files added to remux (stream-copy) jobs. Transcode jobs import subtitles through
    /// `SubtitleSettings::external` instead.
    pub external: Vec<ExternalTrack>,
}

impl TrackEdits {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.order.is_empty() && self.external.is_empty()
    }

    pub(crate) fn edit_for(&self, id: u64) -> Option<&TrackEdit> {
        self.tracks.iter().find(|edit| edit.id == id)
    }

    /// Fails when an added file is missing, so a job never starts a tool on a broken input.
    pub fn check_external_files(&self) -> Result<(), String> {
        for track in &self.external {
            if !track.path.is_file() {
                return Err(format!(
                    "External track file not found: {}",
                    track.path.display()
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Engine {
    Mkvmerge,
    Ffmpeg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackKind {
    Audio,
    Subtitle,
}

impl TrackKind {
    fn label(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Subtitle => "subtitle",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Origin<'a> {
    Source(&'a TrackInfo),
    External(&'a ExternalTrack),
}

/// One output track after edits, selection and container rules.
#[derive(Clone, Debug)]
pub struct PlannedTrack<'a> {
    pub kind: TrackKind,
    pub origin: Origin<'a>,
    pub codec_id: String,
    /// Normalised override; `None` keeps the source value.
    pub language: Option<String>,
    pub title: Option<String>,
    pub default: bool,
    pub forced: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RemuxPlan<'a> {
    pub tracks: Vec<PlannedTrack<'a>>,
    pub warnings: Vec<String>,
    /// Reasons (`compat::remux_audio_drop_reason`) for source audio tracks the container cannot
    /// hold; they are left out of `tracks`.
    pub dropped_source_audio: Vec<String>,
}

/// Validates and lowercases an ISO 639-2 language code.
pub fn normalize_language_code(value: &str) -> Result<String, String> {
    let codes = mkv::parse_language_field(value);
    match codes.as_slice() {
        [code] if code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_lowercase()) => {
            Ok(code.clone())
        }
        _ => Err(format!(
            "Invalid language code '{value}': use a three-letter ISO 639-2 code such as eng"
        )),
    }
}

/// Codec id of an external file from its extension; `None` for unsupported text/image subtitles.
fn external_codec_id(track: &ExternalTrack) -> Option<&'static str> {
    let extension = track
        .path
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase();
    match track.kind {
        ExternalKind::Subtitle => match extension.as_str() {
            "srt" => Some("subrip"),
            "ass" => Some("ass"),
            "ssa" => Some("ssa"),
            "vtt" => Some("webvtt"),
            "sup" => Some("hdmv_pgs_subtitle"),
            _ => None,
        },
        ExternalKind::Audio => Some(match extension.as_str() {
            "ac3" => "ac3",
            "eac3" | "ec3" => "eac3",
            "dts" => "dts",
            "flac" => "flac",
            "aac" | "m4a" => "aac",
            "opus" => "opus",
            "mp3" => "mp3",
            "ogg" | "oga" => "vorbis",
            "wav" => "pcm_s16le",
            _ => "unknown",
        }),
    }
}

fn kind_of(track: &ExternalTrack) -> TrackKind {
    match track.kind {
        ExternalKind::Subtitle => TrackKind::Subtitle,
        ExternalKind::Audio => TrackKind::Audio,
    }
}

fn describe(track: &PlannedTrack<'_>) -> String {
    match track.origin {
        Origin::Source(source) => format!("{} track {}", track.kind.label(), source.id),
        Origin::External(external) => format!(
            "external {} {}",
            track.kind.label(),
            external
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
    }
}

/// Resolves selection, edits, ordering, container rules and the default rule into one track list.
pub fn plan<'a>(
    metadata: &'a MediaMetadata,
    audio_languages: &[String],
    subtitle_languages: &[String],
    include_subtitles: bool,
    edits: &'a TrackEdits,
    engine: Engine,
    container: Container,
) -> Result<RemuxPlan<'a>, String> {
    let mut audio = mkv::matching_tracks(&metadata.audio_tracks, audio_languages);
    if audio.is_empty() && !metadata.audio_tracks.is_empty() {
        audio = metadata.audio_tracks.iter().collect();
    }
    let subtitles = if include_subtitles {
        mkv::matching_tracks(&metadata.subtitle_tracks, subtitle_languages)
    } else {
        Vec::new()
    };
    let mut sources: Vec<(TrackKind, &TrackInfo)> = audio
        .into_iter()
        .map(|track| (TrackKind::Audio, track))
        .chain(
            subtitles
                .into_iter()
                .map(|track| (TrackKind::Subtitle, track)),
        )
        .filter(|(_, track)| edits.edit_for(track.id).is_none_or(|edit| edit.keep))
        .collect();
    sources.sort_by_key(|(_, track)| track.id);
    let mut ordered = Vec::with_capacity(sources.len());
    for id in &edits.order {
        if let Some(position) = sources.iter().position(|(_, track)| track.id == *id) {
            ordered.push(sources.remove(position));
        }
    }
    ordered.extend(sources);

    let mut tracks = Vec::new();
    let mut explicit_default = Vec::new();
    for (kind, track) in ordered {
        let edit = edits.edit_for(track.id);
        let language = edit
            .and_then(|edit| edit.language.as_deref())
            .map(normalize_language_code)
            .transpose()?;
        explicit_default.push(edit.and_then(|edit| edit.default) == Some(true));
        tracks.push(PlannedTrack {
            kind,
            origin: Origin::Source(track),
            codec_id: track.codec_id.clone(),
            language,
            title: edit.and_then(|edit| edit.title.clone()),
            default: edit.and_then(|edit| edit.default).unwrap_or(track.default),
            forced: edit.and_then(|edit| edit.forced).unwrap_or(track.forced),
        });
    }
    for external in &edits.external {
        let language = external
            .language
            .as_deref()
            .map(normalize_language_code)
            .transpose()?;
        explicit_default.push(external.default);
        tracks.push(PlannedTrack {
            kind: kind_of(external),
            origin: Origin::External(external),
            codec_id: external_codec_id(external).unwrap_or_default().to_owned(),
            language,
            title: external.title.clone(),
            default: external.default,
            forced: external.forced,
        });
    }

    let mut warnings = Vec::new();
    let mut dropped_source_audio = Vec::new();
    let mut kept = Vec::with_capacity(tracks.len());
    for (track, explicit) in tracks.into_iter().zip(explicit_default) {
        if engine == Engine::Ffmpeg
            && let Some(reason) = skip_reason(&track, container)
        {
            if track.kind == TrackKind::Audio && matches!(track.origin, Origin::Source(_)) {
                dropped_source_audio.push(reason.clone());
            }
            warnings.push(format!("Skipped {}: {reason}", describe(&track)));
            continue;
        }
        kept.push((track, explicit));
    }

    for kind in [TrackKind::Audio, TrackKind::Subtitle] {
        let winner = kept
            .iter()
            .rposition(|(track, explicit)| track.kind == kind && *explicit)
            .or_else(|| {
                kept.iter()
                    .position(|(track, _)| track.kind == kind && track.default)
            });
        for (index, (track, _)) in kept.iter_mut().enumerate() {
            if track.kind == kind {
                track.default = Some(index) == winner;
            }
        }
    }
    Ok(RemuxPlan {
        tracks: kept.into_iter().map(|(track, _)| track).collect(),
        warnings,
        dropped_source_audio,
    })
}

fn skip_reason(track: &PlannedTrack<'_>, container: Container) -> Option<String> {
    if track.kind == TrackKind::Audio {
        let codec = if track.codec_id.is_empty() {
            "unknown"
        } else {
            track.codec_id.as_str()
        };
        return compat::remux_audio_drop_reason(codec, container);
    }
    if track.kind == TrackKind::Subtitle {
        if track.codec_id.is_empty() {
            return Some("unsupported subtitle file type".to_owned());
        }
        if let StreamPlan::Drop(reason) = compat::subtitle_plan(&track.codec_id, container) {
            return Some(reason);
        }
    }
    None
}

/// Warnings about added files for the container, for display before a job runs.
pub fn external_warnings(
    metadata: &MediaMetadata,
    audio_languages: &[String],
    subtitle_languages: &[String],
    include_subtitles: bool,
    edits: &TrackEdits,
    container: Container,
) -> Vec<String> {
    plan(
        metadata,
        audio_languages,
        subtitle_languages,
        include_subtitles,
        edits,
        Engine::Ffmpeg,
        container,
    )
    .map(|plan| {
        plan.warnings
            .into_iter()
            .filter(|warning| warning.starts_with("Skipped external"))
            .collect()
    })
    .unwrap_or_default()
}

fn flag(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

/// MKVToolNix arguments for an edited remux. The caller checked that `edits` is not empty.
pub fn build_mkvmerge_arguments(
    input: &Path,
    output: &Path,
    metadata: &MediaMetadata,
    audio_languages: &[String],
    subtitle_languages: &[String],
    settings: MkvRemuxSettings<'_>,
    edits: &TrackEdits,
) -> Result<Vec<OsString>, String> {
    let plan = plan(
        metadata,
        audio_languages,
        subtitle_languages,
        settings.include_subtitles,
        edits,
        Engine::Mkvmerge,
        Container::Mkv,
    )?;
    let mut sources = Vec::new();
    let mut externals = Vec::new();
    for track in &plan.tracks {
        match track.origin {
            Origin::Source(source) => {
                let id = source
                    .mkvmerge_id
                    .ok_or_else(|| "MKVToolNix track IDs are unavailable; use FFmpeg".to_owned())?;
                sources.push((track, id));
            }
            Origin::External(external) => externals.push((track, external)),
        }
    }
    let ids_of = |kind: TrackKind| {
        sources
            .iter()
            .filter(|(track, _)| track.kind == kind)
            .map(|(_, id)| *id)
            .collect::<Vec<_>>()
    };
    let audio_ids = ids_of(TrackKind::Audio);
    let subtitle_ids = ids_of(TrackKind::Subtitle);

    let mut arguments: Vec<OsString> = vec!["--gui-mode".into(), "-o".into(), output.into()];
    if audio_ids.is_empty() {
        arguments.push("--no-audio".into());
    } else {
        arguments.extend(["--audio-tracks".into(), join_ids(&audio_ids).into()]);
    }
    if subtitle_ids.is_empty() {
        arguments.push("--no-subtitles".into());
    } else {
        arguments.extend(["--subtitle-tracks".into(), join_ids(&subtitle_ids).into()]);
    }
    if !settings.keep_chapters {
        arguments.push("--no-chapters".into());
    }
    if !settings.keep_metadata {
        arguments.push("--no-global-tags".into());
    }
    if let Some(title) = settings.title.filter(|title| !title.is_empty()) {
        arguments.extend(["--title".into(), title.into()]);
    }
    for (track, id) in &sources {
        arguments.extend([
            "--default-track-flag".into(),
            format!("{id}:{}", flag(track.default)).into(),
            "--forced-display-flag".into(),
            format!("{id}:{}", flag(track.forced)).into(),
        ]);
        if let Some(language) = &track.language {
            arguments.extend(["--language".into(), format!("{id}:{language}").into()]);
        }
        if let Some(title) = &track.title {
            arguments.extend(["--track-name".into(), format!("{id}:{title}").into()]);
        }
    }
    if !edits.order.is_empty() {
        arguments.extend([
            "--track-order".into(),
            track_order(metadata, &sources, externals.len()).into(),
        ]);
    }
    arguments.push(input.into());

    for (track, external) in &externals {
        if let Some(language) = &track.language {
            arguments.extend(["--language".into(), format!("0:{language}").into()]);
        }
        if let Some(title) = &track.title {
            arguments.extend(["--track-name".into(), format!("0:{title}").into()]);
        }
        arguments.extend([
            "--default-track-flag".into(),
            format!("0:{}", flag(track.default)).into(),
            "--forced-display-flag".into(),
            format!("0:{}", flag(track.forced)).into(),
        ]);
        if external.offset_ms != 0 {
            arguments.extend(["--sync".into(), format!("0:{}", external.offset_ms).into()]);
        }
        if track.kind == TrackKind::Subtitle
            && let Some(charset) = external
                .charset
                .as_deref()
                .filter(|value| !value.is_empty())
        {
            arguments.extend(["--sub-charset".into(), format!("0:{charset}").into()]);
        }
        arguments.push((&external.path).into());
    }
    Ok(arguments)
}

/// `--track-order` list: video and other unlisted source tracks first, then the planned source
/// tracks, then the external files (file ids 1..).
fn track_order(
    metadata: &MediaMetadata,
    sources: &[(&PlannedTrack<'_>, u64)],
    external_count: usize,
) -> String {
    let listed_ids = metadata
        .audio_tracks
        .iter()
        .chain(&metadata.subtitle_tracks)
        .filter_map(|track| track.mkvmerge_id)
        .collect::<Vec<_>>();
    let highest = listed_ids.iter().copied().max().unwrap_or(0);
    let mut entries = (0..=highest)
        .filter(|id| !listed_ids.contains(id))
        .map(|id| format!("0:{id}"))
        .collect::<Vec<_>>();
    entries.extend(sources.iter().map(|(_, id)| format!("0:{id}")));
    entries.extend((1..=external_count).map(|file| format!("{file}:0")));
    entries.join(",")
}

fn join_ids(ids: &[u64]) -> String {
    ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",")
}

fn seconds(offset_ms: i64) -> String {
    let text = format!("{:.3}", offset_ms as f64 / 1000.0);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Target codec that makes FFmpeg decode and re-encode a text subtitle so `-sub_charenc` applies.
pub(crate) fn recode_target(codec_id: &str) -> Option<&'static str> {
    match codec_id {
        "subrip" => Some("subrip"),
        "ass" | "ssa" => Some("ass"),
        "webvtt" => Some("webvtt"),
        _ => None,
    }
}

pub(crate) fn is_utf8(charset: &str) -> bool {
    matches!(
        charset
            .trim()
            .to_ascii_lowercase()
            .replace('_', "-")
            .as_str(),
        "utf-8" | "utf8" | ""
    )
}

fn disposition(default: bool, forced: bool) -> &'static str {
    match (default, forced) {
        (true, true) => "default+forced",
        (true, false) => "default",
        (false, true) => "forced",
        (false, false) => "0",
    }
}

/// FFmpeg stream-copy arguments for an edited remux, plus warnings for skipped tracks.
///
/// Nothing is re-encoded except text subtitles, which convert to the container's text format
/// (and recode when an external file has a non-UTF-8 charset). An audio track the container
/// cannot hold (for example DTS in MP4) or a picture subtitle is skipped with a warning; when
/// that removes every source audio track and none remains, the job fails.
///
/// Honoured from `settings`: container, language selection, `track_edits`, `keep_metadata`,
/// `keep_chapters`, `keep_attachments`, `title`, `tags` and `web_optimized`. Not applied here:
/// chapter names, `align_av_start`, the track-selection behaviours (`selection`,
/// `forced_only`, `default_track`) and per-track audio settings; those need the transcode
/// argument builder (`JobSettings::needs_ffmpeg_muxing`).
pub fn build_ffmpeg_remux_arguments(
    input: &Path,
    output: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
) -> Result<(Vec<OsString>, Vec<String>), String> {
    let container = settings.container;
    if let Some(codec) = metadata
        .video
        .as_ref()
        .map(|video| video.codec_id.as_str())
        .or(metadata.video_codec.as_deref())
        && let StreamPlan::Drop(_) = compat::video_copy_plan(codec, container)
    {
        return Err(format!(
            "Video must be re-encoded for {}",
            container.label()
        ));
    }
    let plan = plan(
        metadata,
        &settings.audio.languages,
        &settings.subtitles.languages,
        settings.subtitles.include,
        &settings.track_edits,
        Engine::Ffmpeg,
        container,
    )?;
    if !plan.dropped_source_audio.is_empty()
        && !plan
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Audio)
    {
        return Err(compat::remux_audio_failure(
            &plan.dropped_source_audio,
            container,
        ));
    }

    let mut arguments: Vec<OsString> = vec!["-i".into(), input.into()];
    let mut maps: Vec<OsString> = vec!["-map".into(), "0:v?".into()];
    let mut per_stream: Vec<OsString> = Vec::new();
    let (mut audio_index, mut subtitle_index, mut next_input) = (0_usize, 0_usize, 1_usize);
    for track in &plan.tracks {
        let (selector, kind_letter, index) = match track.kind {
            TrackKind::Audio => ("a", "a", &mut audio_index),
            TrackKind::Subtitle => ("s", "s", &mut subtitle_index),
        };
        // Audio never converts here: tracks the container cannot hold were skipped by the plan.
        let stream_plan = match track.kind {
            TrackKind::Audio => StreamPlan::Copy,
            TrackKind::Subtitle => compat::subtitle_plan(&track.codec_id, container),
        };
        let mut codec = match stream_plan {
            StreamPlan::Convert(codec) => Some(codec),
            StreamPlan::Copy | StreamPlan::Drop(_) => None,
        };
        match track.origin {
            Origin::Source(source) => {
                let stream = source
                    .ffprobe_index
                    .ok_or_else(|| "FFprobe stream indices are unavailable".to_owned())?;
                maps.extend(["-map".into(), format!("0:{stream}").into()]);
            }
            Origin::External(external) => {
                if external.offset_ms != 0 {
                    arguments.extend(["-itsoffset".into(), seconds(external.offset_ms).into()]);
                }
                if track.kind == TrackKind::Subtitle
                    && let Some(charset) = external
                        .charset
                        .as_deref()
                        .filter(|charset| !is_utf8(charset))
                {
                    arguments.extend(["-sub_charenc".into(), charset.into()]);
                    if codec.is_none() {
                        codec = recode_target(&track.codec_id);
                    }
                }
                arguments.extend(["-i".into(), (&external.path).into()]);
                maps.extend(["-map".into(), format!("{next_input}:0").into()]);
                next_input += 1;
            }
        }
        if let Some(codec) = codec {
            per_stream.extend([format!("-c:{kind_letter}:{index}").into(), codec.into()]);
        }
        per_stream.extend([
            format!("-disposition:{selector}:{index}").into(),
            disposition(track.default, track.forced).into(),
        ]);
        if let Some(language) = &track.language {
            per_stream.extend([
                format!("-metadata:s:{selector}:{index}").into(),
                format!("language={language}").into(),
            ]);
        }
        if let Some(title) = &track.title {
            // MP4/MOV have no stream title tag; the muxer stores the handler name instead.
            let key = if matches!(container, Container::Mp4 | Container::Mov) {
                "handler_name"
            } else {
                "title"
            };
            per_stream.extend([
                format!("-metadata:s:{selector}:{index}").into(),
                format!("{key}={title}").into(),
            ]);
        }
        *index += 1;
    }

    arguments.extend(maps);
    if container == Container::Mkv && settings.keep_attachments {
        arguments.extend(["-map".into(), "0:t?".into()]);
    }
    arguments.extend(["-c".into(), "copy".into()]);
    arguments.extend(per_stream);
    arguments.extend([
        "-map_metadata".into(),
        if settings.keep_metadata { "0" } else { "-1" }.into(),
        "-map_chapters".into(),
        if settings.keep_chapters { "0" } else { "-1" }.into(),
    ]);
    if let Some(title) = settings.title.as_deref().filter(|title| !title.is_empty()) {
        arguments.extend(["-metadata".into(), format!("title={title}").into()]);
    }
    for (key, value) in settings.tags.entries() {
        arguments.extend(["-metadata".into(), format!("{key}={value}").into()]);
    }
    arguments.extend([
        "-max_muxing_queue_size".into(),
        "4096".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        "-y".into(),
        "-f".into(),
        container.ffmpeg_muxer().into(),
    ]);
    if settings.web_optimized && matches!(container, Container::Mp4 | Container::Mov) {
        arguments.extend(["-movflags".into(), "+faststart".into()]);
    }
    arguments.push(output.into());
    Ok((arguments, plan.warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mkv::VideoInfo;
    use std::path::PathBuf;

    fn track(id: u64, language: &str, codec_id: &str, default: bool) -> TrackInfo {
        TrackInfo {
            id,
            mkvmerge_id: Some(id),
            ffprobe_index: Some(id),
            language: language.to_owned(),
            codec_id: codec_id.to_owned(),
            default,
            ..TrackInfo::default()
        }
    }

    /// Video id 0, audio ids 1 (jpn, default) and 2 (eng), subtitle ids 3 (eng) and 4 (fra).
    fn metadata() -> MediaMetadata {
        MediaMetadata {
            audio_tracks: vec![track(1, "jpn", "aac", true), track(2, "eng", "ac3", false)],
            subtitle_tracks: vec![
                track(3, "eng", "subrip", true),
                track(4, "fra", "subrip", false),
            ],
            video: Some(VideoInfo {
                codec_id: "vp9".to_owned(),
                ..VideoInfo::default()
            }),
            video_codec: Some("vp9".to_owned()),
            ..MediaMetadata::default()
        }
    }

    fn external(path: &str, kind: ExternalKind) -> ExternalTrack {
        ExternalTrack {
            path: PathBuf::from(path),
            kind,
            ..ExternalTrack::default()
        }
    }

    fn text(arguments: &[OsString]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    fn mkvmerge(edits: &TrackEdits) -> Vec<String> {
        let settings = MkvRemuxSettings {
            include_subtitles: true,
            keep_chapters: true,
            keep_metadata: true,
            title: None,
            track_edits: Some(edits),
        };
        text(
            &build_mkvmerge_arguments(
                Path::new("in.mkv"),
                Path::new("out.mkv"),
                &metadata(),
                &[],
                &[],
                settings,
                edits,
            )
            .unwrap(),
        )
    }

    fn ffmpeg(edits: TrackEdits, container: Container) -> (Vec<String>, Vec<String>) {
        let settings = JobSettings {
            container,
            track_edits: edits,
            ..JobSettings::default()
        };
        let (arguments, warnings) = build_ffmpeg_remux_arguments(
            Path::new("in.mkv"),
            Path::new("out"),
            &metadata(),
            &settings,
        )
        .unwrap();
        (text(&arguments), warnings)
    }

    fn has_pair(arguments: &[String], first: &str, second: &str) -> bool {
        arguments
            .windows(2)
            .any(|pair| pair[0] == first && pair[1] == second)
    }

    #[test]
    fn serde_defaults_make_empty_edits_a_no_op() {
        let edits: TrackEdits = serde_json::from_str("{}").unwrap();
        assert!(edits.is_empty());
        let edit: TrackEdit = serde_json::from_str(r#"{"id":2}"#).unwrap();
        assert!(edit.keep);
        let settings: JobSettings = serde_json::from_str("{}").unwrap();
        assert!(settings.track_edits.is_empty());
        let edited = JobSettings {
            track_edits: TrackEdits {
                external: vec![external("a.srt", ExternalKind::Subtitle)],
                ..TrackEdits::default()
            },
            ..JobSettings::default()
        };
        assert!(edited.is_remux_only());
    }

    #[test]
    fn language_codes_are_validated_and_normalised() {
        assert_eq!(normalize_language_code(" ENG ").unwrap(), "eng");
        assert_eq!(normalize_language_code("und").unwrap(), "und");
        assert!(normalize_language_code("en").is_err());
        assert!(normalize_language_code("english").is_err());
        assert!(normalize_language_code("e1g").is_err());
        assert!(normalize_language_code("eng,fra").is_err());
    }

    #[test]
    fn mkvmerge_drops_retags_and_flips_flags() {
        let edits = TrackEdits {
            tracks: vec![
                TrackEdit {
                    id: 2,
                    keep: false,
                    ..TrackEdit::default()
                },
                TrackEdit {
                    id: 1,
                    language: Some("ENG".to_owned()),
                    title: Some("Commentary".to_owned()),
                    default: Some(false),
                    ..TrackEdit::default()
                },
                TrackEdit {
                    id: 4,
                    default: Some(true),
                    forced: Some(true),
                    ..TrackEdit::default()
                },
            ],
            ..TrackEdits::default()
        };
        let arguments = mkvmerge(&edits);
        assert!(has_pair(&arguments, "--audio-tracks", "1"));
        assert!(has_pair(&arguments, "--subtitle-tracks", "3,4"));
        assert!(has_pair(&arguments, "--language", "1:eng"));
        assert!(has_pair(&arguments, "--track-name", "1:Commentary"));
        assert!(has_pair(&arguments, "--default-track-flag", "1:0"));
        assert!(has_pair(&arguments, "--default-track-flag", "3:0"));
        assert!(has_pair(&arguments, "--default-track-flag", "4:1"));
        assert!(has_pair(&arguments, "--forced-display-flag", "4:1"));
        assert!(has_pair(&arguments, "--forced-display-flag", "3:0"));
        assert_eq!(arguments.last().map(String::as_str), Some("in.mkv"));
        assert!(!arguments.contains(&"--track-order".to_owned()));
    }

    #[test]
    fn mkvmerge_orders_tracks_with_video_first_and_externals_last() {
        let edits = TrackEdits {
            order: vec![4, 2],
            external: vec![external("a.srt", ExternalKind::Subtitle)],
            ..TrackEdits::default()
        };
        let arguments = mkvmerge(&edits);
        assert!(has_pair(
            &arguments,
            "--track-order",
            "0:0,0:4,0:2,0:1,0:3,1:0"
        ));
    }

    #[test]
    fn mkvmerge_adds_external_srt_with_offset_and_charset() {
        let edits = TrackEdits {
            external: vec![ExternalTrack {
                language: Some("spa".to_owned()),
                title: Some("Spanish".to_owned()),
                default: true,
                forced: false,
                offset_ms: 500,
                charset: Some("CP1252".to_owned()),
                ..external("sub/es.srt", ExternalKind::Subtitle)
            }],
            ..TrackEdits::default()
        };
        let arguments = mkvmerge(&edits);
        let start = arguments
            .iter()
            .position(|value| value == "in.mkv")
            .unwrap();
        assert_eq!(
            arguments[start + 1..],
            [
                "--language",
                "0:spa",
                "--track-name",
                "0:Spanish",
                "--default-track-flag",
                "0:1",
                "--forced-display-flag",
                "0:0",
                "--sync",
                "0:500",
                "--sub-charset",
                "0:CP1252",
                "sub/es.srt"
            ]
        );
        // The explicit default on the new subtitle clears the source default (id 3).
        assert!(has_pair(&arguments, "--default-track-flag", "3:0"));
    }

    #[test]
    fn mkvmerge_adds_external_ac3_without_charset() {
        let edits = TrackEdits {
            external: vec![ExternalTrack {
                language: Some("eng".to_owned()),
                offset_ms: -250,
                charset: Some("CP1252".to_owned()),
                ..external("dub.ac3", ExternalKind::Audio)
            }],
            ..TrackEdits::default()
        };
        let arguments = mkvmerge(&edits);
        let start = arguments
            .iter()
            .position(|value| value == "in.mkv")
            .unwrap();
        let tail = &arguments[start + 1..];
        assert!(has_pair(tail, "--sync", "0:-250"));
        assert!(!tail.contains(&"--sub-charset".to_owned()));
        assert_eq!(tail.last().map(String::as_str), Some("dub.ac3"));
        // Not explicit default: the source default audio stays the only one.
        assert!(has_pair(&arguments, "--default-track-flag", "1:1"));
        assert!(has_pair(tail, "--default-track-flag", "0:0"));
    }

    #[test]
    fn later_explicit_default_wins_and_others_are_cleared() {
        let edits = TrackEdits {
            tracks: vec![
                TrackEdit {
                    id: 1,
                    default: Some(true),
                    ..TrackEdit::default()
                },
                TrackEdit {
                    id: 2,
                    default: Some(true),
                    ..TrackEdit::default()
                },
            ],
            ..TrackEdits::default()
        };
        let arguments = mkvmerge(&edits);
        assert!(has_pair(&arguments, "--default-track-flag", "1:0"));
        assert!(has_pair(&arguments, "--default-track-flag", "2:1"));
        // Subtitle default is untouched.
        assert!(has_pair(&arguments, "--default-track-flag", "3:1"));
    }

    #[test]
    fn invalid_language_fails_before_any_tool_runs() {
        let edits = TrackEdits {
            tracks: vec![TrackEdit {
                id: 1,
                language: Some("english".to_owned()),
                ..TrackEdit::default()
            }],
            ..TrackEdits::default()
        };
        let settings = MkvRemuxSettings {
            include_subtitles: true,
            keep_chapters: true,
            keep_metadata: true,
            title: None,
            track_edits: Some(&edits),
        };
        let error = build_mkvmerge_arguments(
            Path::new("in.mkv"),
            Path::new("out.mkv"),
            &metadata(),
            &[],
            &[],
            settings,
            &edits,
        )
        .unwrap_err();
        assert!(error.contains("Invalid language code"));
    }

    #[test]
    fn ffmpeg_drops_retags_flips_flags_and_reorders() {
        let edits = TrackEdits {
            tracks: vec![
                TrackEdit {
                    id: 2,
                    keep: false,
                    ..TrackEdit::default()
                },
                TrackEdit {
                    id: 1,
                    language: Some("eng".to_owned()),
                    title: Some("Main".to_owned()),
                    default: Some(false),
                    ..TrackEdit::default()
                },
                TrackEdit {
                    id: 4,
                    default: Some(true),
                    forced: Some(true),
                    ..TrackEdit::default()
                },
            ],
            order: vec![4],
            ..TrackEdits::default()
        };
        let (arguments, warnings) = ffmpeg(edits, Container::Mkv);
        assert!(warnings.is_empty());
        let maps = arguments
            .windows(2)
            .filter(|pair| pair[0] == "-map")
            .map(|pair| pair[1].as_str())
            .collect::<Vec<_>>();
        assert_eq!(maps, ["0:v?", "0:4", "0:1", "0:3", "0:t?"]);
        // Output subtitle 0 is source 4, output audio 0 is source 1.
        assert!(has_pair(&arguments, "-disposition:s:0", "default+forced"));
        assert!(has_pair(&arguments, "-disposition:s:1", "0"));
        assert!(has_pair(&arguments, "-disposition:a:0", "0"));
        assert!(has_pair(&arguments, "-metadata:s:a:0", "language=eng"));
        assert!(has_pair(&arguments, "-metadata:s:a:0", "title=Main"));
        assert!(has_pair(&arguments, "-c", "copy"));
    }

    #[test]
    fn ffmpeg_adds_external_srt_with_offset_and_charset() {
        let edits = TrackEdits {
            external: vec![ExternalTrack {
                language: Some("spa".to_owned()),
                title: Some("Spanish".to_owned()),
                offset_ms: 500,
                charset: Some("CP1252".to_owned()),
                ..external("es.srt", ExternalKind::Subtitle)
            }],
            ..TrackEdits::default()
        };
        let (arguments, warnings) = ffmpeg(edits, Container::Mkv);
        assert!(warnings.is_empty());
        let start = arguments
            .iter()
            .position(|value| value == "-itsoffset")
            .unwrap();
        assert_eq!(
            arguments[start..start + 5],
            ["-itsoffset", "0.5", "-sub_charenc", "CP1252", "-i"]
        );
        assert_eq!(arguments[start + 5], "es.srt");
        assert!(has_pair(&arguments, "-map", "1:0"));
        // A non-UTF-8 charset forces a re-encode so that the decoder recodes the text.
        assert!(has_pair(&arguments, "-c:s:2", "subrip"));
        assert!(has_pair(&arguments, "-metadata:s:s:2", "language=spa"));
        assert!(has_pair(&arguments, "-metadata:s:s:2", "title=Spanish"));
        assert!(has_pair(&arguments, "-disposition:s:2", "0"));
    }

    #[test]
    fn ffmpeg_adds_external_ac3() {
        let edits = TrackEdits {
            external: vec![ExternalTrack {
                language: Some("eng".to_owned()),
                default: true,
                ..external("dub.ac3", ExternalKind::Audio)
            }],
            ..TrackEdits::default()
        };
        let (arguments, warnings) = ffmpeg(edits.clone(), Container::Mkv);
        assert!(warnings.is_empty());
        assert!(!arguments.contains(&"-itsoffset".to_owned()));
        assert!(has_pair(&arguments, "-i", "dub.ac3"));
        assert!(has_pair(&arguments, "-disposition:a:2", "default"));
        assert!(has_pair(&arguments, "-disposition:a:0", "0"));
        // MP4 holds AC-3 as is.
        let (mp4, _) = ffmpeg(edits, Container::Mp4);
        assert!(!mp4.iter().any(|value| value.starts_with("-c:a:")));
    }

    #[test]
    fn ffmpeg_converts_text_subtitles_and_skips_picture_subtitles_per_container() {
        let edits = TrackEdits {
            external: vec![
                external("a.srt", ExternalKind::Subtitle),
                external("b.sup", ExternalKind::Subtitle),
                external("c.xyz", ExternalKind::Subtitle),
            ],
            ..TrackEdits::default()
        };
        let (mp4, warnings) = ffmpeg(edits.clone(), Container::Mp4);
        assert!(has_pair(&mp4, "-c:s:2", "mov_text"));
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("b.sup") && warnings[0].contains("Picture-based"));
        assert!(warnings[1].contains("c.xyz"));
        assert!(!mp4.contains(&"b.sup".to_owned()));
        let webm_settings = JobSettings {
            container: Container::WebM,
            track_edits: edits.clone(),
            ..JobSettings::default()
        };
        let mut opus = metadata();
        opus.audio_tracks = vec![track(1, "jpn", "opus", true)];
        let (webm, _) = build_ffmpeg_remux_arguments(
            Path::new("in.mkv"),
            Path::new("out"),
            &opus,
            &webm_settings,
        )
        .unwrap();
        let webm = text(&webm);
        assert!(has_pair(&webm, "-c:s:2", "webvtt"));
        let (mkv, warnings) = ffmpeg(edits, Container::Mkv);
        assert!(mkv.contains(&"b.sup".to_owned()));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("c.xyz"));
    }

    #[test]
    fn unstorable_audio_is_dropped_with_a_warning_never_converted() {
        let mut dts = metadata();
        dts.audio_tracks = vec![track(1, "eng", "dts", true), track(2, "jpn", "aac", false)];
        let settings = JobSettings {
            container: Container::Mp4,
            track_edits: TrackEdits {
                tracks: vec![TrackEdit {
                    id: 2,
                    title: Some("Main".to_owned()),
                    ..TrackEdit::default()
                }],
                ..TrackEdits::default()
            },
            ..JobSettings::default()
        };
        let (arguments, warnings) = build_ffmpeg_remux_arguments(
            Path::new("in.mkv"),
            Path::new("out.mp4"),
            &dts,
            &settings,
        )
        .unwrap();
        let arguments = text(&arguments);
        assert!(!arguments.contains(&"0:1".to_owned()), "{arguments:?}");
        assert!(arguments.contains(&"0:2".to_owned()), "{arguments:?}");
        assert!(!arguments.iter().any(|value| value.starts_with("-c:a:")));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("DTS audio cannot be stored in MP4"))
        );
        // MP4 stores no stream title: the handler name carries it.
        assert!(has_pair(&arguments, "-metadata:s:a:0", "handler_name=Main"));
        // The compat warnings the UI shows list the same drop.
        let shown = crate::compat::job_warnings(&dts, &settings);
        assert!(
            shown
                .iter()
                .any(|warning| warning.contains("Audio track 1 will be dropped")),
            "{shown:?}"
        );

        // Only audio is DTS: the job fails before FFmpeg starts.
        dts.audio_tracks.truncate(1);
        let error = build_ffmpeg_remux_arguments(
            Path::new("in.mkv"),
            Path::new("out.mp4"),
            &dts,
            &settings,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "DTS audio cannot be stored in MP4; choose MKV or use Transcode"
        );
    }

    #[test]
    fn faststart_follows_web_optimized_and_mkv_keeps_title_tag() {
        let mut settings = JobSettings {
            container: Container::Mp4,
            track_edits: TrackEdits {
                tracks: vec![TrackEdit {
                    id: 1,
                    title: Some("Main".to_owned()),
                    ..TrackEdit::default()
                }],
                ..TrackEdits::default()
            },
            ..JobSettings::default()
        };
        let build = |settings: &JobSettings| {
            text(
                &build_ffmpeg_remux_arguments(
                    Path::new("in.mkv"),
                    Path::new("out"),
                    &metadata(),
                    settings,
                )
                .unwrap()
                .0,
            )
        };
        assert!(has_pair(&build(&settings), "-movflags", "+faststart"));
        settings.web_optimized = false;
        assert!(!build(&settings).contains(&"-movflags".to_owned()));
        settings.container = Container::Mkv;
        assert!(has_pair(&build(&settings), "-metadata:s:a:0", "title=Main"));
    }

    #[test]
    fn external_warnings_report_only_added_files() {
        let edits = TrackEdits {
            external: vec![external("b.sup", ExternalKind::Subtitle)],
            ..TrackEdits::default()
        };
        let warnings = external_warnings(&metadata(), &[], &[], true, &edits, Container::Mp4);
        assert_eq!(warnings.len(), 1);
        assert!(external_warnings(&metadata(), &[], &[], true, &edits, Container::Mkv).is_empty());
    }

    #[test]
    fn missing_external_file_is_reported() {
        let edits = TrackEdits {
            external: vec![external("definitely-missing.srt", ExternalKind::Subtitle)],
            ..TrackEdits::default()
        };
        assert!(
            edits
                .check_external_files()
                .unwrap_err()
                .contains("definitely-missing.srt")
        );
    }
}
