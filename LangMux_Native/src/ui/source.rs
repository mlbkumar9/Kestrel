use super::widgets;
use crate::app::{KestrelApp, Step};
use crate::formats::Container;
use crate::inputs::{self, ExistingPolicy, OutputTarget};
use crate::theme;
use eframe::egui::{
    self, Align, Button, ComboBox, CornerRadius, Layout, RichText, ScrollArea, Stroke,
};

pub fn show(app: &mut KestrelApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button("Add files…").clicked() {
            app.add_files_dialog();
        }
        if ui.button("Add folder…").clicked() {
            app.add_folder_dialog();
        }
        if ui
            .add_enabled(
                !app.sources.is_empty()
                    || !app.preferences.input_items.is_empty()
                    || !app.preferences.removed_sources.is_empty(),
                Button::new("Clear"),
            )
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
    if app.tools.ffmpeg.is_none() && app.tools.mkvmerge.is_none() {
        widgets::card(ui, |ui| {
            ui.label(RichText::new("Media tools are not installed").strong());
            ui.label(
                RichText::new(
                    "Install FFmpeg or MKVToolNix, or use the installer/zip which include them.",
                )
                .color(theme::MUTED),
            );
            ui.horizontal(|ui| {
                ui.hyperlink_to("FFmpeg downloads", "https://ffmpeg.org/download.html");
                ui.label(RichText::new("·").color(theme::SUBTLE));
                ui.hyperlink_to("MKVToolNix downloads", "https://mkvtoolnix.download/");
            });
        });
        ui.add_space(12.0);
    }
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

fn empty_state(app: &mut KestrelApp, ui: &mut egui::Ui) {
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
            ui.label(RichText::new("Drop video files or folders").color(theme::MUTED));
            ui.label(RichText::new("MKV, MP4, MOV, WebM, AVI, TS and more").color(theme::SUBTLE));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Add files…").clicked() {
                    app.add_files_dialog();
                }
                if ui.button("Add folder…").clicked() {
                    app.add_folder_dialog();
                }
            });
            ui.add_space(12.0);
        });
    });
}

fn source_table(app: &mut KestrelApp, ui: &mut egui::Ui) {
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
                "{} · {}",
                widgets::count_label(app.sources.len(), "file", "files"),
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
    let widths = source_column_widths(ui.available_width());
    let header = [
        "",
        "NAME",
        "FORMAT",
        "DURATION",
        "VIDEO",
        "AUDIO",
        "SUBTITLES",
        "SIZE",
        "",
    ];
    let table_width = widths.iter().sum::<f32>();
    let header_rect = ui
        .allocate_exact_size(egui::vec2(table_width, 30.0), egui::Sense::hover())
        .0;
    ui.painter()
        .rect_filled(header_rect, CornerRadius::same(6), theme::SURFACE);
    ui.painter().rect_stroke(
        header_rect,
        CornerRadius::same(6),
        Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );
    for (rect, title) in source_column_rects(header_rect, &widths)
        .into_iter()
        .zip(header)
    {
        let mut cell = table_cell(ui, rect);
        cell.label(RichText::new(title).size(9.0).strong().color(theme::MUTED));
    }
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
                let relative = source.source.relative.display().to_string();
                let full_path = source.source.path.display().to_string();
                let row_rect = ui
                    .allocate_exact_size(egui::vec2(table_width, 40.0), egui::Sense::hover())
                    .0;
                ui.painter().rect_filled(
                    row_rect,
                    CornerRadius::ZERO,
                    if index % 2 == 0 {
                        theme::BACKGROUND
                    } else {
                        theme::SURFACE
                    },
                );
                ui.painter().rect_stroke(
                    row_rect,
                    CornerRadius::ZERO,
                    Stroke::new(0.5, theme::BORDER),
                    egui::StrokeKind::Inside,
                );
                let hovered = ui
                    .interact(
                        row_rect,
                        egui::Id::new(("source-row", index)),
                        egui::Sense::hover(),
                    )
                    .hovered();
                let mut cells = source_column_rects(row_rect, &widths).into_iter();
                let mut checkbox = table_cell(ui, cells.next().unwrap());
                checkbox.checkbox(&mut source.checked, "");
                let mut name = table_cell(ui, cells.next().unwrap());
                name.add(
                    egui::Label::new(RichText::new(&relative).size(11.0).color(theme::TEXT))
                        .truncate(),
                )
                .on_hover_text(&full_path);
                let format = source
                    .source
                    .path
                    .extension()
                    .unwrap_or_else(|| std::ffi::OsStr::new(""))
                    .to_string_lossy()
                    .to_ascii_uppercase();
                widgets::chip(
                    &mut table_cell(ui, cells.next().unwrap()),
                    &format,
                    theme::MUTED,
                );
                if let Some(metadata) = &source.info.metadata {
                    let mut duration = table_cell(ui, cells.next().unwrap());
                    duration.label(
                        RichText::new(
                            metadata
                                .duration
                                .map(widgets::format_duration)
                                .unwrap_or_else(|| "—".into()),
                        )
                        .monospace()
                        .size(10.0),
                    );
                    let mut video_cell = table_cell(ui, cells.next().unwrap());
                    if let Some(video) = &metadata.video {
                        let resolution = match (video.width, video.height) {
                            (Some(width), Some(height)) => format!("{width}×{height}"),
                            _ => video.codec_id.clone(),
                        };
                        video_cell
                            .add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "{} · {}{}",
                                        video.codec_id.to_ascii_uppercase(),
                                        resolution,
                                        if video.hdr { " · HDR" } else { "" }
                                    ))
                                    .size(9.0)
                                    .color(theme::MUTED),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&video.codec_id);
                    }
                    for (cell_rect, tracks, color) in [
                        (cells.next().unwrap(), &metadata.audio_tracks, theme::TEXT),
                        (
                            cells.next().unwrap(),
                            &metadata.subtitle_tracks,
                            theme::MUTED,
                        ),
                    ] {
                        let mut cell = table_cell(ui, cell_rect);
                        cell.horizontal(|cell| {
                            let limit = if cell_rect.width() < 110.0 { 2 } else { 3 };
                            for track in tracks.iter().take(limit) {
                                widgets::chip(cell, &track.language, color);
                            }
                            if tracks.len() > limit {
                                cell.label(
                                    RichText::new(format!("+{}", tracks.len() - limit))
                                        .size(9.0)
                                        .color(theme::MUTED),
                                );
                            }
                        });
                    }
                } else {
                    for cell_rect in cells.by_ref().take(4) {
                        let mut cell = table_cell(ui, cell_rect);
                        if app.scan_loading {
                            cell.spinner();
                        } else {
                            cell.label(RichText::new("—").color(theme::SUBTLE));
                        }
                    }
                }
                let mut size = table_cell(ui, cells.next().unwrap());
                size.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(widgets::format_size(source.info.size))
                            .monospace()
                            .size(10.0),
                    );
                });
                let mut remove_cell = table_cell(ui, cells.next().unwrap());
                remove_cell.with_layout(Layout::right_to_left(Align::Center), |cell| {
                    if hovered
                        && cell
                            .small_button("x")
                            .on_hover_text("Remove source")
                            .clicked()
                    {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                let path = app.sources.remove(index).source.path;
                crate::app::remove_sources(&mut app.preferences, &[path]);
            }
        });
}

fn source_column_rects(rect: egui::Rect, widths: &[f32; 9]) -> [egui::Rect; 9] {
    let mut left = rect.left();
    std::array::from_fn(|index| {
        let cell = egui::Rect::from_min_size(
            egui::pos2(left, rect.top()),
            egui::vec2(widths[index], rect.height()),
        );
        left += widths[index];
        cell
    })
}

fn table_cell(ui: &mut egui::Ui, rect: egui::Rect) -> egui::Ui {
    ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(4.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    )
}

fn source_column_widths(available: f32) -> [f32; 9] {
    let flex = (available - 174.0).max(420.0);
    [
        26.0,
        flex * 0.40,
        58.0,
        flex * 0.09,
        flex * 0.20,
        flex * 0.14,
        flex * 0.17,
        66.0,
        24.0,
    ]
}

fn output_panel(app: &mut KestrelApp, ui: &mut egui::Ui) {
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
                "Save as"
            } else {
                "Save to folder"
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
            app.step = Step::Settings;
        }
    });
}
