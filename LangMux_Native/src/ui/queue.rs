use super::widgets;
use crate::app::LangMuxApp;
use crate::theme;
use crate::worker::FileStatus;
use eframe::egui::{self, Align, Button, Layout, ProgressBar, RichText, ScrollArea, Stroke};

pub fn show(app: &mut LangMuxApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.heading("Queue");
        ui.label(
            RichText::new(widgets::count_label(app.queue.len(), "job", "jobs"))
                .size(10.0)
                .color(theme::MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Clear finished").clicked() && !app.queue_running {
                app.queue
                    .retain(|job| !matches!(job.status, FileStatus::Success | FileStatus::Skipped));
            }
            let mut dry_run = app.preferences.dry_run;
            if widgets::toggle(ui, "Dry run", &mut dry_run).changed() {
                app.preferences.dry_run = dry_run;
            }
            if app.queue_running {
                if ui
                    .button(if app.paused { "Resume" } else { "Pause" })
                    .clicked()
                {
                    app.toggle_pause();
                }
                if ui
                    .button(RichText::new("Stop").color(theme::ERROR))
                    .clicked()
                {
                    app.confirm_stop = true;
                }
            } else if widgets::primary_button(ui, "Start queue").clicked() {
                app.start_queue(ctx);
            }
        });
    });
    ui.add_space(12.0);
    if app.queue.is_empty() {
        widgets::card(ui, |ui| {
            ui.set_min_height(230.0);
            ui.vertical_centered(|ui| {
                ui.add_space(36.0);
                ui.label(RichText::new("Your queue is empty").size(20.0).strong());
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Add jobs from the Source or Settings step.").color(theme::MUTED),
                );
                ui.add_space(16.0);
                if widgets::primary_button(ui, "Go to Source").clicked() {
                    app.step = crate::app::Step::Source;
                }
            });
        });
        return;
    }

    ScrollArea::vertical()
        .id_salt("queue-jobs")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - 48.0).max(0.0))
        .show(ui, |ui| {
            let mut selected_all = app.queue.iter().all(|job| job.checked);
            if ui.checkbox(&mut selected_all, "Select all jobs").changed() {
                for job in &mut app.queue {
                    job.checked = selected_all;
                }
            }
            ui.add_space(5.0);
            let mut action = None;
            for index in 0..app.queue.len() {
                let preset_name = widgets::preset_display_name(
                    &app.queue[index].preset,
                    &app.encoders,
                    &app.preferences.user_presets,
                );
                let job = &mut app.queue[index];
                let output_path = job.output.display().to_string();
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut job.checked, "");
                            ui.vertical(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(
                                            job.input
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy(),
                                        )
                                        .strong(),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(job.input.display().to_string());
                                widgets::chip(ui, &preset_name, theme::MUTED);
                            });
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                let output_label = widgets::middle_truncate(
                                    ui,
                                    &output_path,
                                    ui.available_width(),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(output_label).size(9.0).color(theme::MUTED),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&output_path);
                                if matches!(job.status, FileStatus::Success)
                                    && let Some(size) = job.output_size
                                {
                                    let saved = job.original_size.saturating_sub(size);
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · saved {}",
                                            widgets::format_size(size),
                                            widgets::format_size(saved)
                                        ))
                                        .size(9.0)
                                        .color(theme::SUCCESS),
                                    );
                                }
                            });
                            ui.add_space(12.0);
                            ui.vertical(|ui| {
                                ui.label(status_label(job.status));
                                if job.status == FileStatus::Processing {
                                    let detail = match (job.speed, job.eta) {
                                        (Some(speed), Some(eta)) => format!(
                                            "{speed:.1}x · ETA {}",
                                            widgets::format_eta(eta)
                                        ),
                                        _ => job.detail.clone().unwrap_or_default(),
                                    };
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            ProgressBar::new(job.percent as f32 / 100.0)
                                                .desired_width(120.0)
                                                .text(format!("{}%", job.percent)),
                                        );
                                        ui.label(
                                            RichText::new(detail).size(9.0).color(theme::MUTED),
                                        );
                                    });
                                } else if let Some(detail) = &job.detail {
                                    ui.label(RichText::new(detail).size(9.0).color(theme::ERROR));
                                }
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("More").clicked() {
                                    action = Some((job.id, "menu"));
                                }
                                if ui
                                    .small_button("Reveal")
                                    .on_hover_text("Reveal output")
                                    .clicked()
                                {
                                    reveal(&job.output);
                                }
                                if ui.small_button("Down").on_hover_text("Move down").clicked() {
                                    action = Some((job.id, "down"));
                                }
                                if ui.small_button("Up").on_hover_text("Move up").clicked() {
                                    action = Some((job.id, "up"));
                                }
                            });
                        });
                    });
                if let Some((id, "menu")) = action {
                    let _ = id;
                    action = None;
                }
            }
            if let Some((id, command)) = action {
                match command {
                    "up" => app.move_job(id, -1),
                    "down" => app.move_job(id, 1),
                    _ => {}
                }
            }
        });

    ui.horizontal(|ui| {
        if ui.button("Edit").clicked()
            && let Some(id) = app.queue.iter().find(|job| job.checked).map(|job| job.id)
        {
            app.edit_job(id);
        }
        if ui.button("Duplicate").clicked()
            && let Some(id) = app.queue.iter().find(|job| job.checked).map(|job| job.id)
        {
            app.duplicate_job(id);
        }
        if ui.button("Retry").clicked()
            && let Some(id) = app
                .queue
                .iter()
                .find(|job| job.checked && job.status == FileStatus::Error)
                .map(|job| job.id)
        {
            app.retry_job(id);
        }
        if ui
            .add_enabled(!app.queue_running, Button::new("Remove selected"))
            .clicked()
        {
            app.remove_selected_queue();
        }
    });
}

fn status_label(status: FileStatus) -> RichText {
    match status {
        FileStatus::Pending => RichText::new("Pending").color(theme::MUTED),
        FileStatus::Processing => RichText::new("Processing").color(theme::ACCENT),
        FileStatus::Success => RichText::new("Completed").color(theme::SUCCESS),
        FileStatus::Skipped => RichText::new("Skipped").color(theme::MUTED),
        FileStatus::Error => RichText::new("Failed").color(theme::ERROR),
        FileStatus::Cancelled => RichText::new("Stopped").color(theme::WARNING),
    }
}

fn reveal(path: &std::path::Path) {
    #[cfg(target_os = "linux")]
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    #[cfg(target_os = "windows")]
    let mut command = crate::tools::command(std::path::Path::new("explorer"));
    #[cfg(target_os = "macos")]
    let mut command = crate::tools::command(std::path::Path::new("open"));
    #[cfg(target_os = "linux")]
    let mut command = crate::tools::command(std::path::Path::new("xdg-open"));
    #[cfg(target_os = "windows")]
    command.arg(format!("/select,{}", path.display()));
    #[cfg(target_os = "macos")]
    command.arg("-R").arg(path);
    #[cfg(target_os = "linux")]
    command.arg(parent);
    let _ = command.spawn();
}
