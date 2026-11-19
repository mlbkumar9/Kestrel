//! Subtitle behaviour settings and helpers for imported subtitle files (HandBrake Subtitles tab).

use super::SubtitleSettings;
use crate::external::{ExternalKind, ExternalTrack};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// When a subtitle is rendered into the video.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BurnBehavior {
    #[default]
    None,
    /// Burn the forced subtitle that translates foreign-language speech.
    ///
    /// FFmpeg cannot scan audio for foreign speech, so this is approximated as a forced-flagged
    /// subtitle track in the primary (first selected) audio language, falling back to any forced
    /// track among the selected ones.
    ForeignAudio,
    FirstSelected,
    /// The foreign-audio track when one exists, otherwise the first selected track.
    ForeignThenFirst,
}

/// Which soft subtitle track is flagged default.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleDefault {
    /// The first subtitle track in the output.
    #[default]
    First,
    None,
    /// The first forced track, if any.
    Forced,
}

impl SubtitleSettings {
    /// The four-way burn-in behaviour, derived from `burn_in` and `burn_in_fallback_first`.
    pub fn burn_behavior(&self) -> BurnBehavior {
        use super::BurnIn;
        match (self.burn_in, self.burn_in_fallback_first) {
            (BurnIn::None, _) => BurnBehavior::None,
            (BurnIn::FirstSelected, _) => BurnBehavior::FirstSelected,
            (BurnIn::ForcedOnly, false) => BurnBehavior::ForeignAudio,
            (BurnIn::ForcedOnly, true) => BurnBehavior::ForeignThenFirst,
        }
    }

    pub fn set_burn_behavior(&mut self, behavior: BurnBehavior) {
        use super::BurnIn;
        (self.burn_in, self.burn_in_fallback_first) = match behavior {
            BurnBehavior::None => (BurnIn::None, false),
            BurnBehavior::ForeignAudio => (BurnIn::ForcedOnly, false),
            BurnBehavior::FirstSelected => (BurnIn::FirstSelected, false),
            BurnBehavior::ForeignThenFirst => (BurnIn::ForcedOnly, true),
        };
    }
}

/// FFmpeg codec id of an imported subtitle file, from its extension.
pub fn external_codec_id(path: &Path) -> Result<&'static str, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "srt" => Ok("subrip"),
        "ass" | "ssa" => Ok("ass"),
        "vtt" => Ok("webvtt"),
        "sub" => Ok("microdvd"),
        "sup" => Ok("hdmv_pgs_subtitle"),
        "idx" => Ok("dvd_subtitle"),
        _ => Err(format!(
            "Unsupported subtitle file type: {}",
            path.display()
        )),
    }
}

pub fn is_image_codec(codec_id: &str) -> bool {
    matches!(codec_id, "hdmv_pgs_subtitle" | "dvd_subtitle")
}

/// Checks the imported tracks for settings the transcode builder cannot honour.
pub fn validate_external(tracks: &[ExternalTrack]) -> Result<(), String> {
    let mut burned = 0;
    for track in tracks {
        if track.kind == ExternalKind::Audio {
            return Err(format!(
                "External audio tracks are not supported in transcode jobs: {}",
                track.path.display()
            ));
        }
        external_codec_id(&track.path)?;
        if let Some(charset) = &track.charset
            && !is_token(charset)
        {
            return Err(format!("Invalid subtitle character set: {charset}"));
        }
        if let Some(language) = &track.language
            && !language.is_empty()
            && !language
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            return Err(format!("Invalid subtitle language: {language}"));
        }
        if track.burn_in {
            burned += 1;
        }
    }
    if burned > 1 {
        return Err("Only one subtitle track can be burned into the video".to_owned());
    }
    Ok(())
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

/// An `-itsoffset`/filter shift in seconds.
pub fn offset_seconds(offset_ms: i64) -> String {
    format!("{}", offset_ms as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcode::BurnIn;

    #[test]
    fn burn_behavior_round_trips_through_the_stored_fields() {
        let mut settings = SubtitleSettings::default();
        for behavior in [
            BurnBehavior::None,
            BurnBehavior::ForeignAudio,
            BurnBehavior::FirstSelected,
            BurnBehavior::ForeignThenFirst,
        ] {
            settings.set_burn_behavior(behavior);
            assert_eq!(settings.burn_behavior(), behavior);
        }
        settings.set_burn_behavior(BurnBehavior::ForeignAudio);
        assert_eq!(settings.burn_in, BurnIn::ForcedOnly);
    }

    #[test]
    fn imported_subtitle_types_and_limits_are_validated() {
        assert_eq!(external_codec_id(Path::new("a.SRT")).unwrap(), "subrip");
        assert_eq!(
            external_codec_id(Path::new("a.sup")).unwrap(),
            "hdmv_pgs_subtitle"
        );
        assert!(external_codec_id(Path::new("a.txt")).is_err());
        let burned = ExternalTrack {
            path: "a.srt".into(),
            burn_in: true,
            ..ExternalTrack::default()
        };
        assert!(validate_external(std::slice::from_ref(&burned)).is_ok());
        assert!(validate_external(&[burned.clone(), burned]).is_err());
        let audio = ExternalTrack {
            path: "a.mka".into(),
            kind: ExternalKind::Audio,
            ..ExternalTrack::default()
        };
        assert!(validate_external(&[audio]).is_err());
    }
}
