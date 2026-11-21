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
/// The SVG paints its body in `currentColor`, which an image cannot inherit, so it is served
/// under two names with the colour filled in: `LOGO` for dark themes, `LOGO_LIGHT` for light.
#[cfg(kestrel_mark)]
pub const LOGO: &str = "kestrel-mark.svg";
#[cfg(not(kestrel_mark))]
pub const LOGO: &str = "kestrel.png";
#[cfg(kestrel_mark)]
pub const LOGO_LIGHT: &str = "kestrel-mark-light.svg";
#[cfg(not(kestrel_mark))]
pub const LOGO_LIGHT: &str = "kestrel.png";

#[cfg(kestrel_mark)]
const LOGO_SVG: &str = include_str!("../assets/kestrel-mark.svg");
#[cfg(not(kestrel_mark))]
const LOGO_PNG: &[u8] = include_bytes!("../../kestrel-app/assets/kestrel.png");

/// Theme text colours (`Theme::dark().text` and `Theme::light().text`) for the logo body.
#[cfg(kestrel_mark)]
const LOGO_ON_DARK: &str = "#EDEDF0";
#[cfg(kestrel_mark)]
const LOGO_ON_LIGHT: &str = "#17171A";

#[cfg(kestrel_mark)]
fn logo_bytes(path: &str) -> Option<Cow<'static, [u8]>> {
    let colour = match path {
        LOGO => LOGO_ON_DARK,
        LOGO_LIGHT => LOGO_ON_LIGHT,
        _ => return None,
    };
    Some(Cow::Owned(
        LOGO_SVG.replace("currentColor", colour).into_bytes(),
    ))
}

#[cfg(not(kestrel_mark))]
fn logo_bytes(path: &str) -> Option<Cow<'static, [u8]>> {
    (path == LOGO).then_some(Cow::Borrowed(LOGO_PNG))
}

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
    "chevron-up",
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
    "arrow-left",
    "arrow-right",
    "panel-left",
    "bell",
    "win-min",
    "win-max",
    "win-restore",
    "win-close",
];

/// Inter 4.1 static faces: Regular, Medium, SemiBold, Bold.
pub const INTER: [&[u8]; 4] = [
    include_bytes!("../assets/Inter-Regular.ttf"),
    include_bytes!("../assets/Inter-Medium.ttf"),
    include_bytes!("../assets/Inter-SemiBold.ttf"),
    include_bytes!("../assets/Inter-Bold.ttf"),
];
pub const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/JetBrainsMono-Regular.ttf");

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = logo_bytes(path) {
            return Ok(Some(bytes));
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
