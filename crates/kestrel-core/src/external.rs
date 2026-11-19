use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    /// FFmpeg codec name found by probing the file (`opus`, `truehd`, `subrip`, ...). `None`
    /// until probed; planning then falls back to the file extension.
    pub codec: Option<String>,
    /// Transcode only: render the subtitle into the video instead of muxing it.
    pub burn_in: bool,
}

/// Fills in `codec` for every track that has none by probing the file with ffprobe. A probe
/// that fails leaves the track on the extension fallback and is returned as a warning.
pub fn resolve_codecs(ffprobe: Option<&Path>, tracks: &mut [ExternalTrack]) -> Vec<String> {
    let mut warnings = Vec::new();
    for track in tracks.iter_mut().filter(|track| track.codec.is_none()) {
        let Some(ffprobe) = ffprobe else {
            warnings.push(format!(
                "{}: ffprobe is unavailable, so the codec is guessed from the file extension",
                track.path.display()
            ));
            continue;
        };
        match crate::mkv::probe_external_codec(ffprobe, &track.path, track.kind) {
            Ok(codec) => track.codec = Some(codec),
            Err(error) => warnings.push(format!(
                "{}: could not probe the file ({error}); the codec is guessed from its extension",
                track.path.display()
            )),
        }
    }
    warnings
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
        assert_eq!(track.codec, None);
    }
}
