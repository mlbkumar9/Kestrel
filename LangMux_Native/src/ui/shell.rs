use super::{logs, queue, settings, source};
use crate::app::{LangMuxApp, Step};
use crate::branding::APP_NAME;
use crate::theme;
use eframe::egui::{self, Align, Button, CornerRadius, Layout, ProgressBar, RichText, Stroke};

pub fn show(app: &mut LangMuxApp, root: &mut egui::Ui, ctx: &egui::Context) {
    egui::Panel::top("app-header")
        .exact_size(48.0)
        .frame(
            egui::Frame::new()
                .fill(theme::BACKGROUND)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .inner_margin(egui::Margin::symmetric(16, 6)),
        )
        .show(root, |ui| {
            let content = ui.available_rect_before_wrap();
            let center_x = content.center().x;
            let tab_widths = [100.0, 100.0, 100.0];
            let nav_width = tab_widths.iter().sum::<f32>();
            let nav_left = center_x - nav_width / 2.0;
            let left_rect =
                egui::Rect::from_min_max(content.min, egui::pos2(nav_left, content.bottom()));
            let right_rect = egui::Rect::from_min_max(
                egui::pos2(nav_left + nav_width, content.top()),
                content.max,
            );
            let mut left_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(left_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            if let Some(logo) = &app.logo {
                left_ui.image((logo.id(), egui::vec2(28.0, 28.0)));
            }
            left_ui.label(RichText::new(APP_NAME).strong().size(15.0));

            let steps = [
                (Step::Source, format!("1  Source ({})", app.sources.len())),
                (Step::Settings, "2  Settings".to_owned()),
                (Step::Queue, format!("3  Queue ({})", app.queue.len())),
            ];
            let mut tab_left = nav_left;
            for ((step, title), width) in steps.into_iter().zip(tab_widths) {
                let selected = app.step == step;
                let response = ui.put(
                    egui::Rect::from_min_size(
                        egui::pos2(tab_left, content.top()),
                        egui::vec2(width, content.height()),
                    ),
                    Button::new(RichText::new(title).size(11.0).color(if selected {
                        theme::TEXT
                    } else {
                        theme::MUTED
                    }))
                    .fill(theme::BACKGROUND)
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(0)),
                );
                if response.clicked() {
                    app.step = step;
                }
                if selected {
                    ui.painter().line_segment(
                        [response.rect.left_bottom(), response.rect.right_bottom()],
                        Stroke::new(2.0, theme::ACCENT),
                    );
                }
                tab_left += width;
            }

            let mut actions_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(right_rect)
                    .layout(Layout::right_to_left(Align::Center)),
            );
            actions_ui.menu_button("More", |ui| {
                if ui.button("About").clicked() {
                    app.about_open = true;
                    ui.close();
                }
                if ui.button("Import presets…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file()
                        && let Ok(contents) = std::fs::read_to_string(path)
                        && let Ok(imported) = crate::presets::import_user_presets(&contents)
                    {
                        app.preferences.user_presets.extend(imported);
                    }
                    ui.close();
                }
                if ui.button("Export presets…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("langmux-presets.json")
                        .save_file()
                        && let Ok(contents) =
                            crate::presets::export_user_presets(&app.preferences.user_presets)
                    {
                        let _ = std::fs::write(path, contents);
                    }
                    ui.close();
                }
            });
            if actions_ui
                .add(
                    Button::new(RichText::new("Logs").color(if app.preferences.logs_open {
                        theme::ACCENT
                    } else {
                        theme::MUTED
                    }))
                    .fill(if app.preferences.logs_open {
                        theme::ACCENT.gamma_multiply(0.14)
                    } else {
                        theme::BACKGROUND
                    })
                    .stroke(Stroke::new(
                        1.0,
                        if app.preferences.logs_open {
                            theme::ACCENT.gamma_multiply(0.6)
                        } else {
                            theme::BORDER
                        },
                    )),
                )
                .clicked()
            {
                app.preferences.logs_open = !app.preferences.logs_open;
            }
            let found = usize::from(app.tools.mkvmerge.is_some())
                + usize::from(app.tools.ffmpeg.is_some())
                + usize::from(app.tools.ffprobe.is_some());
            let color = if found == 3 {
                theme::SUCCESS
            } else if app.tools.ffmpeg.is_none() {
                theme::ERROR
            } else {
                theme::WARNING
            };
            actions_ui.menu_button(RichText::new("● Tools").color(color), |ui| {
                for (name, tool) in [
                    ("mkvmerge", app.tools.mkvmerge.as_ref()),
                    ("ffmpeg", app.tools.ffmpeg.as_ref()),
                    ("ffprobe", app.tools.ffprobe.as_ref()),
                ] {
                    let (status, version, path) = tool.map_or(
                        (
                            "Missing",
                            "Not found".to_owned(),
                            "Bundle or PATH".to_owned(),
                        ),
                        |tool| {
                            (
                                "Available",
                                tool.version
                                    .clone()
                                    .unwrap_or_else(|| "Version unknown".to_owned()),
                                tool.path.display().to_string(),
                            )
                        },
                    );
                    ui.label(RichText::new(format!("{name} · {status}")).strong());
                    ui.label(RichText::new(version).size(9.0).color(theme::MUTED));
                    ui.label(RichText::new(path).size(9.0).color(theme::SUBTLE));
                    ui.separator();
                }
                ui.label(
                    RichText::new(format!(
                        "Hardware encoders: {}",
                        app.encoders.hardware.len()
                    ))
                    .size(10.0),
                );
            });
        });
    egui::Panel::bottom("queue-status")
        .exact_size(40.0)
        .frame(
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .inner_margin(egui::Margin::symmetric(14, 4)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                let completed = app
                    .queue
                    .iter()
                    .filter(|job| {
                        matches!(
                            job.status,
                            crate::worker::FileStatus::Success | crate::worker::FileStatus::Skipped
                        )
                    })
                    .count();
                let active_index = app
                    .queue
                    .iter()
                    .position(|job| job.status == crate::worker::FileStatus::Processing)
                    .map(|index| index + 1);
                let active = app
                    .queue
                    .iter()
                    .find(|job| job.status == crate::worker::FileStatus::Processing);
                let status = if let Some(index) = active_index {
                    let detail =
                        active.map_or_else(String::new, |job| match (job.speed, job.eta) {
                            (Some(speed), Some(eta)) => format!(
                                " · {speed:.1}x · {} left",
                                crate::ui::widgets::format_duration(eta)
                            ),
                            _ => String::new(),
                        });
                    format!("Encoding {index} of {}{detail}", app.queue.len())
                } else if app.queue_running {
                    "Queue running".to_owned()
                } else {
                    format!("Idle · {} queued · {completed} completed", app.queue.len())
                };
                ui.label(RichText::new(status).size(10.0).color(theme::MUTED));
                let overall = if app.queue.is_empty() {
                    0.0
                } else {
                    app.queue.iter().map(|job| job.percent as f32).sum::<f32>()
                        / (100.0 * app.queue.len() as f32)
                };
                ui.add(
                    ProgressBar::new(overall)
                        .desired_width(ui.available_width().min(220.0))
                        .desired_height(8.0),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_enabled(app.queue_running, Button::new("Stop"))
                        .clicked()
                    {
                        app.confirm_stop = true;
                    }
                    if ui
                        .add_enabled(
                            app.queue_running,
                            Button::new(if app.paused { "Resume" } else { "Pause" }),
                        )
                        .clicked()
                    {
                        app.toggle_pause();
                    }
                });
            });
        });
    if app.step == Step::Settings {
        egui::Panel::bottom("settings-action-bar")
            .exact_size(44.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .inner_margin(egui::Margin::symmetric(14, 4)),
            )
            .show(root, |ui| settings::action_bar(app, ui, ctx));
    }
    if app.preferences.logs_open {
        egui::Panel::bottom("logs-drawer")
            .resizable(true)
            .default_size(170.0)
            .min_size(90.0)
            .max_size(380.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::INSET)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .inner_margin(egui::Margin::symmetric(14, 8)),
            )
            .show(root, |ui| logs::show(app, ui, ctx));
    }
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BACKGROUND)
                .inner_margin(egui::Margin::symmetric(18, 14)),
        )
        .show(root, |ui| match app.step {
            Step::Source => source::show(app, ui),
            Step::Settings => settings::show(app, ui),
            Step::Queue => queue::show(app, ui, ctx),
        });
    if app.about_open {
        egui::Window::new(format!("About {APP_NAME}"))
            .open(&mut app.about_open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.heading(APP_NAME);
                ui.label("A native media language and stream remuxing utility.");
                ui.label(RichText::new(env!("CARGO_PKG_VERSION")).color(theme::MUTED));
            });
    }
    if app.confirm_stop {
        egui::Window::new("Stop queue?")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Stop the active job and remove its partial output?");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        app.confirm_stop = false;
                    }
                    if ui
                        .add(egui::Button::new("Stop").fill(theme::ERROR))
                        .clicked()
                    {
                        app.stop_queue();
                        app.confirm_stop = false;
                    }
                });
            });
    }
    if let Some((message, at)) = &app.toast
        && at.elapsed().as_secs_f32() < 4.0
    {
        egui::Area::new(egui::Id::new("status-toast"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-22.0, -42.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(message);
                });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }
    if app.drag_hovered {
        let rect = root.clip_rect();
        root.painter().rect_stroke(
            rect.shrink(4.0),
            CornerRadius::same(8),
            Stroke::new(2.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
}
