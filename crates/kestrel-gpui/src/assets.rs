//! Embedded assets: stroke icons ported from the prototype and the bundled fonts.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$((
            concat!("icons/", $name, ".svg"),
            include_bytes!(concat!("../assets/icons/", $name, ".svg")) as &[u8],
        )),*]
    };
}

/// Sidebar logo. `assets/kestrel-mark.svg` when present at build time, else the app PNG.
#[cfg(kestrel_mark)]
pub const LOGO: &str = "kestrel-mark.svg";
#[cfg(not(kestrel_mark))]
pub const LOGO: &str = "kestrel.png";

#[cfg(kestrel_mark)]
const LOGO_BYTES: &[u8] = include_bytes!("../assets/kestrel-mark.svg");
#[cfg(not(kestrel_mark))]
const LOGO_BYTES: &[u8] = include_bytes!("../../kestrel-app/assets/kestrel.png");

const ICONS: &[(&str, &[u8])] = icons![
    "library",
    "remux",
    "transcode",
    "queue",
    "activity",
    "metrics",
    "settings",
    "search",
    "upload",
    "plus",
    "moon",
    "sun",
    "x",
    "check",
    "chevron-down",
    "chevron-right",
    "chevron-left",
    "more",
    "folder",
    "file",
    "film",
    "trash",
    "refresh",
    "copy",
    "download",
    "alert",
    "info",
    "pause",
    "play",
    "stop",
];

pub const INTER: &[u8] = include_bytes!("../../kestrel-app/assets/Inter-Regular.ttf");
pub const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/JetBrainsMono-Regular.ttf");

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == LOGO {
            return Ok(Some(Cow::Borrowed(LOGO_BYTES)));
        }
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_valid_svg_text() {
        for (name, bytes) in ICONS {
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(text.contains("<svg"), "{name}");
        }
        assert!(Assets.load("icons/library.svg").unwrap().is_some());
        assert!(Assets.load("icons/missing.svg").unwrap().is_none());
        assert!(!Assets.load(LOGO).unwrap().unwrap().is_empty());
    }
}
