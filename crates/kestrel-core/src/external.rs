use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Kind of a track added from a separate file.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalKind {
    #[default]
    Subtitle,
    Audio,
}

/// A subtitle or audio track muxed in from its own file (SRT, ASS, VTT, SUP, or an audio file).
///
/// Used by both remux (stream copy) and transcode jobs. `offset_ms` shifts the track in time
/// (positive delays it). `charset` is the text encoding of a text subtitle file; `None` means UTF-8.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct ExternalTrack {
    pub path: PathBuf,
    pub kind: ExternalKind,
    pub language: Option<String>,
    pub title: Option<String>,
    pub default: bool,
    pub forced: bool,
    pub offset_ms: i64,
    pub charset: Option<String>,
    /// Transcode only: render the subtitle into the video instead of muxing it.
    pub burn_in: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_track_defaults_to_an_unflagged_subtitle() {
        let track: ExternalTrack = serde_json::from_str(r#"{"path":"a.srt"}"#).unwrap();
        assert_eq!(track.kind, ExternalKind::Subtitle);
        assert_eq!(track.offset_ms, 0);
        assert!(!track.default && !track.forced && !track.burn_in);
        assert_eq!(track.charset, None);
    }
}
