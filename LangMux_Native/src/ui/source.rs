use super::widgets;
use crate::app::{LangMuxApp, Step};
use crate::formats::Container;
use crate::inputs::{self, ExistingPolicy, OutputTarget};
use crate::theme;
use eframe::egui::{
    self, Align, Button, ComboBox, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea,
    Stroke,
};
use std::path::Path;

pub fn show(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button("＋ Add files").clicked() {
            app.add_files_dialog();
        }
        if ui.button("＋ Add folder").clicked() {
            app.add_folder_dialog();
        }
        if ui
            .add_enabled(!app.sources.is_empty(), Button::new("Clear"))
            .clicked()
        {
            app.clear_sources();
        }
        ui.add_space(8.0);
        widgets::styled_input(
            ui,
            &mut app.search,
            "Search sources",
            210.0,
            "source-search",
        );
    });
    ui.add_space(12.0);
    if app.sources.is_empty() {
        empty_state(app, ui);
    } else {
        ui.horizontal_top(|ui| {
            let output_width = 320.0_f32.min(ui.available_width() * 0.36);
            ui.allocate_ui_with_layout(
                egui::vec2(
                    (ui.available_width() - output_width - 16.0).max(500.0),
                    ui.available_height(),
                ),
                Layout::top_down(Align::Min),
                |ui| source_table(app, ui),
            );
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(
                egui::vec2(output_width, ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| output_panel(app, ui),
            );
        });
    }
}

fn empty_state(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    let rect = ui.available_rect_before_wrap();
    let card = rect.shrink2(egui::vec2((rect.width() - 620.0).max(0.0) / 2.0, 70.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card));
    widgets::card(&mut child, |ui| {
        ui.set_min_width(520.0);
        ui.set_min_height(240.0);
        ui.vertical_centered(|ui| {
            ui.add_space(26.0);
            ui.label(RichText::new("Add media to begin").size(22.0).strong());
            ui.add_space(8.0);
            ui.label(
                RichText::new("Drop supported media here, or choose files and folders to scan.")
                    .color(theme::MUTED),
            );
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Add files").clicked() {
                    app.add_files_dialog();
                }
                if ui.button("Add folder").clicked() {
                    app.add_folder_dialog();
                }
            });
            ui.add_space(12.0);
            ui.label(
                RichText::new("MKV · MP4 · MOV · WebM · AVI · MPEG · TS")
                    .size(10.5)
                    .color(theme::SUBTLE),
            );
        });
    });
}

fn source_table(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let mut all_checked = app.sources.iter().all(|source| source.checked);
        if ui.checkbox(&mut all_checked, "").changed() {
            for source in &mut app.sources {
                source.checked = all_checked;
            }
        }
        ui.label(RichText::new("SOURCE FILES").strong());
        ui.label(
            RichText::new(format!(
                "{} files · {}",
                app.sources.len(),
                widgets::format_size(app.sources.iter().map(|source| source.info.size).sum())
            ))
            .size(10.5)
            .color(theme::MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut include = app.preferences.include_subfolders;
            if widgets::toggle(ui, "Include subfolders", &mut include).clicked() {
                app.preferences.include_subfolders = include;
                app.refresh_sources();
            }
        });
    });
    ui.add_space(6.0);
    Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("  NAME")
                        .size(9.0)
                        .strong()
                        .color(theme::MUTED),
                );
                ui.add_space(160.0);
                ui.label(
                    RichText::new("CONTAINER")
                        .size(9.0)
                        .strong()
                        .color(theme::MUTED),
                );
                ui.add_space(24.0);
                ui.label(
                    RichText::new("DURATION · VIDEO")
                        .size(9.0)
                        .strong()
                        .color(theme::MUTED),
                );
                ui.add_space(55.0);
                ui.label(
                    RichText::new("AUDIO / SUBTITLES")
                        .size(9.0)
                        .strong()
                        .color(theme::MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("SIZE").size(9.0).strong().color(theme::MUTED));
                });
            });
        });
    ScrollArea::vertical()
        .id_salt("source-files")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let query = app.search.to_lowercase();
            let mut remove = None;
            for (index, source) in app.sources.iter_mut().enumerate() {
                if !query.is_empty()
                    && !source
                        .source
                        .relative
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&query)
                    && !source
                        .source
                        .path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&query)
                {
                    continue;
                }
                Frame::new()
                    .fill(if index % 2 == 0 {
                        theme::BACKGROUND
                    } else {
                        theme::SURFACE
                    })
                    .stroke(Stroke::new(0.5, theme::BORDER))
                    .inner_margin(Margin::symmetric(8, 7))
                    .show(ui, |ui| {
                        ui.set_min_height(53.0);
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut source.checked, "");
                            ui.vertical(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(source.source.relative.to_string_lossy())
                                            .size(11.0)
                                            .color(theme::TEXT),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(source.source.path.display().to_string());
                                ui.label(
                                    RichText::new(
                                        source
                                            .source
                                            .path
                                            .parent()
                                            .unwrap_or_else(|| Path::new(""))
                                            .display()
                                            .to_string(),
                                    )
                                    .size(9.0)
                                    .color(theme::SUBTLE),
                                );
                            });
                            ui.add_space(8.0);
                            widgets::chip(
                                ui,
                                &source
                                    .source
                                    .path
                                    .extension()
                                    .unwrap_or_else(|| std::ffi::OsStr::new(""))
                                    .to_string_lossy()
                                    .to_ascii_uppercase(),
                                theme::MUTED,
                            );
                            ui.add_space(8.0);
                            if let Some(metadata) = &source.info.metadata {
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(
                                            metadata
                                                .duration
                                                .map(widgets::format_duration)
                                                .unwrap_or_else(|| "—".into()),
                                        )
                                        .monospace()
                                        .size(10.0),
                                    );
                                    if let Some(video) = &metadata.video {
                                        let resolution = match (video.width, video.height) {
                                            (Some(width), Some(height)) => {
                                                format!("{width}×{height}")
                                            }
                                            _ => video.codec_id.clone(),
                                        };
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · {}{}",
                                                video.codec_id.to_ascii_uppercase(),
                                                resolution,
                                                if video.hdr { " · HDR" } else { "" }
                                            ))
                                            .size(9.0)
                                            .color(theme::MUTED),
                                        );
                                    }
                                });
                                ui.add_space(10.0);
                                ui.vertical(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        for track in metadata.audio_tracks.iter().take(3) {
                                            widgets::chip(ui, &track.language, theme::TEXT);
                                        }
                                    });
                                    ui.horizontal_wrapped(|ui| {
                                        for track in metadata.subtitle_tracks.iter().take(3) {
                                            widgets::chip(ui, &track.language, theme::MUTED);
                                        }
                                    });
                                });
                            } else if app.scan_loading {
                                ui.spinner();
                                ui.label(
                                    RichText::new("Reading metadata…")
                                        .size(10.0)
                                        .color(theme::SUBTLE),
                                );
                            } else {
                                ui.label(
                                    RichText::new("Metadata unavailable")
                                        .size(10.0)
                                        .color(theme::SUBTLE),
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(widgets::format_size(source.info.size))
                                        .monospace()
                                        .size(10.0),
                                );
                                if ui
                                    .small_button("×")
                                    .on_hover_text("Remove source")
                                    .clicked()
                                {
                                    remove = Some(index);
                                }
                            });
                        });
                    });
            }
            if let Some(index) = remove {
                let path = app.sources.remove(index).source.path;
                app.preferences.input_items.retain(
                    |item| !matches!(item, crate::inputs::InputItem::File(file) if file == &path),
                );
            }
        });
}

fn output_panel(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    widgets::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.heading("Output");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Reset").clicked() {
                    app.reset_output_target();
                }
            });
        });
        let selected = app.selected_source_indices();
        let target = app.output_target_for(&selected);
        ui.label(
            RichText::new(if selected.len() == 1 {
                "File target"
            } else {
                "Folder target"
            })
            .size(10.0)
            .color(theme::MUTED),
        );
        let path = match &target {
            OutputTarget::File(path) | OutputTarget::Folder(path) => path,
        };
        let path_label =
            widgets::middle_truncate(ui, &path.display().to_string(), ui.available_width());
        ui.add(egui::Label::new(RichText::new(path_label).monospace().size(9.5)).truncate())
            .on_hover_text(path.display().to_string());
        if ui.button("Browse…").clicked() {
            app.browse_output(&selected);
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new("FORMAT")
                .size(9.0)
                .strong()
                .color(theme::MUTED),
        );
        ComboBox::from_id_salt("output-container")
            .selected_text(app.preferences.current_settings.container.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for container in [
                    Container::Mkv,
                    Container::Mp4,
                    Container::Mov,
                    Container::WebM,
                ] {
                    if ui
                        .selectable_value(
                            &mut app.preferences.current_settings.container,
                            container,
                            container.label(),
                        )
                        .clicked()
                    {
                        app.mark_preview_dirty();
                    }
                }
            });
        ui.add_space(6.0);
        ui.label(
            RichText::new("IF OUTPUT EXISTS")
                .size(9.0)
                .strong()
                .color(theme::MUTED),
        );
        ComboBox::from_id_salt("existing-policy")
            .selected_text(match app.preferences.existing {
                ExistingPolicy::Skip => "Skip",
                ExistingPolicy::Overwrite => "Overwrite",
                ExistingPolicy::Rename => "Rename",
            })
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut app.preferences.existing, ExistingPolicy::Skip, "Skip");
                ui.selectable_value(
                    &mut app.preferences.existing,
                    ExistingPolicy::Overwrite,
                    "Overwrite",
                );
                ui.selectable_value(
                    &mut app.preferences.existing,
                    ExistingPolicy::Rename,
                    "Rename",
                );
            });
        ui.add_space(8.0);
        ui.label(
            RichText::new("PLANNED OUTPUTS")
                .size(9.0)
                .strong()
                .color(theme::MUTED),
        );
        let sources = selected
            .iter()
            .map(|index| app.sources[*index].source.clone())
            .collect::<Vec<_>>();
        if let Ok(plans) = inputs::plan_outputs(
            &sources,
            &target,
            app.preferences.current_settings.container,
        ) {
            for plan in plans.iter().take(3) {
                let label = widgets::middle_truncate(
                    ui,
                    &plan.output.display().to_string(),
                    ui.available_width(),
                );
                ui.add(
                    egui::Label::new(RichText::new(label).size(9.0).color(theme::MUTED)).truncate(),
                )
                .on_hover_text(plan.output.display().to_string());
            }
            if plans.len() > 3 {
                ui.label(
                    RichText::new(format!("…and {} more", plans.len() - 3))
                        .size(9.0)
                        .color(theme::SUBTLE),
                );
            }
        }
        if let Some(error) = &app.output_error {
            ui.label(RichText::new(error).size(10.0).color(theme::ERROR));
        }
        ui.add_space(10.0);
        let reason = if selected.is_empty() {
            "Add source files to continue"
        } else {
            "Continue to settings"
        };
        if selected.is_empty() {
            ui.label(RichText::new(reason).size(9.0).color(theme::SUBTLE));
        } else if widgets::primary_button(ui, "Continue →").clicked() {
            app.preferences.output_target = Some(target);
            app.step = Step::Settings;
        }
    });
}
