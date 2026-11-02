use crate::formats;
use crate::tools;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TrackInfo {
    pub id: u64,
    pub mkvmerge_id: Option<u64>,
    pub ffprobe_index: Option<u64>,
    pub language: String,
    pub codec: String,
    pub codec_id: String,
    pub channels: Option<u32>,
    pub forced: bool,
    pub default: bool,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct MediaMetadata {
    pub audio_tracks: Vec<TrackInfo>,
    pub subtitle_tracks: Vec<TrackInfo>,
    pub video_codec: Option<String>,
    pub video: Option<VideoInfo>,
    pub container: Option<String>,
    pub has_chapters: bool,
    pub duration: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct VideoInfo {
    pub codec_id: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub bit_depth: Option<u8>,
    pub hdr: bool,
    pub interlaced: Option<bool>,
    pub ffprobe_index: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct FileInfo {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub metadata: Option<MediaMetadata>,
}

#[derive(Clone, Debug, Default)]
pub struct BatchStats {
    pub total_files: usize,
    pub total_size: u64,
}

#[derive(Clone, Debug, Default)]
pub struct AvailableLanguages {
    pub audio: Vec<String>,
    pub subtitles: Vec<String>,
}

pub fn normalize_languages(languages: &[String]) -> Vec<String> {
    languages
        .iter()
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn parse_language_field(value: &str) -> Vec<String> {
    normalize_languages(&[value.to_owned()])
}

pub fn is_mkv_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mkv"))
}

pub fn scan_directory(directory: &Path) -> Result<Vec<FileInfo>, String> {
    if !directory.is_dir() {
        return Err(format!("Not a folder: {}", directory.display()));
    }

    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if formats::is_supported_input(&path) {
            let metadata = entry.metadata().map_err(|error| error.to_string())?;
            if metadata.is_file() {
                files.push(FileInfo {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path,
                    size: metadata.len(),
                    metadata: None,
                });
            }
        }
    }
    files.sort_by_key(|file| file.name.to_lowercase());
    Ok(files)
}

pub fn batch_stats(files: &[FileInfo]) -> BatchStats {
    let total_size = files.iter().map(|file| file.size).sum();
    BatchStats {
        total_files: files.len(),
        total_size,
    }
}

pub fn default_output_dir(input: &Path) -> PathBuf {
    input.join("converted")
}

pub fn output_path(output_directory: &Path, input: &Path) -> PathBuf {
    output_directory.join(input.file_name().unwrap_or_default())
}

pub fn partial_output_path(output: &Path, process_id: u32, attempt: u32) -> PathBuf {
    let mut name = output
        .file_stem()
        .or_else(|| output.file_name())
        .unwrap_or_else(|| OsStr::new("output"))
        .to_os_string();
    let extension = output
        .extension()
        .unwrap_or_else(|| OsStr::new("mkv"))
        .to_string_lossy();
    name.push(format!(
        ".langmux-partial-{process_id}-{attempt}.{extension}"
    ));
    output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(name)
}

pub fn paths_resolve_to_same_file(input: &Path, output: &Path) -> Result<bool, String> {
    let canonical_input = std::fs::canonicalize(input)
        .map_err(|error| format!("Could not resolve input path: {error}"))?;
    let output_name = output
        .file_name()
        .ok_or_else(|| "Output path has no file name".to_owned())?;
    let output_parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_output_parent = std::fs::canonicalize(output_parent)
        .map_err(|error| format!("Could not resolve output folder: {error}"))?;
    let canonicalized_output = canonical_output_parent.join(output_name);
    if paths_equal(&canonical_input, &canonicalized_output) {
        return Ok(true);
    }
    Ok(std::fs::canonicalize(output)
        .is_ok_and(|resolved_output| paths_equal(&canonical_input, &resolved_output)))
}

pub fn directories_resolve_to_same_path(input: &Path, output: &Path) -> std::io::Result<bool> {
    let canonical_input = std::fs::canonicalize(input)?;
    let canonical_output = std::fs::canonicalize(output)?;
    Ok(paths_equal(&canonical_input, &canonical_output))
}

#[cfg(windows)]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(windows))]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemuxExitOutcome {
    Success,
    SuccessWithWarnings,
    Failure,
}

pub fn classify_remux_exit(
    is_mkvtoolnix: bool,
    exit_code: Option<i32>,
    has_nonempty_output: bool,
) -> RemuxExitOutcome {
    match (is_mkvtoolnix, exit_code, has_nonempty_output) {
        (_, Some(0), true) => RemuxExitOutcome::Success,
        (true, Some(1), true) => RemuxExitOutcome::SuccessWithWarnings,
        _ => RemuxExitOutcome::Failure,
    }
}

pub fn read_metadata(
    mkvmerge: Option<&Path>,
    ffprobe: Option<&Path>,
    input: &Path,
) -> Result<MediaMetadata, String> {
    if !is_mkv_path(input) {
        let Some(executable) = ffprobe else {
            return Err("ffprobe is required to read non-MKV inputs".to_owned());
        };
        return read_ffprobe_metadata(executable, input);
    }

    let mkvmerge_result = mkvmerge
        .map(|executable| {
            let output = tools::command(executable)
                .arg("-J")
                .arg(input)
                .output()
                .map_err(|error| format!("Failed to run mkvmerge: {error}"))?;
            parse_identification_json(&output)
        })
        .unwrap_or_else(|| Err("mkvmerge is unavailable".to_owned()));

    match mkvmerge_result {
        Ok(mut metadata) => {
            if let Some(executable) = ffprobe
                && let Ok(probe_metadata) = read_ffprobe_metadata(executable, input)
            {
                attach_ffprobe_metadata(&mut metadata, &probe_metadata);
                if metadata.duration.is_none() {
                    metadata.duration = probe_metadata.duration;
                }
                if metadata.container.is_none() {
                    metadata.container = probe_metadata.container;
                }
                metadata.has_chapters |= probe_metadata.has_chapters;
                if metadata.video.is_none() {
                    metadata.video = probe_metadata.video;
                }
            }
            Ok(metadata)
        }
        Err(mkvmerge_error) => {
            if let Some(executable) = ffprobe {
                read_ffprobe_metadata(executable, input)
                    .map_err(|error| format!("{mkvmerge_error}; ffprobe fallback failed: {error}"))
            } else {
                Err(format!("{mkvmerge_error}; ffprobe is unavailable"))
            }
        }
    }
}

fn read_ffprobe_metadata(executable: &Path, input: &Path) -> Result<MediaMetadata, String> {
    let output = tools::command(executable)
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-show_chapters",
            "-of",
            "json",
        ])
        .arg(input)
        .output()
        .map_err(|error| format!("Failed to run ffprobe: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if error.is_empty() {
            format!("ffprobe exited with status {}", output.status)
        } else {
            error
        });
    }
    parse_ffprobe_json(&String::from_utf8_lossy(&output.stdout))
}

fn parse_identification_json(output: &Output) -> Result<MediaMetadata, String> {
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if error.is_empty() {
            format!("mkvmerge exited with status {}", output.status)
        } else {
            error
        });
    }
    parse_metadata_json(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_metadata_json(json: &str) -> Result<MediaMetadata, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("Invalid mkvmerge JSON: {error}"))?;
    let mut metadata = MediaMetadata::default();

    if let Some(tracks) = value["tracks"].as_array() {
        for track in tracks {
            let kind = track["type"].as_str().unwrap_or_default();
            let properties = &track["properties"];
            let language = properties["language"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("und")
                .trim()
                .to_ascii_lowercase();
            let info = TrackInfo {
                id: track["id"].as_u64().unwrap_or_default(),
                mkvmerge_id: track["id"].as_u64(),
                ffprobe_index: None,
                language,
                codec: track["codec"].as_str().unwrap_or("unknown").to_owned(),
                codec_id: normalize_codec_id(
                    properties["codec_id"]
                        .as_str()
                        .or_else(|| track["codec"].as_str())
                        .unwrap_or("unknown"),
                ),
                channels: properties["audio_channels"]
                    .as_u64()
                    .and_then(|channels| u32::try_from(channels).ok()),
                forced: properties["forced_track"].as_bool().unwrap_or(false),
                default: properties["default_track"].as_bool().unwrap_or(false),
                title: properties["track_name"].as_str().map(str::to_owned),
            };
            match kind {
                "video" if metadata.video_codec.is_none() => {
                    metadata.video_codec = Some(info.codec.clone());
                    let dimensions = parse_dimensions(properties["pixel_dimensions"].as_str());
                    metadata.video = Some(VideoInfo {
                        codec_id: info.codec_id.clone(),
                        width: dimensions.map(|(width, _)| width),
                        height: dimensions.map(|(_, height)| height),
                        fps: None,
                        bit_depth: properties["bit_depth"]
                            .as_u64()
                            .and_then(|depth| u8::try_from(depth).ok()),
                        hdr: false,
                        interlaced: None,
                        ffprobe_index: None,
                    });
                }
                "audio" => metadata.audio_tracks.push(info),
                "subtitle" | "subtitles" => metadata.subtitle_tracks.push(info),
                _ => {}
            }
        }
    }

    metadata.duration = value["container"]["properties"]["duration"]
        .as_f64()
        .map(|nanoseconds| nanoseconds / 1_000_000_000.0);
    metadata.has_chapters = value["chapters"]
        .as_array()
        .is_some_and(|chapters| !chapters.is_empty());
    Ok(metadata)
}

pub fn parse_ffprobe_json(json: &str) -> Result<MediaMetadata, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("Invalid ffprobe JSON: {error}"))?;
    let mut metadata = MediaMetadata::default();

    if let Some(streams) = value["streams"].as_array() {
        for stream in streams {
            let kind = stream["codec_type"].as_str().unwrap_or_default();
            let language = stream["tags"]["language"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("und")
                .trim()
                .to_ascii_lowercase();
            let index = stream["index"].as_u64().unwrap_or_default();
            let codec = stream["codec_name"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned();
            let codec_id = normalize_codec_id(&codec);
            let info = TrackInfo {
                id: index,
                mkvmerge_id: None,
                ffprobe_index: Some(index),
                language,
                codec: codec.clone(),
                codec_id: codec_id.clone(),
                channels: stream["channels"]
                    .as_u64()
                    .and_then(|channels| u32::try_from(channels).ok()),
                forced: stream["disposition"]["forced"].as_u64().unwrap_or_default() != 0,
                default: stream["disposition"]["default"]
                    .as_u64()
                    .unwrap_or_default()
                    != 0,
                title: stream["tags"]["title"].as_str().map(str::to_owned),
            };
            match kind {
                "video" if metadata.video_codec.is_none() => {
                    metadata.video_codec = Some(codec);
                    metadata.video = Some(VideoInfo {
                        codec_id,
                        width: stream["width"]
                            .as_u64()
                            .and_then(|width| u32::try_from(width).ok()),
                        height: stream["height"]
                            .as_u64()
                            .and_then(|height| u32::try_from(height).ok()),
                        fps: parse_frame_rate(
                            stream["avg_frame_rate"]
                                .as_str()
                                .or_else(|| stream["r_frame_rate"].as_str()),
                        ),
                        bit_depth: stream["bits_per_raw_sample"]
                            .as_str()
                            .and_then(|depth| depth.parse::<u8>().ok())
                            .or_else(|| pix_fmt_bit_depth(stream["pix_fmt"].as_str())),
                        hdr: matches!(
                            stream["color_transfer"].as_str(),
                            Some("smpte2084" | "arib-std-b67")
                        ),
                        interlaced: match stream["field_order"].as_str() {
                            Some("progressive") => Some(false),
                            Some("unknown") | None => None,
                            Some(_) => Some(true),
                        },
                        ffprobe_index: Some(index),
                    });
                }
                "audio" => metadata.audio_tracks.push(info),
                "subtitle" => metadata.subtitle_tracks.push(info),
                _ => {}
            }
        }
    }

    metadata.duration = value["format"]["duration"]
        .as_str()
        .and_then(|duration| duration.parse::<f64>().ok())
        .or_else(|| value["format"]["duration"].as_f64());
    metadata.container = value["format"]["format_name"].as_str().map(str::to_owned);
    metadata.has_chapters = value["chapters"]
        .as_array()
        .is_some_and(|chapters| !chapters.is_empty());
    Ok(metadata)
}

fn attach_ffprobe_metadata(metadata: &mut MediaMetadata, ffprobe: &MediaMetadata) {
    attach_track_indices(&mut metadata.audio_tracks, &ffprobe.audio_tracks);
    attach_track_indices(&mut metadata.subtitle_tracks, &ffprobe.subtitle_tracks);
}

fn attach_track_indices(tracks: &mut [TrackInfo], probe_tracks: &[TrackInfo]) {
    let mut used = vec![false; probe_tracks.len()];
    for track in tracks {
        let matching_index = probe_tracks
            .iter()
            .enumerate()
            .find(|(index, candidate)| {
                !used[*index]
                    && candidate.language == track.language
                    && (candidate.codec_id.eq_ignore_ascii_case(&track.codec_id)
                        || candidate.codec.eq_ignore_ascii_case(&track.codec))
            })
            .or_else(|| {
                probe_tracks
                    .iter()
                    .enumerate()
                    .find(|(index, _)| !used[*index])
            })
            .map(|(index, _)| index);
        if let Some(index) = matching_index {
            used[index] = true;
            track.ffprobe_index = probe_tracks[index].ffprobe_index;
            track.codec_id.clone_from(&probe_tracks[index].codec_id);
            track.channels = probe_tracks[index].channels;
            track.forced = probe_tracks[index].forced;
            track.default = probe_tracks[index].default;
            if track.title.is_none() {
                track.title.clone_from(&probe_tracks[index].title);
            }
        }
    }
}

fn normalize_codec_id(codec_id: &str) -> String {
    let codec = codec_id.trim().to_ascii_lowercase();
    match codec.as_str() {
        "v_mpeg4/iso/avc" | "avc" | "avc1" => "h264".to_owned(),
        "v_mpegh/iso/hevc" | "hev1" | "hvc1" => "hevc".to_owned(),
        "v_vp8" => "vp8".to_owned(),
        "v_vp9" => "vp9".to_owned(),
        "v_av1" | "av01" => "av1".to_owned(),
        "a_aac" => "aac".to_owned(),
        "a_ac3" => "ac3".to_owned(),
        "a_eac3" => "eac3".to_owned(),
        "a_opus" => "opus".to_owned(),
        "a_vorbis" => "vorbis".to_owned(),
        "s_text/utf8" => "subrip".to_owned(),
        "s_text/ass" => "ass".to_owned(),
        "s_text/ssa" => "ssa".to_owned(),
        "s_text/webvtt" => "webvtt".to_owned(),
        "s_hdmv/pgs" => "hdmv_pgs_subtitle".to_owned(),
        "s_vobsub" => "dvd_subtitle".to_owned(),
        _ => codec,
    }
}

fn parse_dimensions(dimensions: Option<&str>) -> Option<(u32, u32)> {
    let (width, height) = dimensions?.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn parse_frame_rate(value: Option<&str>) -> Option<f64> {
    let (numerator, denominator) = value?.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    (denominator != 0.0).then_some(numerator / denominator)
}

fn pix_fmt_bit_depth(value: Option<&str>) -> Option<u8> {
    let value = value?;
    for depth in ["16", "14", "12", "10"] {
        if value.contains(depth) {
            return depth.parse().ok();
        }
    }
    Some(8)
}

pub fn available_languages(metadata: &[MediaMetadata]) -> AvailableLanguages {
    let mut audio = BTreeSet::new();
    let mut subtitles = BTreeSet::new();
    for file in metadata {
        audio.extend(
            file.audio_tracks
                .iter()
                .map(|track| track.language.to_ascii_lowercase()),
        );
        subtitles.extend(
            file.subtitle_tracks
                .iter()
                .map(|track| track.language.to_ascii_lowercase()),
        );
    }
    AvailableLanguages {
        audio: audio.into_iter().collect(),
        subtitles: subtitles.into_iter().collect(),
    }
}

pub fn selected_tracks(
    metadata: &MediaMetadata,
    audio_languages: &[String],
    subtitle_languages: &[String],
) -> (Vec<u64>, Vec<u64>) {
    let audio_languages = normalize_languages(audio_languages);
    let subtitle_languages = normalize_languages(subtitle_languages);
    let audio = metadata
        .audio_tracks
        .iter()
        .filter(|track| audio_languages.contains(&track.language.to_ascii_lowercase()))
        .map(|track| track.id)
        .collect();
    let subtitles = metadata
        .subtitle_tracks
        .iter()
        .filter(|track| subtitle_languages.contains(&track.language.to_ascii_lowercase()))
        .map(|track| track.id)
        .collect();
    (audio, subtitles)
}

pub fn build_arguments(
    input: &Path,
    output: &Path,
    metadata: &MediaMetadata,
    audio_languages: &[String],
    subtitle_languages: &[String],
    dry_run: bool,
) -> Result<Option<Vec<OsString>>, String> {
    let mut audio_tracks = matching_tracks(&metadata.audio_tracks, audio_languages);
    if audio_tracks.is_empty() && !metadata.audio_tracks.is_empty() {
        audio_tracks = metadata.audio_tracks.iter().collect();
    }
    let subtitle_tracks = matching_tracks(&metadata.subtitle_tracks, subtitle_languages);
    if audio_tracks.is_empty() {
        return Err("No matching audio tracks found".to_owned());
    }
    if dry_run {
        return Ok(None);
    }
    let audio_ids = audio_tracks
        .iter()
        .map(|track| {
            track
                .mkvmerge_id
                .ok_or_else(|| "MKVToolNix track IDs are unavailable; use FFmpeg".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let subtitle_ids = subtitle_tracks
        .iter()
        .map(|track| {
            track
                .mkvmerge_id
                .ok_or_else(|| "MKVToolNix track IDs are unavailable; use FFmpeg".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut arguments = vec![
        OsString::from("--gui-mode"),
        OsString::from("-o"),
        output.as_os_str().to_owned(),
        OsString::from("--audio-tracks"),
        OsString::from(join_ids(&audio_ids)),
    ];
    if subtitle_ids.is_empty() {
        arguments.push(OsString::from("--no-subtitles"));
    } else {
        arguments.push(OsString::from("--subtitle-tracks"));
        arguments.push(OsString::from(join_ids(&subtitle_ids)));
    }
    arguments.push(input.as_os_str().to_owned());
    Ok(Some(arguments))
}

pub fn build_ffmpeg_arguments(
    input: &Path,
    output: &Path,
    metadata: &MediaMetadata,
    audio_languages: &[String],
    subtitle_languages: &[String],
    dry_run: bool,
) -> Result<Option<Vec<OsString>>, String> {
    let mut audio_tracks = matching_tracks(&metadata.audio_tracks, audio_languages);
    if audio_tracks.is_empty() && !metadata.audio_tracks.is_empty() {
        audio_tracks = metadata.audio_tracks.iter().collect();
    }
    let subtitle_tracks = matching_tracks(&metadata.subtitle_tracks, subtitle_languages);
    if audio_tracks.is_empty() {
        return Err("No matching audio tracks found".to_owned());
    }
    if dry_run {
        return Ok(None);
    }

    let mut arguments = vec![OsString::from("-i"), input.as_os_str().to_owned()];
    arguments.extend([OsString::from("-map"), OsString::from("0:v?")]);
    for track in audio_tracks.iter().chain(subtitle_tracks.iter()) {
        let index = track
            .ffprobe_index
            .ok_or_else(|| "FFprobe stream indices are unavailable".to_owned())?;
        arguments.extend([OsString::from("-map"), OsString::from(format!("0:{index}"))]);
    }
    arguments.extend([
        OsString::from("-map"),
        OsString::from("0:t?"),
        OsString::from("-c"),
        OsString::from("copy"),
        OsString::from("-map_metadata"),
        OsString::from("0"),
        OsString::from("-map_chapters"),
        OsString::from("0"),
        OsString::from("-progress"),
        OsString::from("pipe:1"),
        OsString::from("-nostats"),
        OsString::from("-y"),
        output.as_os_str().to_owned(),
    ]);
    Ok(Some(arguments))
}

fn matching_tracks<'a>(tracks: &'a [TrackInfo], languages: &[String]) -> Vec<&'a TrackInfo> {
    let languages = normalize_languages(languages);
    if languages.is_empty() {
        return tracks.iter().collect();
    }
    tracks
        .iter()
        .filter(|track| languages.contains(&track.language.to_ascii_lowercase()))
        .collect()
}

fn join_ids(ids: &[u64]) -> String {
    ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",")
}

pub fn parse_progress_line(line: &str) -> Option<u32> {
    static PROGRESS: OnceLock<Regex> = OnceLock::new();
    let expression = PROGRESS.get_or_init(|| {
        Regex::new(r"(?i)(?:#GUI#progress|Progress:)\s*([0-9]{1,3})")
            .expect("valid progress expression")
    });
    expression
        .captures(line)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<u32>().ok())
        .map(|value| value.min(100))
}

pub fn parse_ffmpeg_progress_line(line: &str, duration: Option<f64>) -> Option<u32> {
    let duration = duration.filter(|duration| duration.is_finite() && *duration > 0.0)?;
    let microseconds = line
        .strip_prefix("out_time_us=")?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(((microseconds as f64 / 1_000_000.0 / duration) * 100.0).clamp(0.0, 100.0) as u32)
}

#[cfg(test)]
mod tests {
    use super::{
        MediaMetadata, RemuxExitOutcome, build_arguments, build_ffmpeg_arguments,
        classify_remux_exit, default_output_dir, directories_resolve_to_same_path,
        normalize_languages, parse_ffmpeg_progress_line, parse_ffprobe_json, parse_metadata_json,
        parse_progress_line, partial_output_path, paths_resolve_to_same_file, selected_tracks,
    };
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> MediaMetadata {
        parse_metadata_json(include_str!("../tests/fixtures/mkvmerge-identify.json"))
            .expect("fixture should parse")
    }

    #[test]
    fn normalizes_languages_case_and_whitespace() {
        assert_eq!(
            normalize_languages(&[" ENG, jPn ".to_owned(), "spa,ENG".to_owned()]),
            ["eng", "jpn", "spa"]
        );
    }

    #[test]
    fn missing_language_tags_are_undefined() {
        let metadata = fixture();
        assert_eq!(metadata.audio_tracks[2].language, "und");
        assert_eq!(selected_tracks(&metadata, &["UND".to_owned()], &[]).0, [5]);
    }

    #[test]
    fn selects_audio_and_subtitle_track_ids_by_language() {
        let metadata = fixture();
        let (audio, subtitles) = selected_tracks(
            &metadata,
            &["JPN".to_owned(), "eng".to_owned()],
            &["spa".to_owned()],
        );
        assert_eq!(audio, [1, 2]);
        assert_eq!(subtitles, [7]);
    }

    #[test]
    fn builds_track_flags_and_disables_unselected_subtitles() {
        let metadata = fixture();
        let arguments = build_arguments(
            Path::new("source.mkv"),
            Path::new("out/source.mkv"),
            &metadata,
            &["eng".to_owned()],
            &["eng".to_owned()],
            false,
        )
        .expect("arguments should build")
        .expect("not a dry run");
        let arguments: Vec<String> = arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        assert!(arguments.iter().any(|argument| argument == "--gui-mode"));
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--audio-tracks", "1"])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--subtitle-tracks", "6"])
        );

        let without_subtitles = build_arguments(
            Path::new("source.mkv"),
            Path::new("out/source.mkv"),
            &metadata,
            &["eng".to_owned()],
            &["not-present".to_owned()],
            false,
        )
        .expect("arguments should build")
        .expect("not a dry run");
        assert!(
            without_subtitles
                .iter()
                .any(|argument| argument == "--no-subtitles")
        );
    }

    #[test]
    fn dry_run_validates_tracks_without_producing_output_arguments() {
        let arguments = build_arguments(
            Path::new("source.mkv"),
            Path::new("out/source.mkv"),
            &fixture(),
            &["eng".to_owned()],
            &[],
            true,
        )
        .expect("matching dry run should succeed");
        assert!(arguments.is_none());
    }

    #[test]
    fn parses_mkvmerge_progress_and_caps_values() {
        assert_eq!(parse_progress_line("Progress: 46%"), Some(46));
        assert_eq!(parse_progress_line("#GUI#progress 101"), Some(100));
        assert_eq!(parse_progress_line("No progress here"), None);
    }

    #[test]
    fn parses_ffprobe_streams_and_missing_languages() {
        let metadata = parse_ffprobe_json(include_str!("../tests/fixtures/ffprobe-identify.json"))
            .expect("fixture should parse");
        assert_eq!(metadata.video_codec.as_deref(), Some("h264"));
        assert_eq!(
            metadata.video.as_ref().map(|video| video.codec_id.as_str()),
            Some("h264")
        );
        assert_eq!(
            metadata.video.as_ref().and_then(|video| video.width),
            Some(1920)
        );
        assert_eq!(
            metadata.video.as_ref().and_then(|video| video.height),
            Some(1080)
        );
        assert_eq!(
            metadata.video.as_ref().and_then(|video| video.fps),
            Some(24000.0 / 1001.0)
        );
        assert_eq!(
            metadata.video.as_ref().and_then(|video| video.bit_depth),
            Some(10)
        );
        assert_eq!(
            metadata.video.as_ref().and_then(|video| video.interlaced),
            Some(false)
        );
        assert!(metadata.video.as_ref().is_some_and(|video| video.hdr));
        assert_eq!(metadata.duration, Some(12.5));
        assert_eq!(metadata.audio_tracks[0].language, "eng");
        assert_eq!(metadata.audio_tracks[0].ffprobe_index, Some(1));
        assert_eq!(metadata.audio_tracks[1].language, "und");
        assert_eq!(metadata.audio_tracks[1].ffprobe_index, Some(3));
        assert_eq!(metadata.subtitle_tracks[0].language, "spa");
    }

    #[test]
    fn builds_ffmpeg_stream_copy_arguments_with_absolute_indices() {
        let metadata = parse_ffprobe_json(include_str!("../tests/fixtures/ffprobe-identify.json"))
            .expect("fixture should parse");
        let arguments = build_ffmpeg_arguments(
            Path::new("source.mkv"),
            Path::new("out/source.mkv"),
            &metadata,
            &["eng".to_owned()],
            &["spa".to_owned()],
            false,
        )
        .expect("arguments should build")
        .expect("not a dry run")
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:v?"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:1"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:5"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-map", "0:t?"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-c", "copy"]));
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-map_metadata", "0"])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-map_chapters", "0"])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-progress", "pipe:1"])
        );
        assert!(arguments.iter().any(|argument| argument == "-nostats"));
        assert!(arguments.iter().any(|argument| argument == "-y"));
        assert_eq!(arguments.last().map(String::as_str), Some("out/source.mkv"));
    }

    #[test]
    fn parses_ffmpeg_progress_from_format_duration() {
        assert_eq!(
            parse_ffmpeg_progress_line("out_time_us=5000000", Some(10.0)),
            Some(50)
        );
        assert_eq!(
            parse_ffmpeg_progress_line("out_time_us=20000000", Some(10.0)),
            Some(100)
        );
        assert_eq!(
            parse_ffmpeg_progress_line("out_time_us=5000000", None),
            None
        );
        assert_eq!(
            parse_ffmpeg_progress_line("progress=continue", Some(10.0)),
            None
        );
    }

    #[test]
    fn defaults_output_to_a_converted_subfolder() {
        assert_eq!(
            default_output_dir(Path::new("/media/movies")),
            Path::new("/media/movies/converted")
        );
        assert_eq!(
            super::output_path(
                &default_output_dir(Path::new("/media/movies")),
                Path::new("/media/movies/film.mkv")
            ),
            Path::new("/media/movies/converted/film.mkv")
        );
    }

    #[test]
    fn resolves_output_paths_before_comparing_with_input() {
        let directory = temporary_test_directory();
        std::fs::create_dir_all(&directory).expect("test directory should be created");
        let input = directory.join("film.mkv");
        std::fs::write(&input, b"input").expect("input fixture should be written");

        assert!(
            paths_resolve_to_same_file(&input, &directory.join("./film.mkv"))
                .expect("paths should resolve")
        );
        assert!(
            !paths_resolve_to_same_file(&input, &directory.join("other.mkv"))
                .expect("paths should resolve")
        );
        assert!(
            directories_resolve_to_same_path(&directory, &directory.join("."))
                .expect("directories should resolve")
        );

        std::fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn names_partial_outputs_in_the_output_directory() {
        assert_eq!(
            partial_output_path(Path::new("/videos/film.mkv"), 123, 4),
            Path::new("/videos/film.langmux-partial-123-4.mkv")
        );
        assert_eq!(
            partial_output_path(Path::new("film.mkv"), 123, 4),
            Path::new("./film.langmux-partial-123-4.mkv")
        );
    }

    #[test]
    fn partial_output_path_preserves_the_target_extension() {
        assert_eq!(
            partial_output_path(Path::new("/videos/film.mp4"), 123, 4),
            Path::new("/videos/film.langmux-partial-123-4.mp4")
        );
    }

    #[test]
    fn classifies_engine_exit_codes_and_nonempty_outputs() {
        assert_eq!(
            classify_remux_exit(true, Some(0), true),
            RemuxExitOutcome::Success
        );
        assert_eq!(
            classify_remux_exit(true, Some(1), true),
            RemuxExitOutcome::SuccessWithWarnings
        );
        assert_eq!(
            classify_remux_exit(true, Some(1), false),
            RemuxExitOutcome::Failure
        );
        assert_eq!(
            classify_remux_exit(true, Some(2), true),
            RemuxExitOutcome::Failure
        );
        assert_eq!(
            classify_remux_exit(false, Some(1), true),
            RemuxExitOutcome::Failure
        );
        assert_eq!(
            classify_remux_exit(false, Some(0), true),
            RemuxExitOutcome::Success
        );
    }

    fn temporary_test_directory() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after UNIX epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "langmux-path-test-{}-{timestamp}",
            std::process::id()
        ))
    }
}
