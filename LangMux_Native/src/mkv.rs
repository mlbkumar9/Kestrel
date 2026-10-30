use crate::tools;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TrackInfo {
    pub id: u64,
    pub mkvmerge_id: Option<u64>,
    pub ffprobe_index: Option<u64>,
    pub language: String,
    pub codec: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MediaMetadata {
    pub audio_tracks: Vec<TrackInfo>,
    pub subtitle_tracks: Vec<TrackInfo>,
    pub video_codec: Option<String>,
    pub duration: Option<f64>,
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
        if is_mkv_path(&path) {
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
    input.join("remuxed")
}

pub fn output_path(output_directory: &Path, input: &Path) -> PathBuf {
    output_directory.join(input.file_name().unwrap_or_default())
}

pub fn read_metadata(
    mkvmerge: Option<&Path>,
    ffprobe: Option<&Path>,
    input: &Path,
) -> Result<MediaMetadata, String> {
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
                attach_ffprobe_indices(&mut metadata, &probe_metadata);
                if metadata.duration.is_none() {
                    metadata.duration = probe_metadata.duration;
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
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
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
                title: properties["track_name"].as_str().map(str::to_owned),
            };
            match kind {
                "video" if metadata.video_codec.is_none() => {
                    metadata.video_codec = Some(info.codec);
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
            let info = TrackInfo {
                id: index,
                mkvmerge_id: None,
                ffprobe_index: Some(index),
                language,
                codec: codec.clone(),
                title: stream["tags"]["title"].as_str().map(str::to_owned),
            };
            match kind {
                "video" if metadata.video_codec.is_none() => {
                    metadata.video_codec = Some(codec);
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
    Ok(metadata)
}

fn attach_ffprobe_indices(metadata: &mut MediaMetadata, ffprobe: &MediaMetadata) {
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
                    && candidate.codec.eq_ignore_ascii_case(&track.codec)
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
        }
    }
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
    if normalize_languages(audio_languages).is_empty() {
        return Err("At least one audio language is required".to_owned());
    }
    let audio_tracks = matching_tracks(&metadata.audio_tracks, audio_languages);
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
    if normalize_languages(audio_languages).is_empty() {
        return Err("At least one audio language is required".to_owned());
    }
    let audio_tracks = matching_tracks(&metadata.audio_tracks, audio_languages);
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
        MediaMetadata, build_arguments, build_ffmpeg_arguments, default_output_dir,
        normalize_languages, parse_ffmpeg_progress_line, parse_ffprobe_json, parse_metadata_json,
        parse_progress_line, selected_tracks,
    };
    use std::path::Path;

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
            &[],
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
    fn defaults_output_to_a_remuxed_subfolder() {
        assert_eq!(
            default_output_dir(Path::new("/media/movies")),
            Path::new("/media/movies/remuxed")
        );
        assert_eq!(
            super::output_path(
                &default_output_dir(Path::new("/media/movies")),
                Path::new("/media/movies/film.mkv")
            ),
            Path::new("/media/movies/remuxed/film.mkv")
        );
    }
}
