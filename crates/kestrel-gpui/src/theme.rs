use gpui::{Rgba, rgb};

pub fn hex(value: u32) -> Rgba {
    rgb(value)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub background: Rgba,
    pub sidebar: Rgba,
    pub card: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub muted_text: Rgba,
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub hover: Rgba,
    pub selected: Rgba,
    pub success: Rgba,
    pub warning: Rgba,
    pub danger: Rgba,
    pub on_accent: Rgba,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            background: hex(0x09090b),
            sidebar: hex(0x18181b),
            card: hex(0x18181b),
            border: hex(0x27272a),
            text: hex(0xfafafa),
            muted_text: hex(0xa1a1aa),
            accent: hex(0x3b82f6),
            accent_hover: hex(0x2563eb),
            hover: hex(0x27272a),
            selected: hex(0x172554),
            success: hex(0x10b981),
            warning: hex(0xf59e0b),
            danger: hex(0xef4444),
            on_accent: hex(0xffffff),
        }
    }

    pub fn light() -> Self {
        Self {
            background: hex(0xffffff),
            sidebar: hex(0xf4f4f5),
            card: hex(0xfafafa),
            border: hex(0xe4e4e7),
            text: hex(0x18181b),
            muted_text: hex(0x71717a),
            hover: hex(0xe4e4e7),
            selected: hex(0xdbeafe),
            ..Self::dark()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_have_distinct_surfaces_and_shared_accent() {
        let dark = Theme::dark();
        let light = Theme::light();
        assert_ne!(dark.background, light.background);
        assert_ne!(dark.text, light.text);
        assert_ne!(dark.sidebar, light.sidebar);
        assert_ne!(dark.border, light.border);
        assert_eq!(dark.accent, hex(0x3b82f6));
        assert_eq!(light.accent, dark.accent);
    }

    #[test]
    fn hex_decodes_channels_and_is_opaque() {
        let color = hex(0x123456);
        assert_eq!(color.r, 18.0 / 255.0);
        assert_eq!(color.g, 52.0 / 255.0);
        assert_eq!(color.b, 86.0 / 255.0);
        assert_eq!(color.a, 1.0);
        assert_eq!(hex(0).r, 0.0);
        assert_eq!(hex(0xffffff).r, 1.0);
    }
}
