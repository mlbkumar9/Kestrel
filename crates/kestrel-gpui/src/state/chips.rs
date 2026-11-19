use kestrel_core::mkv::{self, MediaMetadata};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Subtitles,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageChip {
    pub code: String,
    pub count: usize,
    pub selected: bool,
}
#[derive(Clone, Default)]
pub struct LanguageSelection {
    pub audio: Vec<String>,
    pub subtitles: Vec<String>,
    custom_audio: BTreeSet<String>,
    custom_subtitles: BTreeSet<String>,
}
impl LanguageSelection {
    pub fn new(audio: &str, subtitles: &str) -> Self {
        Self {
            audio: mkv::parse_language_field(audio),
            subtitles: mkv::parse_language_field(subtitles),
            ..Default::default()
        }
    }
    fn codes_mut(&mut self, kind: TrackKind) -> &mut Vec<String> {
        match kind {
            TrackKind::Audio => &mut self.audio,
            TrackKind::Subtitles => &mut self.subtitles,
        }
    }
    pub fn codes(&self, kind: TrackKind) -> &[String] {
        match kind {
            TrackKind::Audio => &self.audio,
            TrackKind::Subtitles => &self.subtitles,
        }
    }
    pub fn toggle(&mut self, kind: TrackKind, input: &str) {
        let selected = self.codes_mut(kind);
        for code in mkv::parse_language_field(input) {
            if selected.contains(&code) {
                selected.retain(|c| c != &code);
            } else {
                selected.push(code);
            }
        }
        *selected = mkv::normalize_languages(selected);
    }
    pub fn add(&mut self, kind: TrackKind, input: &str) {
        let custom = match kind {
            TrackKind::Audio => &mut self.custom_audio,
            TrackKind::Subtitles => &mut self.custom_subtitles,
        };
        custom.extend(mkv::parse_language_field(input));
        let selected = self.codes_mut(kind);
        selected.extend(mkv::parse_language_field(input));
        *selected = mkv::normalize_languages(selected);
    }
    pub fn chips<'a>(
        &self,
        kind: TrackKind,
        metadata: impl IntoIterator<Item = &'a MediaMetadata>,
    ) -> Vec<LanguageChip> {
        let mut counts = BTreeMap::<String, usize>::new();
        for metadata in metadata {
            let tracks = match kind {
                TrackKind::Audio => &metadata.audio_tracks,
                TrackKind::Subtitles => &metadata.subtitle_tracks,
            };
            for track in tracks {
                for code in mkv::parse_language_field(&track.language) {
                    *counts.entry(code).or_default() += 1;
                }
            }
        }
        let selected = self.codes(kind).iter().cloned().collect::<BTreeSet<_>>();
        for code in &selected {
            counts.entry(code.clone()).or_default();
        }
        let custom = match kind {
            TrackKind::Audio => &self.custom_audio,
            TrackKind::Subtitles => &self.custom_subtitles,
        };
        for code in custom {
            counts.entry(code.clone()).or_default();
        }
        counts
            .into_iter()
            .map(|(code, count)| LanguageChip {
                selected: selected.contains(&code),
                code,
                count,
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::mkv::TrackInfo;
    fn track(code: &str) -> TrackInfo {
        TrackInfo {
            language: code.into(),
            ..Default::default()
        }
    }
    #[test]
    fn counts_tracks_and_keeps_selections_independent() {
        let metadata = MediaMetadata {
            audio_tracks: vec![track("ENG"), track("eng"), track("jpn")],
            subtitle_tracks: vec![track("jpn")],
            ..Default::default()
        };
        let mut model = LanguageSelection::new("eng", "jpn");
        assert_eq!(
            model.chips(TrackKind::Audio, [&metadata])[0],
            LanguageChip {
                code: "eng".into(),
                count: 2,
                selected: true
            }
        );
        model.toggle(TrackKind::Audio, "ENG");
        assert!(model.audio.is_empty());
        assert_eq!(model.subtitles, vec!["jpn"]);
        assert_eq!(model.chips(TrackKind::Subtitles, [&metadata])[0].count, 1);
    }
    #[test]
    fn custom_codes_are_normalized_deduplicated_and_visible_without_tracks() {
        let mut model = LanguageSelection::default();
        model.add(TrackKind::Audio, " FRA, fra, xyz , ");
        let chips = model.chips(TrackKind::Audio, []);
        assert_eq!(
            chips.iter().map(|c| c.code.as_str()).collect::<Vec<_>>(),
            vec!["fra", "xyz"]
        );
        assert!(chips.iter().all(|c| c.count == 0 && c.selected));
        model.toggle(TrackKind::Subtitles, "  ");
        assert!(model.subtitles.is_empty());
        model.toggle(TrackKind::Audio, "xyz");
        assert_eq!(model.audio, vec!["fra"]);
        assert_eq!(
            model.chips(TrackKind::Audio, [])[1],
            LanguageChip {
                code: "xyz".into(),
                count: 0,
                selected: false
            }
        );
        assert!(
            LanguageSelection::default()
                .chips(TrackKind::Audio, [])
                .is_empty()
        );
    }
}
