use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TrackInfo {
    pub id: u64,
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
    pub estimated_seconds: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct AvailableLanguages {
    pub audio: Vec<String>,
    pub subtitles: Vec<String>,
}

pub fn find_mkvmerge() -> Result<PathBuf, String> {
    let executable = if cfg!(windows) {
        "mkvmerge.exe"
    } else {
        "mkvmerge"
    };

    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(executable);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    let default_windows = Path::new(r"C:\Program Files\MKVToolNix\mkvmerge.exe");
    if cfg!(windows) && default_windows.is_file() {
        return Ok(default_windows.to_path_buf());
    }

    Err("mkvmerge not found. Install MKVToolNix and ensure mkvmerge is on PATH.".to_owned())
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
        estimated_seconds: (total_size > 0)
            .then_some((total_size as f64 / 1_073_741_824.0) * 120.0),
    }
}

pub fn default_output_dir(input: &Path) -> PathBuf {
    input.join("remuxed")
}

pub fn output_path(output_directory: &Path, input: &Path) -> PathBuf {
    output_directory.join(input.file_name().unwrap_or_default())
}

pub fn read_metadata(executable: &Path, input: &Path) -> Result<MediaMetadata, String> {
    let output = command(executable)
        .arg("-J")
        .arg(input)
        .output()
        .map_err(|error| format!("Failed to run mkvmerge: {error}"))?;
    parse_identification_json(&output)
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
    let (audio, subtitles) = selected_tracks(metadata, audio_languages, subtitle_languages);
    if normalize_languages(audio_languages).is_empty() {
        return Err("At least one audio language is required".to_owned());
    }
    if audio.is_empty() {
        return Err("No matching audio tracks found".to_owned());
    }
    if dry_run {
        return Ok(None);
    }

    let mut arguments = vec![
        OsString::from("--gui-mode"),
        OsString::from("-o"),
        output.as_os_str().to_owned(),
        OsString::from("--audio-tracks"),
        OsString::from(join_ids(&audio)),
    ];
    if subtitles.is_empty() {
        arguments.push(OsString::from("--no-subtitles"));
    } else {
        arguments.push(OsString::from("--subtitle-tracks"));
        arguments.push(OsString::from(join_ids(&subtitles)));
    }
    arguments.push(input.as_os_str().to_owned());
    Ok(Some(arguments))
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

pub fn command(executable: &Path) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new(executable);
        command.creation_flags(0x08000000);
        command
    }
    #[cfg(not(windows))]
    {
        Command::new(executable)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MediaMetadata, build_arguments, default_output_dir, normalize_languages,
        parse_metadata_json, parse_progress_line, selected_tracks,
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
