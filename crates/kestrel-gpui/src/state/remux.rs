//! Remux page view model: language filters, per-file track edits and added files.
//!
//! Track edits and added files are kept as plain data so a later step can map
//! them to the core remux editor. Language selection and container already map
//! to `kestrel_core::transcode::JobSettings`.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use kestrel_core::formats::Container;
use kestrel_core::inputs::ExistingPolicy;
use kestrel_core::mkv::MediaMetadata;
use kestrel_core::transcode::{AudioSettings, JobSettings, SubtitleSettings};
use kestrel_core::worker::RemuxEngine;

use super::format::{channel_label, codec_label};

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

// TODO(integration): map to kestrel_core::remux_edit::TrackEdits
#[allow(dead_code)] // seam for the core remux_edit mapping, exercised by tests
/// Placeholder for the core mapping: the edits that will be handed to the remux editor.
pub fn pending_core_edits<'a>(state: &'a RemuxState, path: &Path) -> Option<&'a FileEdits> {
    state.edits.get(path)
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
}
