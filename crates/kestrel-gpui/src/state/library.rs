//! Sources added by the user and their probed metadata.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kestrel_core::inputs::{self, InputItem};
use kestrel_core::mkv::{MediaMetadata, TrackInfo};

use super::format::{channel_label, codec_label, format_duration, format_size};
use super::transcode::ChapterRow;

#[derive(Clone, Debug)]
pub enum Probe {
    Pending,
    Ready(Box<MediaMetadata>),
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Source {
    pub path: PathBuf,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub probe: Probe,
    pub chapters: Vec<ChapterRow>,
    pub selected: bool,
}

impl Source {
    fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_default();
        let size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
        Self {
            path,
            name,
            ext,
            size,
            probe: Probe::Pending,
            chapters: Vec::new(),
            selected: true,
        }
    }

    pub fn metadata(&self) -> Option<&MediaMetadata> {
        match &self.probe {
            Probe::Ready(metadata) => Some(metadata),
            _ => None,
        }
    }

    pub fn stem(&self) -> String {
        self.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.name.clone())
    }

    pub fn video_codec(&self) -> Option<String> {
        self.metadata()
            .and_then(|meta| meta.video_codec.as_deref().map(codec_label))
    }

    pub fn resolution(&self) -> Option<String> {
        let video = self.metadata()?.video.as_ref()?;
        Some(format!("{}x{}", video.width?, video.height?))
    }

    pub fn fps_label(&self) -> Option<String> {
        let fps = self.metadata()?.video.as_ref()?.fps?;
        Some(
            format!("{fps:.3}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned(),
        )
    }

    pub fn hdr_label(&self) -> &'static str {
        match self.metadata().and_then(|meta| meta.video.as_ref()) {
            Some(video) if video.hdr => "HDR",
            _ => "SDR",
        }
    }

    pub fn video_label(&self) -> String {
        match &self.probe {
            Probe::Pending => "Probing...".to_owned(),
            Probe::Failed(_) => "Unreadable".to_owned(),
            Probe::Ready(meta) => {
                let mut parts = Vec::new();
                if let Some(codec) = &meta.video_codec {
                    parts.push(codec_label(codec));
                }
                if let Some(video) = &meta.video {
                    if let Some(depth) = video.bit_depth.filter(|depth| *depth > 8) {
                        parts.push(format!("{depth}b"));
                    }
                    if let (Some(width), Some(height)) = (video.width, video.height) {
                        parts.push(format!("{width}x{height}"));
                    }
                    if video.hdr {
                        parts.push("HDR".to_owned());
                    }
                }
                if parts.is_empty() {
                    "none".to_owned()
                } else {
                    parts.join(" ")
                }
            }
        }
    }

    pub fn audio_label(&self) -> String {
        match self.metadata() {
            None => "-".to_owned(),
            Some(meta) if meta.audio_tracks.is_empty() => "none".to_owned(),
            Some(meta) => {
                let mut parts: Vec<String> = meta
                    .audio_tracks
                    .iter()
                    .take(3)
                    .map(|track| {
                        let mut label = codec_label(&track.codec);
                        let channels = channel_label(track.channels);
                        if !channels.is_empty() {
                            label = format!("{label} {channels}");
                        }
                        format!("{label} {}", track.language)
                    })
                    .collect();
                if meta.audio_tracks.len() > 3 {
                    parts.push(format!("+{}", meta.audio_tracks.len() - 3));
                }
                parts.join(", ")
            }
        }
    }

    pub fn subs_label(&self) -> String {
        match self.metadata() {
            None => "-".to_owned(),
            Some(meta) if meta.subtitle_tracks.is_empty() => "none".to_owned(),
            Some(meta) => {
                let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
                for track in &meta.subtitle_tracks {
                    let codec = codec_label(&track.codec);
                    match groups.iter_mut().find(|(name, _)| *name == codec) {
                        Some((_, languages)) => languages.push(&track.language),
                        None => groups.push((codec, vec![&track.language])),
                    }
                }
                groups
                    .into_iter()
                    .map(|(codec, languages)| format!("{codec} {}", languages.join(", ")))
                    .collect::<Vec<_>>()
                    .join("; ")
            }
        }
    }

    pub fn duration_label(&self) -> String {
        format_duration(self.metadata().and_then(|meta| meta.duration))
    }

    pub fn size_label(&self) -> String {
        format_size(self.size)
    }

    /// Resolution and size, as shown in the Transcode source list.
    pub fn short_meta(&self) -> String {
        match self.resolution() {
            Some(resolution) => format!("{resolution} - {}", self.size_label()),
            None => self.size_label(),
        }
    }

    fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_lowercase();
        [
            self.name.to_lowercase(),
            self.video_label().to_lowercase(),
            self.audio_label().to_lowercase(),
            self.subs_label().to_lowercase(),
        ]
        .iter()
        .any(|haystack| haystack.contains(&needle))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LibraryStats {
    pub files: usize,
    pub total_size: u64,
    pub codecs: Vec<(String, usize)>,
    pub estimated_output: u64,
}

#[derive(Debug, Default)]
pub struct Library {
    pub sources: Vec<Source>,
    pub filter: String,
    /// Index of the source shown in Transcode.
    pub current: usize,
}

impl Library {
    /// Adds files and folders. Returns the paths that were not already present and need probing.
    pub fn add_paths(&mut self, paths: &[PathBuf]) -> Vec<PathBuf> {
        let items: Vec<InputItem> = paths
            .iter()
            .map(|path| {
                if path.is_dir() {
                    InputItem::Folder {
                        path: path.clone(),
                        recursive: true,
                    }
                } else {
                    InputItem::File(path.clone())
                }
            })
            .collect();
        let mut added = Vec::new();
        for source in inputs::expand(&items, &[], &[]) {
            if self.sources.iter().any(|known| known.path == source.path) {
                continue;
            }
            added.push(source.path.clone());
            self.sources.push(Source::new(source.path));
        }
        added
    }

    pub fn set_probe(
        &mut self,
        path: &Path,
        result: Result<MediaMetadata, String>,
        chapters: Vec<ChapterRow>,
    ) {
        if let Some(source) = self.sources.iter_mut().find(|source| source.path == path) {
            source.chapters = chapters;
            source.probe = match result {
                Ok(metadata) => Probe::Ready(Box::new(metadata)),
                Err(error) => Probe::Failed(error),
            };
        }
    }

    /// Indices of sources that match the filter text.
    pub fn visible(&self) -> Vec<usize> {
        self.sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.matches(self.filter.trim()))
            .map(|(index, _)| index)
            .collect()
    }

    pub fn selected(&self) -> Vec<usize> {
        self.sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.selected)
            .map(|(index, _)| index)
            .collect()
    }

    pub fn set_all_selected(&mut self, selected: bool) {
        for index in self.visible() {
            self.sources[index].selected = selected;
        }
    }

    pub fn current_source(&self) -> Option<&Source> {
        self.sources.get(self.current)
    }

    pub fn stats(&self) -> LibraryStats {
        let mut codecs: BTreeMap<String, usize> = BTreeMap::new();
        for source in &self.sources {
            if let Some(codec) = source.video_codec() {
                *codecs.entry(codec).or_default() += 1;
            }
        }
        let mut codecs: Vec<_> = codecs.into_iter().collect();
        codecs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let total_size = self.sources.iter().map(|source| source.size).sum::<u64>();
        LibraryStats {
            files: self.sources.len(),
            total_size,
            codecs,
            // Heuristic: an HEVC encode at the default quality keeps roughly a third of the bytes.
            estimated_output: total_size / 100 * 35,
        }
    }

    /// Languages and how many tracks carry each, over every source, most common first.
    pub fn language_counts(&self, subtitles: bool) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for meta in self.sources.iter().filter_map(Source::metadata) {
            let tracks: &[TrackInfo] = if subtitles {
                &meta.subtitle_tracks
            } else {
                &meta.audio_tracks
            };
            for track in tracks {
                *counts.entry(track.language.clone()).or_default() += 1;
            }
        }
        let mut counts: Vec<_> = counts.into_iter().collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::mkv::VideoInfo;

    fn probed(name: &str, languages: &[&str]) -> Source {
        let mut source = Source::new(PathBuf::from(name));
        source.probe = Probe::Ready(Box::new(MediaMetadata {
            audio_tracks: languages
                .iter()
                .map(|language| TrackInfo {
                    language: (*language).to_owned(),
                    codec: "aac".to_owned(),
                    channels: Some(2),
                    ..TrackInfo::default()
                })
                .collect(),
            video_codec: Some("hevc".to_owned()),
            video: Some(VideoInfo {
                width: Some(1920),
                height: Some(1080),
                ..VideoInfo::default()
            }),
            duration: Some(90.0),
            ..MediaMetadata::default()
        }));
        source
    }

    #[test]
    fn labels_come_from_probed_metadata() {
        let source = probed("a.mkv", &["jpn", "eng"]);
        assert_eq!(source.audio_label(), "AAC 2.0 jpn, AAC 2.0 eng");
        assert_eq!(source.video_label(), "HEVC 1920x1080");
        assert_eq!(source.duration_label(), "0:01:30");
        assert_eq!(source.subs_label(), "none");
    }

    #[test]
    fn stats_and_languages_aggregate_sources() {
        let mut library = Library::default();
        library.sources.push(probed("a.mkv", &["jpn", "eng"]));
        library.sources.push(probed("b.mkv", &["eng"]));
        let stats = library.stats();
        assert_eq!(stats.files, 2);
        assert_eq!(stats.codecs, vec![("HEVC".to_owned(), 2)]);
        assert_eq!(
            library.language_counts(false),
            vec![("eng".to_owned(), 2), ("jpn".to_owned(), 1)]
        );
    }

    #[test]
    fn filter_matches_name_and_language() {
        let mut library = Library::default();
        library.sources.push(probed("alpha.mkv", &["jpn"]));
        library.sources.push(probed("beta.mkv", &["fre"]));
        library.filter = "fre".to_owned();
        assert_eq!(library.visible(), vec![1]);
        library.filter = "ALPHA".to_owned();
        assert_eq!(library.visible(), vec![0]);
    }
}
