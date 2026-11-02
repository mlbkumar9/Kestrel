use super::widgets;
use crate::app::{LangMuxApp, SettingsTab};
use crate::presets::{self, Preset};
use crate::theme;
use crate::transcode::{
    AudioChoice, BurnIn, Deinterlace, Denoise, FrameRate, Mixdown, RateControl, TenBit,
    VideoEncode, VideoEncoder, VideoSettings,
};
use eframe::egui::{self, Align, ComboBox, Layout, RichText, ScrollArea, Slider};

pub fn show(app: &mut LangMuxApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    if app.preview_source.is_none() {
        app.preview_source = app.sources.first().map(|source| source.source.path.clone());
    }
    let before = format!("{:?}", app.preferences.current_settings);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(180.0, ui.available_height()),
            Layout::top_down(Align::Min),
            |ui| presets_panel(app, ui),
        );
        ui.add_space(12.0);
        let preview_width = 342.0_f32.min(ui.available_width() * 0.38);
        let center_width = (ui.available_width() - preview_width - 12.0).max(430.0);
        ui.allocate_ui_with_layout(
            egui::vec2(center_width, ui.available_height()),
            Layout::top_down(Align::Min),
            |ui| settings_panel(app, ui),
        );
        ui.add_space(12.0);
        ui.allocate_ui_with_layout(
            egui::vec2(preview_width, ui.available_height()),
            Layout::top_down(Align::Min),
            |ui| preview_panel(app, ui),
        );
    });
    if before != format!("{:?}", app.preferences.current_settings) {
        app.mark_preview_dirty();
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} source(s) · {}",
                app.selected_source_indices().len(),
                app.preferences.selected_preset_id
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
                let response = ui.selectable_label(selected, RichText::new(&preset.name).strong());
                if response.clicked() {
                    app.preferences.selected_preset_id = preset.id.clone();
                    app.preferences.current_settings = preset.settings.clone();
                }
                ui.label(
                    RichText::new(preset.description)
                        .size(9.0)
                        .color(theme::SUBTLE),
                );
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
    ui.heading("Processing summary");
    ui.label(
        RichText::new(format!(
            "{} output · {} video · {} audio · {} subtitle handling",
            settings.container.label(),
            if matches!(settings.video, VideoSettings::Copy) {
                "source video"
            } else {
                "re-encoded video"
            },
            format!("{:?}", settings.audio.codec).to_ascii_lowercase(),
            format!("{:?}", settings.subtitles.burn_in).to_ascii_lowercase()
        ))
        .color(theme::MUTED),
    );
    for index in app.selected_source_indices() {
        if let Some(metadata) = app
            .sources
            .get(index)
            .and_then(|source| source.info.metadata.as_ref())
        {
            for warning in crate::compat::job_warnings(metadata, &settings) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("!").color(theme::WARNING));
                    ui.label(RichText::new(warning).color(theme::TEXT));
                    if ui.small_button("Use MKV").clicked() {
                        app.preferences.current_settings.container = crate::formats::Container::Mkv;
                    }
                });
            }
        }
    }
    ui.add_space(12.0);
    widgets::section_title(ui, "Advanced");
    egui::CollapsingHeader::new("Advanced options")
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
            ui.horizontal(|ui| {
                ui.label("Existing outputs");
                ComboBox::from_id_salt("settings-existing")
                    .selected_text(format!("{:?}", app.preferences.existing))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.preferences.existing,
                            crate::inputs::ExistingPolicy::Skip,
                            "Skip",
                        );
                        ui.selectable_value(
                            &mut app.preferences.existing,
                            crate::inputs::ExistingPolicy::Overwrite,
                            "Overwrite",
                        );
                        ui.selectable_value(
                            &mut app.preferences.existing,
                            crate::inputs::ExistingPolicy::Rename,
                            "Rename",
                        );
                    });
            });
            let mut dry = app.preferences.dry_run;
            widgets::toggle(ui, "Dry run", &mut dry);
            app.preferences.dry_run = dry;
        });
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
    if ui.button("Detect crop").clicked()
        && let Some(index) = selected_index
    {
        app.detect_crop(index);
    }
    let VideoSettings::Encode(encode) = &mut app.preferences.current_settings.video else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        for (encoder, label) in [
            (VideoEncoder::X264, "H.264"),
            (VideoEncoder::X265, "H.265"),
            (VideoEncoder::SvtAv1, "AV1"),
            (VideoEncoder::Vp9, "VP9"),
        ] {
            if ui
                .selectable_label(encode.encoder == encoder, label)
                .clicked()
            {
                encode.encoder = encoder;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Encoder");
        ComboBox::from_id_salt("video-encoder")
            .selected_text(format!("{:?}", encode.encoder))
            .show_ui(ui, |ui| {
                for encoder in app.encoders.software.iter() {
                    ui.selectable_value(
                        &mut encode.encoder,
                        encoder.clone(),
                        format!("{encoder:?}"),
                    );
                }
                for encoder in app.encoders.hardware.iter() {
                    ui.selectable_value(
                        &mut encode.encoder,
                        encoder.clone(),
                        format!("{encoder:?}"),
                    );
                }
            });
    });
    if let RateControl::Quality(quality) = &mut encode.rate {
        ui.horizontal(|ui| {
            ui.label("Quality");
            ui.add(Slider::new(quality, 0..=100).show_value(true));
        });
    } else if let RateControl::Bitrate(bitrate) = &mut encode.rate {
        ui.add(Slider::new(bitrate, 250..=25000).text("kb/s"));
    }
    ui.add(Slider::new(&mut encode.speed, 0..=5).text("Speed"));
    ui.horizontal(|ui| {
        ui.label("Resolution");
        ComboBox::from_id_salt("resolution")
            .selected_text(
                encode
                    .max_height
                    .map_or("Source".to_owned(), |height| format!("Up to {height}p")),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut encode.max_height, None, "Source");
                for height in [2160, 1080, 720, 480] {
                    ui.selectable_value(&mut encode.max_height, Some(height), format!("{height}p"));
                }
            });
    });
    egui::CollapsingHeader::new("Advanced video options")
        .default_open(app.advanced_open)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Frame rate");
                ComboBox::from_id_salt("frame-rate")
                    .selected_text(match encode.frame_rate {
                        FrameRate::Source => "Source".to_owned(),
                        FrameRate::Fixed(fps) => format!("{fps} fps"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut encode.frame_rate, FrameRate::Source, "Source");
                        for fps in [24.0, 25.0, 30.0, 50.0, 60.0] {
                            ui.selectable_value(
                                &mut encode.frame_rate,
                                FrameRate::Fixed(fps),
                                format!("{fps} fps"),
                            );
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Deinterlace");
                ComboBox::from_id_salt("deinterlace")
                    .selected_text(format!("{:?}", encode.deinterlace))
                    .show_ui(ui, |ui| {
                        for option in [Deinterlace::Auto, Deinterlace::Off, Deinterlace::On] {
                            ui.selectable_value(
                                &mut encode.deinterlace,
                                option,
                                format!("{option:?}"),
                            );
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Denoise");
                ComboBox::from_id_salt("denoise")
                    .selected_text(format!("{:?}", encode.denoise))
                    .show_ui(ui, |ui| {
                        for option in [
                            Denoise::Off,
                            Denoise::Light,
                            Denoise::Medium,
                            Denoise::Strong,
                        ] {
                            ui.selectable_value(&mut encode.denoise, option, format!("{option:?}"));
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Rate control");
                if ui
                    .selectable_label(matches!(encode.rate, RateControl::Quality(_)), "Quality")
                    .clicked()
                {
                    encode.rate = RateControl::Quality(60);
                }
                if ui
                    .selectable_label(matches!(encode.rate, RateControl::Bitrate(_)), "Bitrate")
                    .clicked()
                {
                    encode.rate = RateControl::Bitrate(4000);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Pixel depth");
                for option in [TenBit::MatchSource, TenBit::Off, TenBit::On] {
                    ui.selectable_value(&mut encode.ten_bit, option, format!("{option:?}"));
                }
            });
            ui.add_enabled_ui(hdr, |ui| {
                ui.checkbox(&mut encode.tone_map_to_sdr, "Convert HDR to SDR");
            });
            if !hdr {
                encode.tone_map_to_sdr = false;
            }
        });
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
            .selected_text(format!(
                "{:?}",
                app.preferences.current_settings.audio.codec
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
                        format!("{codec:?}"),
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
                .selected_text(format!(
                    "{:?}",
                    app.preferences.current_settings.audio.mixdown
                ))
                .show_ui(ui, |ui| {
                    for mix in [Mixdown::Source, Mixdown::Stereo, Mixdown::Surround51] {
                        ui.selectable_value(
                            &mut app.preferences.current_settings.audio.mixdown,
                            mix,
                            format!("{mix:?}"),
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
            .selected_text(format!(
                "{:?}",
                app.preferences.current_settings.subtitles.burn_in
            ))
            .show_ui(ui, |ui| {
                for mode in [BurnIn::None, BurnIn::ForcedOnly, BurnIn::FirstSelected] {
                    ui.selectable_value(
                        &mut app.preferences.current_settings.subtitles.burn_in,
                        mode,
                        format!("{mode:?}"),
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
        ui.horizontal(|ui| {
            ui.heading("Preview");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.selectable_value(&mut app.preview_original, false, "Filtered");
                ui.selectable_value(&mut app.preview_original, true, "Original");
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
        if selected != app.preview_source.clone().unwrap_or_default() {
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
        ui.add(
            Slider::new(&mut app.preview_time, 0.0..=duration.max(1.0))
                .text("Timeline")
                .show_value(false),
        )
        .on_hover_text("Preview updates after a short pause");
        let height = 170.0_f32.min(ui.available_height() * 0.44);
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
                ui.add(
                    egui::Image::new(texture)
                        .fit_to_exact_size(egui::vec2(ui.available_width(), height)),
                );
            }
        } else if app.preview_pending {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), height),
                Layout::top_down(Align::Center),
                |ui| {
                    ui.add_space(height / 2.0 - 12.0);
                    ui.spinner();
                },
            );
        } else {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), height),
                Layout::top_down(Align::Center),
                |ui| {
                    ui.add_space(height / 2.0 - 9.0);
                    ui.label(RichText::new("Frame preview").color(theme::SUBTLE));
                },
            );
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
}
