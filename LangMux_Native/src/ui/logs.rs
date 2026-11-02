use crate::app::KestrelApp;
use crate::theme;
use eframe::egui::{self, Align, ComboBox, Layout, RichText, ScrollArea};

pub fn show(app: &mut KestrelApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("ACTIVITY LOG").strong().size(10.0));
        ComboBox::from_id_salt("log-level")
            .selected_text(&app.logs_filter)
            .show_ui(ui, |ui| {
                for filter in ["All", "Info", "Warning", "Error"] {
                    ui.selectable_value(&mut app.logs_filter, filter.to_owned(), filter);
                }
            });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Clear").clicked() {
                app.logs.clear();
            }
            if ui.button("Export").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("kestrel.log")
                    .save_file()
            {
                match std::fs::write(&path, app.logs.join("\n")) {
                    Ok(()) => app.push_log(format!("Log exported to {}", path.display())),
                    Err(error) => app.push_log(format!("Log export failed: {error}")),
                }
            }
            if ui.button("Copy").clicked() {
                ctx.copy_text(app.logs.join("\n"));
            }
        });
    });
    ui.separator();
    ScrollArea::vertical()
        .id_salt("activity-log-lines")
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in app
                .logs
                .iter()
                .rev()
                .filter(|line| match app.logs_filter.as_str() {
                    "Warning" => line.to_ascii_lowercase().contains("warning"),
                    "Error" => {
                        line.to_ascii_lowercase().contains("error")
                            || line.to_ascii_lowercase().contains("failed")
                    }
                    "Info" => {
                        !line.to_ascii_lowercase().contains("warning")
                            && !line.to_ascii_lowercase().contains("error")
                    }
                    _ => true,
                })
            {
                ui.label(
                    RichText::new(line)
                        .monospace()
                        .size(10.0)
                        .color(theme::MUTED),
                );
            }
        });
}
