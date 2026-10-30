use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle, Theme, Visuals,
};
use std::sync::Arc;

// Palette roles follow crates/monocode-gpui/src/theme.rs and
// crates/monocode-gpui/src/settings/palettes.rs; separators are derived from
// the 7% content-ink stroke in monocode/src/styles/index.css.
pub const BACKGROUND: Color32 = Color32::from_rgb(20, 20, 20);
pub const SURFACE: Color32 = Color32::from_rgb(23, 23, 23);
pub const INSET: Color32 = Color32::from_rgb(13, 13, 13);
pub const RAISED: Color32 = Color32::from_rgb(31, 31, 31);
pub const BORDER: Color32 = Color32::from_rgb(35, 35, 35);
pub const TEXT: Color32 = Color32::from_rgb(235, 235, 235);
pub const MUTED: Color32 = Color32::from_rgb(150, 150, 150);
pub const SUBTLE: Color32 = Color32::from_rgb(105, 105, 105);
pub const ACCENT: Color32 = Color32::from_rgb(77, 163, 245);
pub const SUCCESS: Color32 = Color32::from_rgb(110, 231, 183);
pub const WARNING: Color32 = Color32::from_rgb(251, 191, 36);
pub const ERROR: Color32 = Color32::from_rgb(248, 113, 113);

// MonoCode uses system UI sans, 13px body text, 12.5px code, and 8/12px
// radius families; see crates/monocode-gpui/src/fonts.rs and src/styles/index.css.
pub const SIDEBAR_WIDTH: f32 = 306.0;
pub const SPACE: f32 = 8.0;
pub const CORNER: u8 = 8;

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(Theme::Dark);
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "JetBrainsMono".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/JetBrainsMono-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrainsMono".to_owned());
    ctx.set_fonts(fonts);

    let mut visuals = Visuals::dark();
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = INSET;
    visuals.faint_bg_color = RAISED;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.28);
    visuals.selection.stroke.color = ACCENT;
    visuals.widgets.noninteractive.bg_fill = BACKGROUND;
    visuals.widgets.noninteractive.bg_stroke.color = BORDER;
    visuals.widgets.noninteractive.fg_stroke.color = TEXT;
    visuals.widgets.inactive.bg_fill = RAISED;
    visuals.widgets.inactive.bg_stroke.color = BORDER;
    visuals.widgets.inactive.fg_stroke.color = TEXT;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(43, 43, 43);
    visuals.widgets.hovered.bg_stroke.color = ACCENT.gamma_multiply(0.6);
    visuals.widgets.hovered.fg_stroke.color = TEXT;
    visuals.widgets.active.bg_fill = Color32::from_rgb(45, 67, 87);
    visuals.widgets.active.bg_stroke.color = ACCENT;
    visuals.widgets.active.fg_stroke.color = Color32::WHITE;
    visuals.widgets.open.bg_fill = RAISED;
    visuals.widgets.open.bg_stroke.color = BORDER;
    visuals.widgets.open.fg_stroke.color = TEXT;
    let mut style = (*ctx.style_of(Theme::Dark)).clone();
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(10.0, SPACE);
    style.spacing.button_padding = egui::vec2(12.0, SPACE);
    style.spacing.indent = 16.0;
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(22.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
    );
    ctx.set_style_of(Theme::Dark, style);
}
