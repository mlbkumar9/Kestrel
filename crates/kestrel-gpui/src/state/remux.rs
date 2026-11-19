//! Remux page view model: language filters, per-file track edits and added files.
//!
//! Track edits and added files are kept as plain data. [`core_track_edits`] maps them onto
//! `kestrel_core::remux_edit::TrackEdits`, and language selection and container map onto
//! `kestrel_core::transcode::JobSettings` in [`RemuxState::job_settings`].

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use kestrel_core::external::{ExternalKind, ExternalTrack};
use kestrel_core::formats::Container;
use kestrel_core::inputs::ExistingPolicy;
use kestrel_core::mkv::MediaMetadata;
use kestrel_core::remux_edit::{self, TrackEdit, TrackEdits};
use kestrel_core::transcode::{AudioSettings, JobSettings, SubtitleSettings};
use kestrel_core::worker::RemuxEngine;

use super::format::{channel_label, codec_label};
use super::transcode::language_code;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
}

impl TrackKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Subtitle => "Subtitle",
        }
    }
}

/// One row of the Tracks table, derived from probed metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackRow {
    pub kind: TrackKind,
    /// Track number inside its type, as shown in the `#` column.
    pub number: u64,
    pub codec: String,
}

/// User edits for one source track. Row order matches [`track_rows`].
#[derive(Clone, Debug, PartialEq)]
pub struct RemuxTrackEdit {
    pub keep: bool,
    pub language: String,
    pub title: String,
    pub default: bool,
    pub forced: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddedKind {
    Subtitle,
    Audio,
}

impl AddedKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Subtitle => "Subtitle",
            Self::Audio => "Audio",
        }
    }
}

/// An external subtitle or audio file added to a remux.
#[derive(Clone, Debug, PartialEq)]
pub struct AddedFile {
    pub path: PathBuf,
    pub kind: AddedKind,
    pub language: String,
    pub title: String,
    pub default: bool,
    pub forced: bool,
    pub offset_ms: i32,
    pub charset: String,
}

impl AddedFile {
    pub fn new(path: PathBuf, kind: AddedKind) -> Self {
        let language = guess_language(&path);
        Self {
            path,
            kind,
            language,
            title: "Added from file".to_owned(),
            default: false,
            forced: false,
            offset_ms: 0,
            charset: "UTF-8".to_owned(),
        }
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn codec_label(&self) -> String {
        let ext = self
            .path
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_default();
        format!("{ext} (external)")
    }
}

/// Reads a language code from names like `movie.es.srt` or `movie.eng.ass`.
pub fn guess_language(path: &std::path::Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase());
    let tag = stem
        .as_deref()
        .and_then(|stem| stem.rsplit('.').next().filter(|_| stem.contains('.')));
    match tag {
        Some(tag)
            if (2..=3).contains(&tag.len()) && tag.chars().all(|c| c.is_ascii_alphabetic()) =>
        {
            tag.to_owned()
        }
        _ => "und".to_owned(),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileEdits {
    pub tracks: Vec<RemuxTrackEdit>,
    pub added: Vec<AddedFile>,
}

pub fn track_rows(meta: &MediaMetadata) -> Vec<TrackRow> {
    let mut rows = Vec::new();
    if meta.video.is_some() || meta.video_codec.is_some() {
        rows.push(TrackRow {
            kind: TrackKind::Video,
            number: 0,
            codec: meta
                .video_codec
                .as_deref()
                .map_or_else(|| "video".to_owned(), codec_label),
        });
    }
    for (kind, tracks) in [
        (TrackKind::Audio, &meta.audio_tracks),
        (TrackKind::Subtitle, &meta.subtitle_tracks),
    ] {
        for (index, track) in tracks.iter().enumerate() {
            let mut codec = codec_label(&track.codec);
            if kind == TrackKind::Audio {
                let channels = channel_label(track.channels);
                if !channels.is_empty() {
                    codec = format!("{codec} {channels}");
                }
            }
            rows.push(TrackRow {
                kind,
                number: index as u64,
                codec,
            });
        }
    }
    rows
}

impl FileEdits {
    pub fn from_metadata(meta: &MediaMetadata) -> Self {
        let mut tracks = Vec::new();
        if meta.video.is_some() || meta.video_codec.is_some() {
            tracks.push(RemuxTrackEdit {
                keep: true,
                language: "und".to_owned(),
                title: String::new(),
                default: true,
                forced: false,
            });
        }
        for track in meta.audio_tracks.iter().chain(&meta.subtitle_tracks) {
            tracks.push(RemuxTrackEdit {
                keep: true,
                language: track.language.clone(),
                title: track.title.clone().unwrap_or_default(),
                default: track.default,
                forced: track.forced,
            });
        }
        Self {
            tracks,
            added: Vec::new(),
        }
    }

    /// Number of edits that differ from the probed file.
    pub fn change_count(&self, meta: &MediaMetadata) -> usize {
        let original = Self::from_metadata(meta);
        let changed = self
            .tracks
            .iter()
            .zip(&original.tracks)
            .filter(|(edit, base)| edit != base)
            .count();
        changed + self.added.len()
    }
}

#[derive(Clone, Debug)]
pub struct RemuxState {
    pub container: Container,
    pub engine: RemuxEngine,
    pub destination: String,
    pub collision: ExistingPolicy,
    pub dry_run: bool,
    pub include_subtitles: bool,
    pub audio_langs: BTreeSet<String>,
    pub sub_langs: BTreeSet<String>,
    /// Languages typed into the "+ code" boxes that no source carries.
    pub extra_audio: Vec<String>,
    pub extra_subs: Vec<String>,
    pub edits: HashMap<PathBuf, FileEdits>,
}

impl Default for RemuxState {
    fn default() -> Self {
        Self {
            container: Container::Mkv,
            engine: RemuxEngine::MkvToolNix,
            destination: String::new(),
            collision: ExistingPolicy::Rename,
            dry_run: false,
            include_subtitles: true,
            audio_langs: BTreeSet::new(),
            sub_langs: BTreeSet::new(),
            extra_audio: Vec::new(),
            extra_subs: Vec::new(),
            edits: HashMap::new(),
        }
    }
}

impl RemuxState {
    pub fn toggle_language(&mut self, subtitles: bool, code: &str) {
        let set = if subtitles {
            &mut self.sub_langs
        } else {
            &mut self.audio_langs
        };
        if !set.remove(code) {
            set.insert(code.to_owned());
        }
    }

    /// Adds a custom language code and selects it. Returns false for an invalid code.
    pub fn add_custom_language(&mut self, subtitles: bool, code: &str) -> bool {
        let code = code.trim().to_ascii_lowercase();
        if !(2..=3).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let extra = if subtitles {
            &mut self.extra_subs
        } else {
            &mut self.extra_audio
        };
        if !extra.contains(&code) {
            extra.push(code.clone());
        }
        let set = if subtitles {
            &mut self.sub_langs
        } else {
            &mut self.audio_langs
        };
        set.insert(code);
        true
    }

    pub fn edits_for(&mut self, path: &Path, meta: &MediaMetadata) -> &mut FileEdits {
        let edits = self
            .edits
            .entry(path.to_path_buf())
            .or_insert_with(|| FileEdits::from_metadata(meta));
        if edits.tracks.len() != track_rows(meta).len() {
            let added = std::mem::take(&mut edits.added);
            *edits = FileEdits::from_metadata(meta);
            edits.added = added;
        }
        edits
    }

    /// Settings shared by every remux job: container and language selection.
    pub fn job_settings(&self) -> JobSettings {
        JobSettings {
            container: self.container,
            audio: AudioSettings {
                languages: self.audio_langs.iter().cloned().collect(),
                ..AudioSettings::default()
            },
            subtitles: SubtitleSettings {
                languages: self.sub_langs.iter().cloned().collect(),
                include: self.include_subtitles,
                ..SubtitleSettings::default()
            },
            ..JobSettings::default()
        }
    }

    /// Short description for the queue table.
    pub fn summary(&self) -> String {
        let languages = |set: &BTreeSet<String>| {
            if set.is_empty() {
                "all".to_owned()
            } else {
                set.iter().cloned().collect::<Vec<_>>().join(",")
            }
        };
        let subs = if self.include_subtitles {
            languages(&self.sub_langs)
        } else {
            "none".to_owned()
        };
        format!(
            "{} - audio {} - subs {}",
            self.container.label(),
            languages(&self.audio_langs),
            subs
        )
    }
}

/// Maps one file's edits onto core track edits. Only values that differ from the probed file
/// are sent, so an untouched table produces an empty (no-op) `TrackEdits`. Video rows are not
/// editable and are skipped. Languages must be ISO 639-2 codes (two-letter codes are converted).
pub fn core_track_edits(meta: &MediaMetadata, edits: &FileEdits) -> Result<TrackEdits, String> {
    let original = FileEdits::from_metadata(meta);
    let skip = usize::from(meta.video.is_some() || meta.video_codec.is_some());
    let mut out = TrackEdits::default();
    let sources = meta.audio_tracks.iter().chain(&meta.subtitle_tracks);
    for ((track, edit), base) in sources
        .zip(edits.tracks.iter().skip(skip))
        .zip(original.tracks.iter().skip(skip))
    {
        if edit == base {
            continue;
        }
        let language = if edit.language == base.language || edit.language.trim().is_empty() {
            None
        } else {
            let code = language_code(&edit.language)
                .or_else(|| {
                    (edit.language.trim().eq_ignore_ascii_case("und")).then(|| "und".to_owned())
                })
                .ok_or_else(|| {
                    format!(
                        "Track {}: '{}' is not a language code (use three letters such as eng)",
                        track.id, edit.language
                    )
                })?;
            Some(remux_edit::normalize_language_code(&code)?)
        };
        out.tracks.push(TrackEdit {
            id: track.id,
            keep: edit.keep,
            default: (edit.default != base.default).then_some(edit.default),
            forced: (edit.forced != base.forced).then_some(edit.forced),
            language,
            title: (edit.title != base.title).then(|| edit.title.clone()),
        });
    }
    for file in &edits.added {
        let kind = match file.kind {
            AddedKind::Subtitle => ExternalKind::Subtitle,
            AddedKind::Audio => ExternalKind::Audio,
        };
        let title = file.title.trim();
        let charset = file.charset.trim();
        out.external.push(ExternalTrack {
            path: file.path.clone(),
            kind,
            language: language_code(&file.language),
            title: (!title.is_empty()).then(|| title.to_owned()),
            default: file.default,
            forced: file.forced,
            offset_ms: i64::from(file.offset_ms),
            charset: (kind == ExternalKind::Subtitle
                && !charset.is_empty()
                && !charset.eq_ignore_ascii_case("utf-8"))
            .then(|| charset.to_owned()),
            burn_in: false,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::mkv::{TrackInfo, VideoInfo};

    fn meta() -> MediaMetadata {
        MediaMetadata {
            video_codec: Some("hevc".to_owned()),
            video: Some(VideoInfo::default()),
            audio_tracks: vec![TrackInfo {
                language: "jpn".to_owned(),
                codec: "aac".to_owned(),
                channels: Some(2),
                default: true,
                ..TrackInfo::default()
            }],
            subtitle_tracks: vec![TrackInfo {
                language: "eng".to_owned(),
                codec: "subrip".to_owned(),
                ..TrackInfo::default()
            }],
            ..MediaMetadata::default()
        }
    }

    #[test]
    fn rows_cover_video_audio_and_subtitles() {
        let rows = track_rows(&meta());
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].kind, TrackKind::Audio);
        assert_eq!(rows[1].codec, "AAC 2.0");
        assert_eq!(rows[2].codec, "SRT");
        assert_eq!(FileEdits::from_metadata(&meta()).tracks.len(), 3);
    }

    #[test]
    fn change_count_tracks_edits_and_added_files() {
        let meta = meta();
        let mut edits = FileEdits::from_metadata(&meta);
        assert_eq!(edits.change_count(&meta), 0);
        edits.tracks[1].keep = false;
        edits.added.push(AddedFile::new(
            PathBuf::from("a.es.srt"),
            AddedKind::Subtitle,
        ));
        assert_eq!(edits.change_count(&meta), 2);
    }

    #[test]
    fn language_is_guessed_from_the_file_name() {
        assert_eq!(guess_language(std::path::Path::new("movie.es.srt")), "es");
        assert_eq!(guess_language(std::path::Path::new("movie.srt")), "und");
    }

    #[test]
    fn language_selection_maps_to_job_settings() {
        let mut state = RemuxState::default();
        state.toggle_language(false, "jpn");
        assert!(state.add_custom_language(true, "ENG"));
        assert!(!state.add_custom_language(true, "x"));
        state.container = Container::Mp4;
        let settings = state.job_settings();
        assert_eq!(settings.container, Container::Mp4);
        assert_eq!(settings.audio.languages, vec!["jpn".to_owned()]);
        assert_eq!(settings.subtitles.languages, vec!["eng".to_owned()]);
        assert!(settings.is_remux_only());
    }

    fn meta_with_ids() -> MediaMetadata {
        let mut meta = meta();
        meta.audio_tracks[0].id = 1;
        meta.subtitle_tracks[0].id = 2;
        meta
    }

    #[test]
    fn untouched_edits_map_to_a_no_op() {
        let meta = meta_with_ids();
        let edits = FileEdits::from_metadata(&meta);
        let core = core_track_edits(&meta, &edits).unwrap();
        assert!(core.is_empty());
    }

    #[test]
    fn track_edits_map_by_source_track_id() {
        let meta = meta_with_ids();
        let mut edits = FileEdits::from_metadata(&meta);
        // Rows: 0 video, 1 audio (id 1), 2 subtitle (id 2).
        edits.tracks[1].language = "es".to_owned();
        edits.tracks[1].title = "Commentary".to_owned();
        edits.tracks[1].default = false;
        edits.tracks[2].keep = false;
        let core = core_track_edits(&meta, &edits).unwrap();
        assert_eq!(
            core.tracks,
            vec![
                TrackEdit {
                    id: 1,
                    keep: true,
                    default: Some(false),
                    forced: None,
                    language: Some("spa".to_owned()),
                    title: Some("Commentary".to_owned()),
                },
                TrackEdit {
                    id: 2,
                    keep: false,
                    ..TrackEdit::default()
                },
            ]
        );
        assert!(core.external.is_empty() && core.order.is_empty());
    }

    #[test]
    fn invalid_language_edits_are_rejected_with_the_track_id() {
        let meta = meta_with_ids();
        let mut edits = FileEdits::from_metadata(&meta);
        edits.tracks[2].language = "english!".to_owned();
        let error = core_track_edits(&meta, &edits).unwrap_err();
        assert!(error.contains("Track 2"), "{error}");
    }

    #[test]
    fn added_files_map_to_external_tracks() {
        let meta = meta_with_ids();
        let mut edits = FileEdits::from_metadata(&meta);
        let mut subtitle = AddedFile::new(PathBuf::from("movie.fr.srt"), AddedKind::Subtitle);
        subtitle.default = true;
        subtitle.offset_ms = -250;
        subtitle.charset = "Windows-1252".to_owned();
        let mut audio = AddedFile::new(PathBuf::from("dub.ac3"), AddedKind::Audio);
        audio.language = "und".to_owned();
        audio.title = String::new();
        audio.charset = "Windows-1252".to_owned();
        edits.added.push(subtitle);
        edits.added.push(audio);
        let core = core_track_edits(&meta, &edits).unwrap();
        assert_eq!(
            core.external,
            vec![
                ExternalTrack {
                    path: PathBuf::from("movie.fr.srt"),
                    kind: ExternalKind::Subtitle,
                    language: Some("fra".to_owned()),
                    title: Some("Added from file".to_owned()),
                    default: true,
                    forced: false,
                    offset_ms: -250,
                    charset: Some("Windows-1252".to_owned()),
                    burn_in: false,
                },
                ExternalTrack {
                    path: PathBuf::from("dub.ac3"),
                    kind: ExternalKind::Audio,
                    language: None,
                    title: None,
                    default: false,
                    forced: false,
                    offset_ms: 0,
                    charset: None,
                    burn_in: false,
                },
            ]
        );
    }
}
