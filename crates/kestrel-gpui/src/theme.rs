//! Colour tokens copied from the approved design prototype (`Main.dc.html`).

use gpui::{Global, Rgba, rgb, rgba};

pub fn hex(value: u32) -> Rgba {
    rgb(value)
}

/// Radii and sizes from the prototype.
pub const RADIUS_CONTROL: f32 = 8.0;
pub const RADIUS_CARD: f32 = 12.0;
pub const RADIUS_OVERLAY: f32 = 14.0;

/// Colours for the preset dots in the sidebar, cycled by index.
pub const PRESET_DOTS: [u32; 6] = [0x7AB4FF, 0xF08A4B, 0xC084FC, 0x4ADE80, 0xFBBF24, 0xF87171];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub light: bool,
    pub bg: Rgba,
    pub panel: Rgba,
    pub raised: Rgba,
    pub hover: Rgba,
    pub line: Rgba,
    pub line2: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub faint: Rgba,
    pub accent: Rgba,
    pub accent_soft: Rgba,
    pub accent_text: Rgba,
    pub on_accent: Rgba,
    pub ok: Rgba,
    pub ok_soft: Rgba,
    pub warn: Rgba,
    pub warn_soft: Rgba,
    pub bad: Rgba,
    pub bad_soft: Rgba,
    pub info: Rgba,
    pub info_soft: Rgba,
    pub shadow: Rgba,
    pub scrim: Rgba,
}

impl Global for Theme {}

impl Theme {
    pub fn dark() -> Self {
        Self {
            light: false,
            bg: hex(0x0A0A0C),
            panel: hex(0x0F0F12),
            raised: hex(0x16161A),
            hover: hex(0x1D1D22),
            line: hex(0x222228),
            line2: hex(0x2E2E36),
            text: hex(0xEDEDF0),
            muted: hex(0xA0A0AA),
            faint: hex(0x80808A),
            accent: hex(0xF08A4B),
            accent_soft: rgba(0xF08A4B24),
            accent_text: hex(0xF5A06B),
            on_accent: hex(0x1A0D05),
            ok: hex(0x4ADE80),
            ok_soft: rgba(0x4ADE801F),
            warn: hex(0xFBBF24),
            warn_soft: rgba(0xFBBF241F),
            bad: hex(0xF87171),
            bad_soft: rgba(0xF871711F),
            info: hex(0x7AB4FF),
            info_soft: rgba(0x7AB4FF1F),
            shadow: rgba(0x0000008C),
            scrim: rgba(0x00000073),
        }
    }

    pub fn light() -> Self {
        Self {
            light: true,
            bg: hex(0xF7F7F5),
            panel: hex(0xFFFFFF),
            raised: hex(0xF2F2F0),
            hover: hex(0xEAEAE7),
            line: hex(0xE4E4E0),
            line2: hex(0xD6D6D1),
            text: hex(0x17171A),
            muted: hex(0x55555E),
            faint: hex(0x686871),
            accent: hex(0xD9652A),
            accent_soft: rgba(0xD9652A1A),
            accent_text: hex(0xA9481A),
            on_accent: hex(0xFFFFFF),
            ok: hex(0x15803D),
            ok_soft: rgba(0x15803D1A),
            warn: hex(0xA16207),
            warn_soft: rgba(0xA162071A),
            bad: hex(0xB91C1C),
            bad_soft: rgba(0xB91C1C14),
            info: hex(0x1D5FC4),
            info_soft: rgba(0x1D5FC414),
            shadow: rgba(0x00000024),
            scrim: rgba(0x00000073),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
    System,
}

impl Appearance {
    pub const ALL: [Self; 3] = [Self::Dark, Self::Light, Self::System];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::System => "System",
        }
    }

    /// Resolves the setting to a palette. `system_light` is the OS appearance.
    pub fn resolve(self, system_light: bool) -> Theme {
        match self {
            Self::Dark => Theme::dark(),
            Self::Light => Theme::light(),
            Self::System if system_light => Theme::light(),
            Self::System => Theme::dark(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_match_the_prototype_tokens() {
        let dark = Theme::dark();
        let light = Theme::light();
        assert_eq!(dark.bg, hex(0x0A0A0C));
        assert_eq!(dark.accent, hex(0xF08A4B));
        assert_eq!(light.accent, hex(0xD9652A));
        assert_eq!(light.panel, hex(0xFFFFFF));
        assert_ne!(dark.text, light.text);
        assert!(!dark.light && light.light);
    }

    #[test]
    fn system_follows_the_os() {
        assert_eq!(Appearance::System.resolve(true), Theme::light());
        assert_eq!(Appearance::System.resolve(false), Theme::dark());
        assert_eq!(Appearance::Dark.resolve(true), Theme::dark());
        assert_eq!(Appearance::Light.resolve(false), Theme::light());
    }

    #[test]
    fn hex_decodes_channels_and_is_opaque() {
        let color = hex(0x123456);
        assert_eq!(color.r, 18.0 / 255.0);
        assert_eq!(color.g, 52.0 / 255.0);
        assert_eq!(color.b, 86.0 / 255.0);
        assert_eq!(color.a, 1.0);
    }
}
