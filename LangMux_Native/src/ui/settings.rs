use super::widgets;
use crate::app::{LangMuxApp, SettingsTab};
use crate::presets::{self, Preset};
use crate::theme;
use crate::transcode::{
    AudioChoice, BurnIn, Crop, Deinterlace, Denoise, FrameRate, HwCodec, HwVendor, Mixdown,
    RateControl, TenBit, VideoEncode, VideoEncoder, VideoSettings,
};
use eframe::egui::{self, Align, ComboBox, Frame, Layout, RichText, ScrollArea, Slider, Stroke};

pub fn show(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    if app.preview_source.is_none()
        && let Some(source) = app.sources.first()
    {
        app.preview_source = Some(source.source.path.clone());
        app.mark_preview_dirty();
    }
    let before = format!("{:?}", app.preferences.current_settings);
    let content_height = ui.available_height();
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), content_height),
        Layout::top_down(Align::Min),
        |ui| {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(180.0, content_height),
                    Layout::top_down(Align::Min),
                    |ui| presets_panel(app, ui),
                );
                ui.add_space(12.0);
                let preview_width = 342.0_f32.min(ui.available_width() * 0.38);
                let center_width = (ui.available_width() - preview_width - 12.0).max(430.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(center_width, content_height),
                    Layout::top_down(Align::Min),
                    |ui| settings_panel(app, ui),
                );
                ui.add_space(12.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(preview_width, content_height),
                    Layout::top_down(Align::Min),
                    |ui| preview_panel(app, ui),
                );
            });
        },
    );
    if before != format!("{:?}", app.preferences.current_settings) {
        app.mark_preview_dirty();
    }
}

pub fn action_bar(app: &mut LangMuxApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let preset_name = presets::builtins(&app.encoders)
        .into_iter()
        .chain(app.preferences.user_presets.clone())
        .find(|preset| preset.id == app.preferences.selected_preset_id)
        .map_or_else(
            || app.preferences.selected_preset_id.clone(),
            |preset| preset.name,
        );
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} · {}",
                widgets::count_label(app.selected_source_indices().len(), "file", "files"),
                preset_name
            ))
            .size(10.0)
            .color(theme::MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if app.editing_job.is_some() {
                if widgets::primary_button(ui, "Update job").clicked() {
                    app.update_edited_job();
                }
                if ui.button("Cancel").clicked() {
                    app.editing_job = None;
                    app.step = crate::app::Step::Queue;
                }
            } else {
                if widgets::primary_button(ui, "Start now").clicked() {
                    app.add_to_queue(true, ctx);
                }
                if ui.button("Add to queue").clicked() {
                    app.add_to_queue(false, ctx);
                }
            }
        });
    });
}

fn presets_panel(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    widgets::section_title(ui, "Presets");
    ui.add_space(8.0);
    ScrollArea::vertical()
        .id_salt("settings-presets")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut all = presets::builtins(&app.encoders);
            all.extend(app.preferences.user_presets.clone());
            for preset in all {
                let selected = app.preferences.selected_preset_id == preset.id;
                let response = Frame::new()
                    .fill(if selected {
                        theme::ACCENT.gamma_multiply(0.13)
                    } else {
                        theme::SURFACE
                    })
                    .stroke(Stroke::new(
                        1.0,
                        if selected {
                            theme::ACCENT
                        } else {
                            theme::BORDER
                        },
                    ))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(RichText::new(&preset.name).strong());
                        ui.label(
                            RichText::new(&preset.description)
                                .size(12.0)
                                .color(theme::MUTED),
                        );
                    })
                    .response;
                let response = ui.interact(
                    response.rect,
                    egui::Id::new(("preset-card", &preset.id)),
                    egui::Sense::click(),
                );
                if response.clicked() {
                    app.preferences.selected_preset_id = preset.id.clone();
                    app.preferences.current_settings = preset.settings.clone();
                }
                response.on_hover_text(&preset.description);
                ui.add_space(4.0);
            }
        });
    let user_preset = app
        .preferences
        .user_presets
        .iter()
        .find(|preset| preset.id == app.preferences.selected_preset_id);
    if let Some(preset) = user_preset
        && preset.settings != app.preferences.current_settings
    {
        ui.label(RichText::new("Modified").size(9.0).color(theme::WARNING));
        ui.horizontal(|ui| {
            if ui.small_button("Save changes").clicked()
                && let Some(preset) = app
                    .preferences
                    .user_presets
                    .iter_mut()
                    .find(|item| item.id == app.preferences.selected_preset_id)
            {
                preset.settings = app.preferences.current_settings.clone();
            }
            if ui.small_button("Revert").clicked()
                && let Some(preset) = app
                    .preferences
                    .user_presets
                    .iter()
                    .find(|item| item.id == app.preferences.selected_preset_id)
            {
                app.preferences.current_settings = preset.settings.clone();
            }
        });
    }
    ui.add_space(8.0);
    widgets::styled_input(
        ui,
        &mut app.preset_name,
        "Preset name",
        164.0,
        "preset-name",
    );
    if ui.small_button("Save as preset").clicked() && !app.preset_name.trim().is_empty() {
        let name = app.preset_name.trim().to_owned();
        let id = name.to_ascii_lowercase().replace(' ', "-");
        let preset = Preset {
            id: id.clone(),
            name,
            description: "User preset".to_owned(),
            builtin: false,
            settings: app.preferences.current_settings.clone(),
        };
        if let Some(existing) = app
            .preferences
            .user_presets
            .iter_mut()
            .find(|item| item.id == id)
        {
            *existing = preset;
        } else {
            app.preferences.user_presets.push(preset);
        }
        app.preferences.selected_preset_id = id;
    }
}

fn settings_panel(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for (tab, label) in [
            (SettingsTab::Summary, "Summary"),
            (SettingsTab::Video, "Video"),
            (SettingsTab::Audio, "Audio"),
            (SettingsTab::Subtitles, "Subtitles"),
            (SettingsTab::Metadata, "Chapters & Metadata"),
        ] {
            if ui
                .selectable_label(app.settings_tab == tab, label)
                .clicked()
            {
                app.settings_tab = tab;
            }
        }
    });
    ui.separator();
    ScrollArea::vertical()
        .id_salt("settings-content")
        .auto_shrink([false, false])
        .show(ui, |ui| match app.settings_tab {
            SettingsTab::Summary => summary(app, ui),
            SettingsTab::Video => video_settings(app, ui),
            SettingsTab::Audio => audio_settings(app, ui),
            SettingsTab::Subtitles => subtitle_settings(app, ui),
            SettingsTab::Metadata => metadata_settings(app, ui),
        });
}

fn summary(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    let settings = app.preferences.current_settings.clone();
    let selected = app.selected_source_indices();
    ui.heading("Processing summary");
    let audio_languages = summary_languages(app, &selected, &settings.audio.languages, true);
    let subtitle_languages =
        summary_languages(app, &selected, &settings.subtitles.languages, false);
    ui.label(
        RichText::new(format!(
            "Video: {}",
            match &settings.video {
                VideoSettings::Copy => "kept as is (no re-encode)".to_owned(),
                VideoSettings::Encode(encode) => format!(
                    "re-encoded as {}",
                    match &encode.encoder {
                        VideoEncoder::X264 => "H.264".to_owned(),
                        VideoEncoder::X265 => "H.265".to_owned(),
                        VideoEncoder::SvtAv1 => "AV1".to_owned(),
                        VideoEncoder::Vp9 => "VP9".to_owned(),
                        VideoEncoder::Hardware { codec, .. } => match codec {
                            HwCodec::H264 => "H.264".to_owned(),
                            HwCodec::Hevc => "H.265".to_owned(),
                            HwCodec::Av1 => "AV1".to_owned(),
                        },
                    }
                ),
            }
        ))
        .color(theme::TEXT),
    );
    ui.label(
        RichText::new(format!(
            "Audio: {audio_languages} — {}",
            if settings.audio.codec == AudioChoice::Passthrough {
                "copied"
            } else {
                "encoded"
            }
        ))
        .color(theme::TEXT),
    );
    ui.label(
        RichText::new(format!(
            "Subtitles: {}",
            if settings.subtitles.include {
                format!(
                    "{subtitle_languages} — {}",
                    match settings.subtitles.burn_in {
                        BurnIn::None => "copied",
                        BurnIn::ForcedOnly => "forced tracks burned in",
                        BurnIn::FirstSelected => "first selected burned in",
                    }
                )
            } else {
                "excluded".to_owned()
            }
        ))
        .color(theme::TEXT),
    );
    let target = app.output_target_for(&selected);
    let target_path = match target {
        crate::inputs::OutputTarget::File(path) | crate::inputs::OutputTarget::Folder(path) => path,
    };
    let target_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    ui.label(
        RichText::new(format!(
            "Output: {} in …/{}",
            settings.container.label(),
            target_name
        ))
        .color(theme::MUTED),
    );

    let mut warning_sources = std::collections::BTreeMap::<String, Vec<String>>::new();
    for index in selected.iter().copied() {
        if let Some(metadata) = app
            .sources
            .get(index)
            .and_then(|source| source.info.metadata.as_ref())
        {
            for warning in crate::compat::job_warnings(metadata, &settings) {
                warning_sources
                    .entry(warning)
                    .or_default()
                    .push(app.sources[index].source.relative.display().to_string());
            }
        }
    }
    for (warning, mut files) in warning_sources {
        files.sort();
        files.dedup();
        Frame::new()
            .fill(theme::WARNING.gamma_multiply(0.08))
            .stroke(Stroke::new(1.0, theme::WARNING.gamma_multiply(0.45)))
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(warning.clone()).color(theme::TEXT))
                        .on_hover_text(files.join("\n"));
                    ui.label(
                        RichText::new(format!(
                            "({})",
                            widgets::count_label(files.len(), "file", "files")
                        ))
                        .size(10.0)
                        .color(theme::MUTED),
                    )
                    .on_hover_text(files.join("\n"));
                    if warning.starts_with("Video must be re-encoded")
                        && matches!(app.preferences.current_settings.video, VideoSettings::Copy)
                        && ui.small_button("Re-encode video").clicked()
                    {
                        app.preferences.current_settings.video =
                            VideoSettings::Encode(VideoEncode::default());
                    }
                });
            });
    }
}

fn summary_languages(
    app: &LangMuxApp,
    selected: &[usize],
    configured: &[String],
    audio: bool,
) -> String {
    let mut languages = std::collections::BTreeSet::new();
    if configured.is_empty() {
        for index in selected {
            if let Some(metadata) = app
                .sources
                .get(*index)
                .and_then(|source| source.info.metadata.as_ref())
            {
                let tracks = if audio {
                    &metadata.audio_tracks
                } else {
                    &metadata.subtitle_tracks
                };
                for track in tracks {
                    if !track.language.is_empty() && track.language != "und" {
                        languages.insert(language_name(&track.language));
                    }
                }
            }
        }
    } else {
        languages.extend(configured.iter().map(|code| language_name(code)));
    }
    if languages.is_empty() {
        "all available languages".to_owned()
    } else {
        languages.into_iter().collect::<Vec<_>>().join(", ")
    }
}

fn language_name(code: &str) -> String {
    match code.to_ascii_lowercase().as_str() {
        "eng" | "en" => "English".to_owned(),
        "jpn" | "ja" => "Japanese".to_owned(),
        "fra" | "fre" | "fr" => "French".to_owned(),
        "deu" | "ger" | "de" => "German".to_owned(),
        "spa" | "es" => "Spanish".to_owned(),
        _ => code.to_owned(),
    }
}

fn video_settings(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    ui.heading("Video");
    ui.horizontal(|ui| {
        if ui
            .selectable_label(
                matches!(app.preferences.current_settings.video, VideoSettings::Copy),
                "Keep original",
            )
            .clicked()
        {
            app.preferences.current_settings.video = VideoSettings::Copy;
        }
        if ui
            .selectable_label(
                matches!(
                    app.preferences.current_settings.video,
                    VideoSettings::Encode(_)
                ),
                "Re-encode",
            )
            .clicked()
        {
            app.preferences.current_settings.video = VideoSettings::Encode(VideoEncode::default());
        }
    });
    if matches!(app.preferences.current_settings.video, VideoSettings::Copy) {
        ui.label(
            RichText::new("Video streams will be copied without re-encoding.").color(theme::MUTED),
        );
        return;
    }
    let selected_index = app.selected_source_indices().first().copied();
    let hdr = app.selected_source_indices().iter().any(|index| {
        app.sources
            .get(*index)
            .and_then(|source| source.info.metadata.as_ref())
            .and_then(|metadata| metadata.video.as_ref())
            .is_some_and(|video| video.hdr)
    });
    let VideoSettings::Encode(encode) = &mut app.preferences.current_settings.video else {
        return;
    };
    let mut detect_crop = false;
    egui::Grid::new("video-settings-grid")
        .num_columns(2)
        .spacing([8.0, 8.0])
        .show(ui, |ui| {
            ui.add_sized(
                [130.0, 22.0],
                egui::Label::new(RichText::new("Codec").color(theme::MUTED)),
            );
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    for (encoder, label) in [
                        (VideoEncoder::X264, "H.264"),
                        (VideoEncoder::X265, "H.265"),
                        (VideoEncoder::SvtAv1, "AV1"),
                        (VideoEncoder::Vp9, "VP9"),
                    ] {
                        if ui
                            .selectable_label(codec_matches(&encode.encoder, &encoder), label)
                            .clicked()
                        {
                            encode.encoder = encoder;
                        }
                    }
                });
                ui.label(
                    RichText::new(codec_hint(&encode.encoder))
                        .size(10.0)
                        .color(theme::MUTED),
                );
            });
            ui.end_row();

            ui.add_sized(
                [130.0, 22.0],
                egui::Label::new(RichText::new("Encoder").color(theme::MUTED)),
            );
            ComboBox::from_id_salt("video-encoder")
                .selected_text(encoder_label(&encode.encoder))
                .show_ui(ui, |ui| {
                    for encoder in app.encoders.software.iter() {
                        ui.selectable_value(
                            &mut encode.encoder,
                            encoder.clone(),
                            encoder_label(encoder),
                        );
                    }
                    for encoder in app.encoders.hardware.iter() {
                        ui.selectable_value(
                            &mut encode.encoder,
                            encoder.clone(),
                            encoder_label(encoder),
                        );
                    }
                });
            ui.end_row();

            ui.add_sized(
                [130.0, 22.0],
                egui::Label::new(RichText::new("Quality").color(theme::MUTED)),
            );
            ui.horizontal(|ui| {
                if let RateControl::Quality(quality) = &mut encode.rate {
                    ui.label("Smaller file");
                    ui.scope(|ui| {
                        style_slider(ui, 280.0);
                        ui.add_sized(
                            [280.0, 20.0],
                            Slider::new(quality, 0..=100).show_value(false),
                        );
                    });
                    ui.label("Better quality");
                    ui.label(
                        RichText::new(crate::transcode::quality_control_label(
                            &encode.encoder,
                            *quality,
                        ))
                        .size(10.0)
                        .color(theme::MUTED),
                    );
                } else if let RateControl::Bitrate(bitrate) = &mut encode.rate {
                    ui.add(Slider::new(bitrate, 250..=25000).text("kb/s"));
                }
            });
            ui.end_row();

            ui.add_sized(
                [130.0, 22.0],
                egui::Label::new(RichText::new("Speed").color(theme::MUTED)),
            );
            ui.horizontal(|ui| {
                ui.label("Smallest file");
                ui.scope(|ui| {
                    style_slider(ui, 280.0);
                    ui.add_sized(
                        [280.0, 20.0],
                        Slider::new(&mut encode.speed, 0..=4).show_value(false),
                    );
                });
                ui.label("Fastest");
                ui.label(
                    RichText::new(["Slow", "Medium", "Fast", "Faster", "Very fast"]
                        [usize::from(encode.speed.min(4))])
                        .size(10.0)
                        .color(theme::MUTED),
                );
            });
            ui.end_row();

            ui.add_sized(
                [130.0, 22.0],
                egui::Label::new(RichText::new("Resolution").color(theme::MUTED)),
            );
            ComboBox::from_id_salt("resolution")
                .selected_text(
                    encode
                        .max_height
                        .map_or("Source".to_owned(), |height| format!("Up to {height}p")),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut encode.max_height, None, "Source");
                    for height in [2160, 1080, 720, 480] {
                        ui.selectable_value(
                            &mut encode.max_height,
                            Some(height),
                            format!("{height}p"),
                        );
                    }
                });
            ui.end_row();

            ui.add_sized(
                [130.0, 22.0],
                egui::Label::new(RichText::new("Advanced").color(theme::MUTED)),
            );
            egui::CollapsingHeader::new("Video options")
                .default_open(app.advanced_open)
                .show(ui, |ui| {
                    egui::Grid::new("video-advanced-grid")
                        .num_columns(2)
                        .spacing([8.0, 7.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Frame rate").color(theme::MUTED));
                            ComboBox::from_id_salt("frame-rate")
                                .selected_text(match encode.frame_rate {
                                    FrameRate::Source => "Source".to_owned(),
                                    FrameRate::Fixed(fps) => format!("{fps} fps"),
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut encode.frame_rate,
                                        FrameRate::Source,
                                        "Source",
                                    );
                                    for fps in [24.0, 25.0, 30.0, 50.0, 60.0] {
                                        ui.selectable_value(
                                            &mut encode.frame_rate,
                                            FrameRate::Fixed(fps),
                                            format!("{fps} fps"),
                                        );
                                    }
                                });
                            ui.end_row();

                            ui.label(RichText::new("Deinterlace").color(theme::MUTED));
                            ComboBox::from_id_salt("deinterlace")
                                .selected_text(deinterlace_label(encode.deinterlace))
                                .show_ui(ui, |ui| {
                                    for option in
                                        [Deinterlace::Auto, Deinterlace::Off, Deinterlace::On]
                                    {
                                        ui.selectable_value(
                                            &mut encode.deinterlace,
                                            option,
                                            deinterlace_label(option),
                                        );
                                    }
                                });
                            ui.end_row();

                            ui.label(RichText::new("Denoise").color(theme::MUTED));
                            ComboBox::from_id_salt("denoise")
                                .selected_text(denoise_label(encode.denoise))
                                .show_ui(ui, |ui| {
                                    for option in [
                                        Denoise::Off,
                                        Denoise::Light,
                                        Denoise::Medium,
                                        Denoise::Strong,
                                    ] {
                                        ui.selectable_value(
                                            &mut encode.denoise,
                                            option,
                                            denoise_label(option),
                                        );
                                    }
                                });
                            ui.end_row();

                            ui.label(RichText::new("Rate control").color(theme::MUTED));
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        matches!(encode.rate, RateControl::Quality(_)),
                                        "Quality",
                                    )
                                    .clicked()
                                {
                                    encode.rate = RateControl::Quality(60);
                                }
                                if ui
                                    .selectable_label(
                                        matches!(encode.rate, RateControl::Bitrate(_)),
                                        "Bitrate",
                                    )
                                    .clicked()
                                {
                                    encode.rate = RateControl::Bitrate(4000);
                                }
                            });
                            ui.end_row();

                            ui.label(RichText::new("Pixel depth").color(theme::MUTED));
                            ui.horizontal(|ui| {
                                for (option, label) in [
                                    (TenBit::MatchSource, "Match source"),
                                    (TenBit::Off, "8-bit"),
                                    (TenBit::On, "10-bit"),
                                ] {
                                    ui.selectable_value(&mut encode.ten_bit, option, label);
                                }
                            });
                            ui.end_row();

                            ui.label(RichText::new("Crop").color(theme::MUTED));
                            ui.horizontal(|ui| {
                                let custom = matches!(encode.crop, Crop::Custom { .. });
                                if ui.selectable_label(!custom, "None").clicked() {
                                    encode.crop = Crop::None;
                                }
                                if ui.selectable_label(custom, "Custom").clicked() && !custom {
                                    encode.crop = Crop::Custom {
                                        top: 0,
                                        bottom: 0,
                                        left: 0,
                                        right: 0,
                                    };
                                }
                            });
                            ui.end_row();
                            if let Crop::Custom {
                                top,
                                bottom,
                                left,
                                right,
                            } = &mut encode.crop
                            {
                                ui.label("");
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::DragValue::new(top)
                                            .range(0..=9999)
                                            .prefix("T "),
                                    );
                                    ui.add(
                                        egui::DragValue::new(bottom)
                                            .range(0..=9999)
                                            .prefix("B "),
                                    );
                                    ui.add(
                                        egui::DragValue::new(left)
                                            .range(0..=9999)
                                            .prefix("L "),
                                    );
                                    ui.add(
                                        egui::DragValue::new(right)
                                            .range(0..=9999)
                                            .prefix("R "),
                                    );
                                    detect_crop = ui.button("Detect").clicked();
                                });
                                ui.end_row();
                            }

                            ui.label(RichText::new("HDR").color(theme::MUTED));
                            let response = ui.add_enabled(
                                hdr,
                                egui::Checkbox::new(
                                    &mut encode.tone_map_to_sdr,
                                    "Convert HDR to SDR",
                                ),
                            );
                            if !hdr {
                                response.on_hover_text(
                                    "Available when at least one selected source contains HDR video.",
                                );
                                encode.tone_map_to_sdr = false;
                            }
                            ui.end_row();
                        });
                });
            ui.end_row();
        });
    if detect_crop && let Some(index) = selected_index {
        app.detect_crop(index);
    }
}

fn codec_matches(selected: &VideoEncoder, software: &VideoEncoder) -> bool {
    let selected_codec = match selected {
        VideoEncoder::X264 => HwCodec::H264,
        VideoEncoder::X265 => HwCodec::Hevc,
        VideoEncoder::SvtAv1 | VideoEncoder::Vp9 => HwCodec::Av1,
        VideoEncoder::Hardware { codec, .. } => *codec,
    };
    match software {
        VideoEncoder::X264 => selected_codec == HwCodec::H264,
        VideoEncoder::X265 => selected_codec == HwCodec::Hevc,
        VideoEncoder::SvtAv1 => {
            matches!(selected, VideoEncoder::SvtAv1)
                || matches!(
                    selected,
                    VideoEncoder::Hardware {
                        codec: HwCodec::Av1,
                        ..
                    }
                )
        }
        VideoEncoder::Vp9 => matches!(selected, VideoEncoder::Vp9),
        VideoEncoder::Hardware { .. } => false,
    }
}

fn codec_hint(encoder: &VideoEncoder) -> &'static str {
    match encoder {
        VideoEncoder::X264
        | VideoEncoder::Hardware {
            codec: HwCodec::H264,
            ..
        } => "Most compatible",
        VideoEncoder::X265
        | VideoEncoder::Hardware {
            codec: HwCodec::Hevc,
            ..
        } => "Smaller files",
        VideoEncoder::SvtAv1
        | VideoEncoder::Hardware {
            codec: HwCodec::Av1,
            ..
        } => "Smallest, slower",
        VideoEncoder::Vp9 => "Web",
    }
}

fn encoder_label(encoder: &VideoEncoder) -> &'static str {
    match encoder {
        VideoEncoder::X264 => "Software (x264)",
        VideoEncoder::X265 => "Software (x265)",
        VideoEncoder::SvtAv1 => "Software (SVT-AV1)",
        VideoEncoder::Vp9 => "Software (VP9)",
        VideoEncoder::Hardware { vendor, .. } => match vendor {
            HwVendor::Nvenc => "NVIDIA NVENC",
            HwVendor::Qsv => "Intel Quick Sync",
            HwVendor::Amf => "AMD AMF",
            HwVendor::VideoToolbox => "Apple VideoToolbox",
            HwVendor::Vaapi => "VAAPI",
        },
    }
}

fn deinterlace_label(option: Deinterlace) -> &'static str {
    match option {
        Deinterlace::Auto => "Automatic",
        Deinterlace::Off => "Off",
        Deinterlace::On => "On",
    }
}

fn denoise_label(option: Denoise) -> &'static str {
    match option {
        Denoise::Off => "Off",
        Denoise::Light => "Light",
        Denoise::Medium => "Medium",
        Denoise::Strong => "Strong",
    }
}

fn audio_settings(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    ui.heading("Audio");
    let languages = app.available_languages().audio;
    language_chips(
        ui,
        &languages,
        &mut app.preferences.current_settings.audio.languages,
    );
    ui.horizontal(|ui| {
        ui.label("Codec");
        ComboBox::from_id_salt("audio-codec")
            .selected_text(audio_choice_label(
                &app.preferences.current_settings.audio.codec,
            ))
            .show_ui(ui, |ui| {
                for codec in [
                    AudioChoice::Passthrough,
                    AudioChoice::Aac,
                    AudioChoice::Opus,
                    AudioChoice::Ac3,
                    AudioChoice::Eac3,
                    AudioChoice::Flac,
                ] {
                    ui.selectable_value(
                        &mut app.preferences.current_settings.audio.codec,
                        codec.clone(),
                        audio_choice_label(&codec),
                    );
                }
            });
    });
    if app.preferences.current_settings.audio.codec != AudioChoice::Passthrough {
        ui.add(
            Slider::new(
                &mut app.preferences.current_settings.audio.bitrate_kbps,
                64..=640,
            )
            .text("Bitrate kb/s"),
        );
        ui.horizontal(|ui| {
            ui.label("Mixdown");
            ComboBox::from_id_salt("mixdown")
                .selected_text(mixdown_label(
                    app.preferences.current_settings.audio.mixdown,
                ))
                .show_ui(ui, |ui| {
                    for mix in [Mixdown::Source, Mixdown::Stereo, Mixdown::Surround51] {
                        ui.selectable_value(
                            &mut app.preferences.current_settings.audio.mixdown,
                            mix,
                            mixdown_label(mix),
                        );
                    }
                });
        });
    }
}

fn subtitle_settings(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    ui.heading("Subtitles");
    ui.checkbox(
        &mut app.preferences.current_settings.subtitles.include,
        "Include selected subtitle tracks",
    );
    let languages = app.available_languages().subtitles;
    language_chips(
        ui,
        &languages,
        &mut app.preferences.current_settings.subtitles.languages,
    );
    ui.horizontal(|ui| {
        ui.label("Burn in");
        ComboBox::from_id_salt("burn-in")
            .selected_text(burn_in_label(
                app.preferences.current_settings.subtitles.burn_in,
            ))
            .show_ui(ui, |ui| {
                for mode in [BurnIn::None, BurnIn::ForcedOnly, BurnIn::FirstSelected] {
                    ui.selectable_value(
                        &mut app.preferences.current_settings.subtitles.burn_in,
                        mode,
                        burn_in_label(mode),
                    );
                }
            });
    });
}

fn metadata_settings(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    ui.heading("Chapters & metadata");
    ui.checkbox(
        &mut app.preferences.current_settings.keep_chapters,
        "Keep chapters",
    );
    ui.checkbox(
        &mut app.preferences.current_settings.keep_metadata,
        "Keep metadata",
    );
    let mut title = app
        .preferences
        .current_settings
        .title
        .clone()
        .unwrap_or_default();
    if widgets::styled_input(
        ui,
        &mut title,
        "Optional output title",
        ui.available_width(),
        "metadata-title",
    )
    .changed()
    {
        app.preferences.current_settings.title = (!title.is_empty()).then_some(title);
    }
    egui::CollapsingHeader::new("Advanced")
        .default_open(app.advanced_open)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Remux engine");
                ComboBox::from_id_salt("remux-engine")
                    .selected_text(app.preferences.remux_engine.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.preferences.remux_engine,
                            crate::worker::RemuxEngine::MkvToolNix,
                            "MKVToolNix",
                        );
                        ui.selectable_value(
                            &mut app.preferences.remux_engine,
                            crate::worker::RemuxEngine::Ffmpeg,
                            "FFmpeg",
                        );
                    });
            });
            let mut dry = app.preferences.dry_run;
            widgets::toggle(ui, "Dry run", &mut dry);
            app.preferences.dry_run = dry;
        });
}

fn language_chips(ui: &mut egui::Ui, languages: &[String], selected: &mut Vec<String>) {
    ui.label(
        RichText::new("Languages · empty selection means all")
            .size(10.0)
            .color(theme::MUTED),
    );
    ui.horizontal_wrapped(|ui| {
        for language in languages {
            let active = selected
                .iter()
                .any(|item| item.eq_ignore_ascii_case(language));
            if widgets::pill(ui, language, active).clicked() {
                if active {
                    selected.retain(|item| !item.eq_ignore_ascii_case(language));
                } else {
                    selected.push(language.clone());
                    selected.sort();
                }
            }
        }
    });
}

fn preview_panel(app: &mut LangMuxApp, ui: &mut egui::Ui) {
    widgets::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ScrollArea::vertical()
            .id_salt("settings-preview")
            .max_height(ui.available_height())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Preview");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let filtered =
                            ui.selectable_value(&mut app.preview_original, false, "Filtered");
                        let original =
                            ui.selectable_value(&mut app.preview_original, true, "Original");
                        if filtered.changed() || original.changed() {
                            app.mark_preview_dirty();
                        }
                    });
                });
                if app.sources.is_empty() {
                    ui.label(RichText::new("Add a source to preview a frame.").color(theme::MUTED));
                    return;
                }
                let mut selected = app
                    .preview_source
                    .clone()
                    .unwrap_or_else(|| app.sources[0].source.path.clone());
                ComboBox::from_id_salt("preview-source")
                    .selected_text(selected.file_name().unwrap_or_default().to_string_lossy())
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for source in &app.sources {
                            ui.selectable_value(
                                &mut selected,
                                source.source.path.clone(),
                                source.source.relative.display().to_string(),
                            );
                        }
                    });
                if app.preview_source.is_none()
                    || selected != app.preview_source.clone().unwrap_or_default()
                {
                    app.preview_source = Some(selected);
                    app.mark_preview_dirty();
                }
                let duration = app
                    .sources
                    .iter()
                    .find(|source| Some(&source.source.path) == app.preview_source.as_ref())
                    .and_then(|source| source.info.metadata.as_ref())
                    .and_then(|metadata| metadata.duration)
                    .unwrap_or(1.0);
                let timestamp =
                    |seconds: f64| format!("{:02}:{:02}", seconds as u64 / 60, seconds as u64 % 60);
                ui.label(
                    RichText::new(format!(
                        "{} / {}",
                        timestamp(app.preview_time),
                        timestamp(duration)
                    ))
                    .size(10.0)
                    .color(theme::MUTED),
                );
                let slider_width = ui.max_rect().width();
                ui.scope(|ui| {
                    style_slider(ui, slider_width);
                    let response = ui
                        .add_sized(
                            [slider_width, 28.0],
                            Slider::new(&mut app.preview_time, 0.0..=duration.max(1.0))
                                .show_value(false)
                                .handle_shape(egui::style::HandleShape::Circle)
                                .trailing_fill(true),
                        )
                        .on_hover_text("Preview updates after a short pause");
                    if response.changed() {
                        app.mark_preview_dirty();
                    }
                });

                if let Some(bytes) = &app.preview_bytes {
                    if app.preview_texture.is_none()
                        && let Ok(image) = image::load_from_memory(bytes)
                    {
                        let rgba = image.to_rgba8();
                        app.preview_texture = Some(ui.ctx().load_texture(
                            "preview-frame",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [rgba.width() as usize, rgba.height() as usize],
                                rgba.as_raw(),
                            ),
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    if let Some(texture) = &app.preview_texture {
                        let source_size = texture.size_vec2();
                        let image_width = ui.available_width();
                        let image_size = egui::vec2(
                            image_width,
                            image_width * source_size.y / source_size.x.max(1.0),
                        );
                        ui.add(egui::Image::new(texture).fit_to_exact_size(image_size));
                    }
                } else if app.preview_pending {
                    ui.add_space(6.0);
                    ui.spinner();
                } else {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Frame preview").color(theme::SUBTLE));
                }
                if let Some(error) = &app.preview_error {
                    ui.label(RichText::new(error).size(9.0).color(theme::ERROR));
                }
                ui.horizontal(|ui| {
                    if ui.button("Estimate size").clicked()
                        && let Some(index) = app.selected_source_indices().first().copied()
                    {
                        app.estimate_size(index);
                    }
                    if app.estimate_running {
                        ui.spinner();
                    } else if let Some(estimate) = &app.size_estimate {
                        ui.label(
                            RichText::new(match estimate {
                                Ok(bytes) => widgets::format_size(*bytes),
                                Err(error) => error.clone(),
                            })
                            .size(9.0)
                            .color(theme::MUTED),
                        );
                    }
                });
            });
    });
}

fn audio_choice_label(choice: &AudioChoice) -> &'static str {
    match choice {
        AudioChoice::Passthrough => "Copy",
        AudioChoice::Aac => "AAC",
        AudioChoice::Opus => "Opus",
        AudioChoice::Ac3 => "AC-3",
        AudioChoice::Eac3 => "E-AC-3",
        AudioChoice::Flac => "FLAC",
    }
}

fn style_slider(ui: &mut egui::Ui, width: f32) {
    ui.spacing_mut().slider_width = width;
    let inactive = &mut ui.visuals_mut().widgets.inactive;
    inactive.bg_fill = theme::SUBTLE;
    inactive.bg_stroke = Stroke::new(1.0, theme::MUTED);
}

fn mixdown_label(mixdown: Mixdown) -> &'static str {
    match mixdown {
        Mixdown::Source => "Match source",
        Mixdown::Stereo => "Stereo",
        Mixdown::Surround51 => "5.1 surround",
    }
}

fn burn_in_label(burn_in: BurnIn) -> &'static str {
    match burn_in {
        BurnIn::None => "Off",
        BurnIn::ForcedOnly => "Forced only",
        BurnIn::FirstSelected => "First selected",
    }
}
