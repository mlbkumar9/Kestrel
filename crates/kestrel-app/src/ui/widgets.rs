use crate::theme;
use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, FontFamily, FontId, Frame, Label, Margin, RichText,
    Sense, Stroke, TextEdit,
};

pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        Button::new(RichText::new(label).strong().color(Color32::WHITE))
            .fill(theme::ACCENT)
            .corner_radius(CornerRadius::same(6)),
    )
}

pub fn toggle(ui: &mut egui::Ui, label: &str, value: &mut bool) -> egui::Response {
    let response = ui.horizontal(|ui| {
        let (rect, hit) = ui.allocate_exact_size(egui::vec2(34.0, 18.0), Sense::click());
        if hit.clicked() {
            *value = !*value;
        }
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(9),
            if *value { theme::ACCENT } else { theme::RAISED },
        );
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(9),
            Stroke::new(1.0, if *value { theme::ACCENT } else { theme::BORDER }),
            egui::StrokeKind::Inside,
        );
        ui.painter().circle_filled(
            egui::pos2(
                if *value {
                    rect.right() - 9.0
                } else {
                    rect.left() + 9.0
                },
                rect.center().y,
            ),
            6.0,
            Color32::WHITE,
        );
        ui.label(RichText::new(label).color(theme::TEXT));
        hit
    });
    response.inner
}

pub fn pill(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    ui.add(
        Button::new(RichText::new(label).size(11.0))
            .fill(if selected {
                theme::ACCENT.gamma_multiply(0.2)
            } else {
                theme::RAISED
            })
            .stroke(Stroke::new(
                1.0,
                if selected {
                    theme::ACCENT.gamma_multiply(0.7)
                } else {
                    theme::BORDER
                },
            ))
            .corner_radius(CornerRadius::same(5)),
    )
}

pub fn chip(ui: &mut egui::Ui, label: &str, color: Color32) {
    Frame::new()
        .fill(theme::INSET)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(label).monospace().size(9.5).color(color));
        });
}

pub fn styled_input(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    width: f32,
    id: &'static str,
) -> egui::Response {
    let response = ui.add_sized(
        [width, 32.0],
        TextEdit::singleline(value)
            .id_salt(id)
            .hint_text(hint)
            .text_color(theme::TEXT)
            .background_color(theme::SURFACE)
            .vertical_align(Align::Center)
            .margin(Margin::symmetric(8, 4)),
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(5),
            Stroke::new(1.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response
}

pub fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(
        RichText::new(title.to_ascii_uppercase())
            .size(9.0)
            .strong()
            .color(theme::MUTED),
    );
}

pub fn middle_truncate(ui: &mut egui::Ui, text: &str, max_width: f32) -> String {
    let font = FontId::new(10.5, FontFamily::Monospace);
    let measure = |text: String| {
        ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(text, font.clone(), theme::TEXT)
                .size()
                .x
        })
    };
    if measure(text.to_owned()) <= max_width {
        return text.to_owned();
    }
    let chars = text.chars().collect::<Vec<_>>();
    let (mut front, mut back) = (chars.len().div_ceil(2), chars.len() / 2);
    loop {
        let suffix_start = chars.len().saturating_sub(back);
        let candidate = format!(
            "{}…{}",
            chars[..front].iter().collect::<String>(),
            chars[suffix_start..].iter().collect::<String>()
        );
        if measure(candidate.clone()) <= max_width {
            return candidate;
        }
        if front >= back && front > 0 {
            front -= 1;
        } else if back > 0 {
            back -= 1;
        } else {
            return "…".to_owned();
        }
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_owned();
    }
    let index = ((bytes as f64).ln() / 1024_f64.ln()).floor() as usize;
    let amount = bytes as f64 / 1024_f64.powi(index.min(4) as i32);
    format!("{amount:.1} {}", UNITS[index.min(4)])
}

pub fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "—".to_owned();
    }
    let seconds = seconds.round() as u64;
    let hours = seconds / 3600;
    let minutes = seconds % 3600 / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

pub fn format_eta(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "—".to_owned();
    }
    let seconds = seconds.round() as u64;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

pub fn count_label(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

pub fn preset_display_name(
    id: &str,
    encoders: &crate::encoders::EncoderSet,
    user_presets: &[crate::presets::Preset],
) -> String {
    crate::presets::builtins(encoders)
        .into_iter()
        .chain(user_presets.iter().cloned())
        .find(|preset| preset.id == id)
        .map_or_else(|| id.to_owned(), |preset| preset.name)
}

pub fn tooltip_label(ui: &mut egui::Ui, text: &str, tooltip: &str) {
    ui.add(Label::new(RichText::new(text).color(theme::MUTED)).truncate())
        .on_hover_text(tooltip);
}

pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(theme::CORNER))
        .inner_margin(Margin::same(12))
        .show(ui, add)
}
