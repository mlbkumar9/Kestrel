use serde::{Deserialize, Serialize};
use std::path::Path;

pub const INPUT_EXTENSIONS: &[&str] = &[
    "mkv", "mk3d", "webm", "mp4", "m4v", "mov", "avi", "ts", "m2ts", "mts", "wmv", "flv", "mpg",
    "mpeg", "vob", "3gp", "ogv",
];

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    #[default]
    Mkv,
    Mp4,
    WebM,
    Mov,
}

impl Container {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Mkv => "mkv",
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
            Self::Mov => "mov",
        }
    }

    pub const fn ffmpeg_muxer(self) -> &'static str {
        match self {
            Self::Mkv => "matroska",
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
            Self::Mov => "mov",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mkv => "MKV",
            Self::Mp4 => "MP4",
            Self::WebM => "WebM",
            Self::Mov => "MOV",
        }
    }
}

pub fn is_supported_input(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            INPUT_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported_input_extensions_case_insensitively() {
        for extension in INPUT_EXTENSIONS {
            assert!(is_supported_input(Path::new(&format!("movie.{extension}"))));
            assert!(is_supported_input(Path::new(&format!(
                "movie.{}",
                extension.to_ascii_uppercase()
            ))));
        }
        assert!(!is_supported_input(Path::new("movie.txt")));
        assert!(!is_supported_input(Path::new("movie")));
    }

    #[test]
    fn containers_expose_their_output_names() {
        assert_eq!(Container::Mkv.extension(), "mkv");
        assert_eq!(Container::Mp4.ffmpeg_muxer(), "mp4");
        assert_eq!(Container::WebM.label(), "WebM");
        assert_eq!(Container::Mov.extension(), "mov");
    }
}
