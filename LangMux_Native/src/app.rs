use crate::mkv::{self, AvailableLanguages, BatchStats, FileInfo};
use crate::theme;
use crate::tools::{Tool, Toolchain};
use crate::worker::{self, BatchSpec, FileStatus, RemuxEngine, WorkerControl, WorkerEvent};
use eframe::CreationContext;
use eframe::egui;
use eframe::egui::{
    Align, Button, Color32, ComboBox, CornerRadius, FontFamily, FontId, Frame, Key, Label, Layout,
    Margin, Panel, ProgressBar, RichText, ScrollArea, Sense, Stroke, TextEdit,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const PREFERENCES_KEY: &str = "langmux.preferences";

#[derive(Clone, Serialize, Deserialize)]
struct Preferences {
    input_directory: String,
    output_directory: String,
    audio_languages: String,
    subtitle_languages: String,
    #[serde(default)]
    engine: RemuxEngine,
    skip_existing: bool,
    dry_run: bool,
    logs_open: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            input_directory: String::new(),
            output_directory: String::new(),
            audio_languages: "eng, jpn".to_owned(),
            subtitle_languages: "eng".to_owned(),
            engine: RemuxEngine::MkvToolNix,
            skip_existing: true,
            dry_run: false,
            logs_open: false,
        }
    }
}

#[derive(Clone, Default)]
struct FileProgress {
    status: FileStatus,
    percent: u32,
    detail: Option<String>,
}

pub struct LangMuxApp {
    preferences: Preferences,
    logo: Option<egui::TextureHandle>,
    files: Vec<FileInfo>,
    progress: Vec<FileProgress>,
    stats: BatchStats,
    languages: Option<AvailableLanguages>,
    scan_loading: bool,
    tools: Toolchain,
    sender: mpsc::Sender<WorkerEvent>,
    receiver: mpsc::Receiver<WorkerEvent>,
    control: WorkerControl,
    is_running: bool,
    is_paused: bool,
    batch_has_finished: bool,
    batch_started: Option<Instant>,
    elapsed_seconds: f64,
    filter: String,
    logs: Vec<String>,
    drag_hovered: bool,
}

impl LangMuxApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let logo = image::load_from_memory(include_bytes!("../../logo.png"))
            .ok()
            .map(|image| {
                let rgba = image.into_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                cc.egui_ctx.load_texture(
                    "langmux-sidebar-logo",
                    egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                    egui::TextureOptions::LINEAR,
                )
            });
        let mut preferences: Preferences = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, PREFERENCES_KEY))
            .unwrap_or_default();
        let (sender, receiver) = mpsc::channel();
        let tools = Toolchain::discover();
        if tools.mkvmerge.is_none() {
            preferences.engine = RemuxEngine::Ffmpeg;
        }
        let mut app = Self {
            preferences,
            logo,
            files: Vec::new(),
            progress: Vec::new(),
            stats: BatchStats::default(),
            languages: None,
            scan_loading: false,
            tools,
            sender,
            receiver,
            control: WorkerControl::default(),
            is_running: false,
            is_paused: false,
            batch_has_finished: false,
            batch_started: None,
            elapsed_seconds: 0.0,
            filter: String::new(),
            logs: Vec::new(),
            drag_hovered: false,
        };
        if app.tools.mkvmerge.is_some() {
            app.push_log("MKVToolNix is ready");
        } else if app.tools.ffmpeg.is_some() {
            app.push_log("MKVToolNix was not found; FFmpeg is selected");
        } else {
            app.push_log("No remux engine was found");
        }
        let input = PathBuf::from(&app.preferences.input_directory);
        if input.is_dir() {
            app.scan_input(input, cc.egui_ctx.clone());
        }
        app
    }

    fn push_log(&mut self, message: impl AsRef<str>) {
        let timestamp = chrono_time();
        self.logs
            .insert(0, format!("[{timestamp}] {}", message.as_ref()));
        self.logs.truncate(500);
    }

    fn scan_input(&mut self, directory: PathBuf, ctx: egui::Context) {
        if self.is_running {
            return;
        }
        let previous_input = PathBuf::from(&self.preferences.input_directory);
        let previous_default = mkv::default_output_dir(&previous_input);
        let use_default = self.preferences.output_directory.is_empty()
            || Path::new(&self.preferences.output_directory) == previous_default.as_path();
        self.preferences.input_directory = directory.to_string_lossy().into_owned();
        if use_default {
            self.preferences.output_directory = mkv::default_output_dir(&directory)
                .to_string_lossy()
                .into_owned();
        }
        self.files.clear();
        self.progress.clear();
        self.stats = BatchStats::default();
        self.elapsed_seconds = 0.0;
        self.batch_has_finished = false;
        self.languages = None;
        self.scan_loading = true;
        self.push_log(format!("Scanning {}", directory.display()));
        worker::scan_directory(directory, self.tools.clone(), self.sender.clone(), ctx);
    }

    fn browse_input(&mut self, ctx: &egui::Context) {
        let dialog = rfd::FileDialog::new();
        let dialog = if Path::new(&self.preferences.input_directory).is_dir() {
            dialog.set_directory(&self.preferences.input_directory)
        } else {
            dialog
        };
        if let Some(directory) = dialog.pick_folder() {
            self.scan_input(directory, ctx.clone());
        }
    }

    fn browse_output(&mut self) {
        let dialog = rfd::FileDialog::new();
        let dialog = if Path::new(&self.preferences.output_directory).is_dir() {
            dialog.set_directory(&self.preferences.output_directory)
        } else {
            dialog
        };
        if let Some(directory) = dialog.pick_folder() {
            self.preferences.output_directory = directory.to_string_lossy().into_owned();
            self.push_log(format!("Output folder set to {}", directory.display()));
        }
    }

    fn accept_drop(&mut self, path: PathBuf, ctx: &egui::Context) {
        let directory = if path.is_dir() {
            Some(path)
        } else if mkv::is_mkv_path(&path) {
            path.parent().map(Path::to_path_buf)
        } else {
            None
        };
        if let Some(directory) = directory {
            self.scan_input(directory, ctx.clone());
        }
    }

    fn start_batch(&mut self, ctx: &egui::Context) {
        if self.is_running {
            return;
        }
        if self.files.is_empty() {
            self.push_log("Choose a folder with MKV files before starting");
            return;
        }
        if let Some(error) = self.selected_engine_error() {
            self.push_log(error);
            return;
        };
        let audio_languages = mkv::parse_language_field(&self.preferences.audio_languages);
        if audio_languages.is_empty() {
            self.push_log("At least one audio language is required");
            return;
        }
        let subtitle_languages = mkv::parse_language_field(&self.preferences.subtitle_languages);
        let output_directory = if self.preferences.output_directory.trim().is_empty() {
            let output = mkv::default_output_dir(Path::new(&self.preferences.input_directory));
            self.preferences.output_directory = output.to_string_lossy().into_owned();
            output
        } else {
            PathBuf::from(&self.preferences.output_directory)
        };

        self.progress = vec![FileProgress::default(); self.files.len()];
        self.elapsed_seconds = 0.0;
        self.batch_has_finished = false;
        self.batch_started = Some(Instant::now());
        self.is_running = true;
        self.is_paused = false;
        worker::start_batch(
            BatchSpec {
                files: self.files.clone(),
                tools: self.tools.clone(),
                engine: self.preferences.engine,
                output_directory,
                audio_languages,
                subtitle_languages,
                skip_existing: self.preferences.skip_existing,
                dry_run: self.preferences.dry_run,
            },
            self.control.clone(),
            self.sender.clone(),
            ctx.clone(),
        );
    }

    fn selected_engine_error(&self) -> Option<String> {
        match self.preferences.engine {
            RemuxEngine::MkvToolNix if self.tools.mkvmerge.is_none() => {
                Some("MKVToolNix is unavailable; choose the FFmpeg engine".to_owned())
            }
            RemuxEngine::Ffmpeg if self.tools.ffmpeg.is_none() => {
                Some("FFmpeg is unavailable; install it or add it to PATH".to_owned())
            }
            RemuxEngine::Ffmpeg if self.tools.ffprobe.is_none() => {
                Some("FFprobe is unavailable; install it or add it to PATH".to_owned())
            }
            _ => None,
        }
    }

    fn stop_batch(&mut self) {
        if self.is_running {
            self.control.stop();
            self.is_paused = false;
            self.push_log("Stopping the active file and removing partial output");
        }
    }

    fn toggle_pause(&mut self) {
        if self.is_running {
            self.is_paused = !self.is_paused;
            self.control.set_paused(self.is_paused);
            self.push_log(if self.is_paused {
                "Paused after the active file"
            } else {
                "Batch resumed"
            });
        }
    }

    fn estimated_remaining_seconds(&self) -> Option<f64> {
        if self.elapsed_seconds < 1.0 {
            return None;
        }

        let total_size: f64 = self.files.iter().map(|file| file.size as f64).sum();
        if total_size <= 0.0 {
            return None;
        }

        let completed_size: f64 = self
            .files
            .iter()
            .zip(&self.progress)
            .map(|(file, progress)| {
                let fraction = match progress.status {
                    FileStatus::Success | FileStatus::Skipped | FileStatus::Error => 1.0,
                    FileStatus::Processing => progress.percent as f64 / 100.0,
                    FileStatus::Pending | FileStatus::Cancelled => 0.0,
                };
                file.size as f64 * fraction
            })
            .sum();
        let completed_fraction = completed_size / total_size;
        if !(0.05..1.0).contains(&completed_fraction) {
            return None;
        }

        let remaining = self.elapsed_seconds * (1.0 - completed_fraction) / completed_fraction;
        remaining.is_finite().then_some(remaining)
    }

    fn toggle_language(&mut self, audio: bool, language: &str) {
        let value = if audio {
            &mut self.preferences.audio_languages
        } else {
            &mut self.preferences.subtitle_languages
        };
        let mut selected = mkv::parse_language_field(value);
        if selected.iter().any(|item| item == language) {
            selected.retain(|item| item != language);
        } else {
            selected.push(language.to_owned());
            selected.sort();
        }
        *value = selected.join(", ");
    }

    fn receive_events(&mut self) {
        let events: Vec<WorkerEvent> = self.receiver.try_iter().collect();
        for event in events {
            match event {
                WorkerEvent::ScanFiles {
                    directory,
                    files,
                    stats,
                } if directory.as_path() == Path::new(&self.preferences.input_directory) => {
                    self.progress = vec![FileProgress::default(); files.len()];
                    self.files = files;
                    self.stats = stats;
                    self.push_log(format!("Found {} MKV file(s)", self.files.len()));
                }
                WorkerEvent::ScanLanguages {
                    directory,
                    languages,
                    metadata,
                    error,
                } if directory.as_path() == Path::new(&self.preferences.input_directory) => {
                    for (path, metadata) in metadata {
                        if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
                            file.metadata = Some(metadata);
                        }
                    }
                    self.languages = Some(languages);
                    self.scan_loading = false;
                    if let Some(error) = error {
                        self.push_log(format!("Some metadata could not be read: {error}"));
                    }
                }
                WorkerEvent::ScanFailed { directory, error }
                    if directory.as_path() == Path::new(&self.preferences.input_directory) =>
                {
                    self.scan_loading = false;
                    self.push_log(format!("Folder scan failed: {error}"));
                }
                WorkerEvent::FileUpdated {
                    index,
                    status,
                    percent,
                    detail,
                } => {
                    if let Some(progress) = self.progress.get_mut(index) {
                        progress.status = status;
                        progress.percent = percent.min(100);
                        progress.detail = detail;
                    }
                }
                WorkerEvent::Log(message) => self.push_log(message),
                WorkerEvent::BatchFinished {
                    cancelled,
                    elapsed_seconds,
                } => {
                    self.is_running = false;
                    self.is_paused = false;
                    self.elapsed_seconds = elapsed_seconds;
                    self.batch_has_finished = true;
                    self.batch_started = None;
                    self.push_log(if cancelled {
                        "Batch stopped"
                    } else {
                        "Batch finished"
                    });
                }
                _ => {}
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let wants_keyboard_input = ctx.egui_wants_keyboard_input();
        let space = !wants_keyboard_input
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Space));
        let stop = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Escape));
        let browse = ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, Key::O));

        if space {
            if self.is_running {
                self.toggle_pause();
            } else {
                self.start_batch(ctx);
            }
        }
        if stop {
            self.stop_batch();
        }
        if browse && !self.is_running {
            self.browse_input(ctx);
        }
    }

    fn draw_sidebar(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        Panel::left("settings_sidebar")
            .resizable(true)
            .default_size(theme::SIDEBAR_WIDTH)
            .size_range(286.0..=400.0)
            .frame(
                Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .inner_margin(Margin::same(18)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.add_sized(
                            [34.0, 34.0],
                            egui::Image::new((logo.id(), egui::vec2(34.0, 34.0)))
                                .corner_radius(CornerRadius::same(theme::CORNER)),
                        );
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new("LangMux").size(17.0).strong());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("LANGUAGE REMUXER")
                                    .size(9.5)
                                    .color(theme::MUTED),
                            );
                            ui.label(RichText::new("v2.0.0").size(9.5).color(theme::SUBTLE));
                        });
                    });
                });
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(10.0);

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.draw_folder_picker(ui, ctx, true);
                        ui.add_space(14.0);
                        self.draw_folder_picker(ui, ctx, false);
                        ui.add_space(20.0);
                        ui.separator();
                        ui.add_space(14.0);
                        self.draw_language_picker(ui, true);
                        ui.add_space(14.0);
                        self.draw_language_picker(ui, false);
                        ui.add_space(18.0);
                        ui.separator();
                        ui.add_space(12.0);
                        self.draw_processing_options(ui);
                        ui.add_space(18.0);
                        self.draw_batch_stats(ui);
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(10.0);
                        tool_status_row(ui, "mkvmerge", self.tools.mkvmerge.as_ref());
                        tool_status_row(ui, "ffmpeg", self.tools.ffmpeg.as_ref());
                        tool_status_row(ui, "ffprobe", self.tools.ffprobe.as_ref());
                    });
            });
    }

    fn draw_folder_picker(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, input: bool) {
        let label = if input {
            "INPUT FOLDER"
        } else {
            "OUTPUT FOLDER"
        };
        ui.label(RichText::new(label).size(10.0).strong().color(theme::MUTED));
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            let button_width = 68.0;
            let path_width =
                (ui.available_width() - button_width - ui.spacing().item_spacing.x).max(84.0);
            let path = if input {
                &self.preferences.input_directory
            } else {
                &self.preferences.output_directory
            };
            let shown = if path.is_empty() {
                if input {
                    "Choose a source folder"
                } else {
                    "Defaults to input/remuxed"
                }
            } else {
                path.as_str()
            };
            let path_field = Frame::new()
                .fill(theme::RAISED)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(8, 0))
                .show(ui, |ui| {
                    ui.set_width(path_width - 16.0);
                    ui.set_min_height(34.0);
                    let available_width = ui.available_width();
                    let display = middle_truncate(ui, shown, available_width);
                    ui.add_sized(
                        [ui.available_width(), 34.0],
                        Label::new(RichText::new(display).monospace().size(10.5).color(
                            if path.is_empty() {
                                theme::SUBTLE
                            } else {
                                theme::TEXT
                            },
                        ))
                        .truncate(),
                    );
                });
            path_field.response.on_hover_text(shown);
            let clicked = ui.add_sized(
                [button_width, 34.0],
                Button::new(if input { "Browse" } else { "Choose" })
                    .fill(theme::SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(CornerRadius::same(6)),
            );
            if clicked.clicked() && !self.is_running {
                if input {
                    self.browse_input(ctx);
                } else {
                    self.browse_output();
                }
            }
        });
    }

    fn draw_language_picker(&mut self, ui: &mut egui::Ui, audio: bool) {
        let title = if audio {
            "AUDIO LANGUAGES"
        } else {
            "SUBTITLE LANGUAGES"
        };
        let selected = if audio {
            &mut self.preferences.audio_languages
        } else {
            &mut self.preferences.subtitle_languages
        };
        ui.label(RichText::new(title).size(10.0).strong().color(theme::MUTED));
        ui.add_space(5.0);
        let field_id = if audio {
            "audio-language-input"
        } else {
            "subtitle-language-input"
        };
        ui.add_enabled_ui(!self.is_running, |ui| {
            let width = ui.available_width();
            styled_text_input(
                ui,
                selected,
                if audio { "eng, jpn" } else { "eng" },
                width,
                field_id,
            );
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(if audio {
                "ISO 639-2 codes · case-insensitive"
            } else {
                "Leave blank to remove all subtitles"
            })
            .size(10.0)
            .color(theme::SUBTLE),
        );

        if let Some(languages) = &self.languages {
            let choices = if audio {
                &languages.audio
            } else {
                &languages.subtitles
            };
            let choices = choices.clone();
            if !choices.is_empty() {
                ui.add_space(7.0);
                ui.horizontal_wrapped(|ui| {
                    for language in &choices {
                        let is_selected = mkv::parse_language_field(if audio {
                            &self.preferences.audio_languages
                        } else {
                            &self.preferences.subtitle_languages
                        })
                        .iter()
                        .any(|item| item == language);
                        let response = ui.add_enabled(
                            !self.is_running,
                            Button::new(RichText::new(language).monospace().size(10.0))
                                .fill(if is_selected {
                                    theme::ACCENT.gamma_multiply(0.22)
                                } else {
                                    theme::RAISED
                                })
                                .stroke(Stroke::new(
                                    1.0,
                                    if is_selected {
                                        theme::ACCENT.gamma_multiply(0.65)
                                    } else {
                                        theme::BORDER
                                    },
                                ))
                                .corner_radius(CornerRadius::same(5)),
                        );
                        if response.clicked() {
                            self.toggle_language(audio, language);
                        }
                    }
                });
            }
        } else if self.scan_loading {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Scanning track languages…")
                    .size(10.0)
                    .color(theme::SUBTLE),
            );
        }
    }

    fn draw_processing_options(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("PROCESSING")
                .size(10.0)
                .strong()
                .color(theme::MUTED),
        );
        ui.add_space(6.0);
        ui.add_enabled_ui(!self.is_running, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Engine").size(11.0).color(theme::MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ComboBox::from_id_salt("remux-engine")
                            .width(132.0)
                            .selected_text(self.preferences.engine.label())
                            .show_ui(ui, |ui| {
                                ui.add_enabled_ui(self.tools.mkvmerge.is_some(), |ui| {
                                    ui.selectable_value(
                                        &mut self.preferences.engine,
                                        RemuxEngine::MkvToolNix,
                                        "MKVToolNix",
                                    );
                                });
                                ui.add_enabled_ui(
                                    self.tools.ffmpeg.is_some() && self.tools.ffprobe.is_some(),
                                    |ui| {
                                        ui.selectable_value(
                                            &mut self.preferences.engine,
                                            RemuxEngine::Ffmpeg,
                                            "FFmpeg",
                                        );
                                    },
                                );
                            });
                    });
                });
                ui.add_space(5.0);
                draw_switch(
                    ui,
                    "Skip existing outputs",
                    &mut self.preferences.skip_existing,
                );
                ui.add_space(2.0);
                draw_switch(ui, "Dry run — preview only", &mut self.preferences.dry_run);
            });
        });
    }

    fn draw_batch_stats(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("BATCH SUMMARY")
                .size(10.0)
                .strong()
                .color(theme::MUTED),
        );
        ui.add_space(8.0);
        Frame::new()
            .fill(theme::INSET)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(CornerRadius::same(theme::CORNER))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    summary_stat(ui, "FILES", &self.stats.total_files.to_string());
                    ui.add_space(18.0);
                    summary_stat(ui, "TOTAL SIZE", &format_size(self.stats.total_size));
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let completed = self
                        .progress
                        .iter()
                        .filter(|item| {
                            matches!(item.status, FileStatus::Success | FileStatus::Skipped)
                        })
                        .count();
                    summary_stat(
                        ui,
                        "COMPLETED",
                        &format!("{completed}/{}", self.stats.total_files),
                    );
                    ui.add_space(18.0);
                    summary_stat(
                        ui,
                        if self.is_running {
                            "ESTIMATE"
                        } else {
                            "ELAPSED"
                        },
                        &if self.is_running {
                            self.estimated_remaining_seconds()
                                .map(format_duration)
                                .unwrap_or_else(|| "—".to_owned())
                        } else if self.batch_has_finished {
                            format_duration(self.elapsed_seconds)
                        } else {
                            "—".to_owned()
                        },
                    );
                });
            });
    }

    fn draw_topbar(&mut self, root: &mut egui::Ui) {
        Panel::top("top_bar")
            .exact_size(64.0)
            .frame(
                Frame::new()
                    .fill(theme::BACKGROUND)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .inner_margin(Margin::symmetric(22, 10)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Remux queue")
                                .size(18.0)
                                .strong()
                                .color(theme::TEXT),
                        );
                        let caption = if self.preferences.input_directory.is_empty() {
                            "Choose a folder to load MKV files".to_owned()
                        } else {
                            format!(
                                "{}  ·  {} file{}",
                                self.preferences.input_directory,
                                self.files.len(),
                                if self.files.len() == 1 { "" } else { "s" }
                            )
                        };
                        ui.label(RichText::new(caption).size(10.5).color(theme::MUTED));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let log_label = if self.preferences.logs_open {
                            "Hide logs"
                        } else {
                            "Logs"
                        };
                        if ui
                            .add_sized(
                                [68.0, 34.0],
                                Button::new(log_label)
                                    .fill(theme::SURFACE)
                                    .stroke(Stroke::new(1.0, theme::BORDER))
                                    .corner_radius(CornerRadius::same(6)),
                            )
                            .clicked()
                        {
                            self.preferences.logs_open = !self.preferences.logs_open;
                        }
                        styled_text_input(
                            ui,
                            &mut self.filter,
                            "Filter files",
                            230.0,
                            "filter-files",
                        );
                    });
                });
            });
    }

    fn draw_action_bar(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        Panel::bottom("action_bar")
            .exact_size(78.0)
            .frame(
                Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .inner_margin(Margin::symmetric(22, 12)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    let total = self.stats.total_files;
                    let done = self
                        .progress
                        .iter()
                        .filter(|item| {
                            matches!(item.status, FileStatus::Success | FileStatus::Skipped)
                        })
                        .count();
                    let current = self
                        .progress
                        .iter()
                        .position(|item| item.status == FileStatus::Processing);
                    let fraction = if total == 0 {
                        0.0
                    } else {
                        let active_fraction = current
                            .and_then(|index| self.progress.get(index))
                            .map(|item| item.percent as f32 / 100.0)
                            .unwrap_or(0.0);
                        ((done as f32 + active_fraction) / total as f32).clamp(0.0, 1.0)
                    };
                    let controls_width = if self.is_running { 224.0 } else { 166.0 };
                    let progress_width = (ui.available_width() - controls_width - 16.0).max(240.0);
                    ui.vertical(|ui| {
                        ui.set_width(progress_width);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("BATCH PROGRESS")
                                    .size(9.5)
                                    .strong()
                                    .color(theme::MUTED),
                            );
                            ui.label(
                                RichText::new(format!("{done} / {total}"))
                                    .monospace()
                                    .size(10.0)
                                    .color(theme::TEXT),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format_duration(self.elapsed_seconds))
                                    .monospace()
                                    .size(10.0)
                                    .color(theme::SUBTLE),
                            );
                        });
                        ui.add(
                            ProgressBar::new(fraction)
                                .desired_width(ui.available_width())
                                .desired_height(5.0)
                                .fill(theme::ACCENT)
                                .corner_radius(CornerRadius::same(3)),
                        );
                    });
                    ui.add_space(16.0);
                    if self.is_running {
                        if ui
                            .add_sized(
                                [92.0, 38.0],
                                Button::new(if self.is_paused { "Resume" } else { "Pause" })
                                    .fill(theme::SURFACE)
                                    .stroke(Stroke::new(1.0, theme::BORDER))
                                    .corner_radius(CornerRadius::same(6)),
                            )
                            .clicked()
                        {
                            self.toggle_pause();
                        }
                        if ui
                            .add_sized(
                                [92.0, 38.0],
                                Button::new(RichText::new("Stop").color(theme::ERROR))
                                    .fill(theme::ERROR.gamma_multiply(0.12))
                                    .stroke(Stroke::new(1.0, theme::ERROR.gamma_multiply(0.35)))
                                    .corner_radius(CornerRadius::same(6)),
                            )
                            .clicked()
                        {
                            self.stop_batch();
                        }
                    } else if ui
                        .add_sized(
                            [150.0, 40.0],
                            Button::new(
                                RichText::new(if self.preferences.dry_run {
                                    "Preview batch"
                                } else {
                                    "Start remux"
                                })
                                .strong()
                                .color(Color32::WHITE),
                            )
                            .fill(theme::ACCENT)
                            .corner_radius(CornerRadius::same(7)),
                        )
                        .clicked()
                    {
                        self.start_batch(ctx);
                    }
                });
            });
    }

    fn draw_logs(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        if !self.preferences.logs_open {
            return;
        }
        Panel::bottom("log_panel")
            .exact_size(204.0)
            .frame(
                Frame::new()
                    .fill(theme::INSET)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .inner_margin(Margin::symmetric(18, 10)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("RUN LOG")
                            .size(10.0)
                            .strong()
                            .color(theme::MUTED),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(Button::new("Export").corner_radius(CornerRadius::same(5)))
                            .clicked()
                        {
                            self.export_logs();
                        }
                        if ui
                            .add(Button::new("Copy").corner_radius(CornerRadius::same(5)))
                            .clicked()
                        {
                            ctx.copy_text(self.logs.join("\n"));
                        }
                        if ui
                            .add(Button::new("Clear").corner_radius(CornerRadius::same(5)))
                            .clicked()
                        {
                            self.logs.clear();
                        }
                    });
                });
                ui.add_space(6.0);
                ui.separator();
                ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.logs.is_empty() {
                            ui.label(
                                RichText::new("No activity yet")
                                    .monospace()
                                    .size(11.0)
                                    .color(theme::SUBTLE),
                            );
                        }
                        for line in self.logs.iter().rev() {
                            ui.label(
                                RichText::new(line)
                                    .monospace()
                                    .size(10.5)
                                    .color(theme::MUTED),
                            );
                        }
                    });
            });
    }

    fn export_logs(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("langmux.log")
            .save_file()
        {
            match std::fs::write(&path, self.logs.join("\n")) {
                Ok(()) => self.push_log(format!("Logs exported to {}", path.display())),
                Err(error) => self.push_log(format!("Log export failed: {error}")),
            }
        }
    }

    fn draw_file_list(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = self.selected_engine_error() {
            Frame::new()
                .fill(theme::ERROR.gamma_multiply(0.08))
                .stroke(Stroke::new(1.0, theme::ERROR.gamma_multiply(0.36)))
                .corner_radius(CornerRadius::same(7))
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("!").strong().color(theme::ERROR));
                        ui.label(RichText::new(error).color(theme::TEXT));
                    });
                });
            ui.add_space(16.0);
        }

        let query = self.filter.to_lowercase();
        let visible: Vec<usize> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| {
                query.is_empty()
                    || file.name.to_lowercase().contains(&query)
                    || file.path.to_string_lossy().to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect();

        if self.files.is_empty() {
            let available = ui.available_rect_before_wrap();
            let card_rect =
                egui::Rect::from_center_size(available.center(), egui::vec2(520.0, 280.0));
            let mut card_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(card_rect)
                    .layout(Layout::top_down(Align::Center)),
            );
            Frame::new()
                .fill(theme::SURFACE)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(CornerRadius::same(theme::CORNER))
                .inner_margin(Margin::same(28))
                .show(&mut card_ui, |ui| {
                    ui.set_width(440.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("Drop a folder here")
                                .size(21.0)
                                .strong()
                                .color(theme::TEXT),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(
                                "Load a folder of MKV files to select tracks and build a batch.",
                            )
                            .size(12.0)
                            .color(theme::MUTED),
                        );
                        ui.add_space(18.0);
                        if ui
                            .add_sized(
                                [176.0, 38.0],
                                Button::new("Choose input folder")
                                    .fill(theme::ACCENT)
                                    .corner_radius(CornerRadius::same(7)),
                            )
                            .clicked()
                        {
                            self.browse_input(ui.ctx());
                        }
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("or drag a folder onto this window")
                                .size(10.5)
                                .color(theme::SUBTLE),
                        );
                        ui.add_space(12.0);
                    });
                });
            return;
        }

        if self.scan_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Reading available language tracks…")
                        .size(11.0)
                        .color(theme::MUTED),
                );
            });
            ui.add_space(8.0);
        }

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available_width = ui.available_width();
                let columns = draw_file_header(ui, available_width);
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);
                for index in visible {
                    self.draw_file_row(ui, index, columns);
                }
                if visible_is_empty(&self.files, &query) {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No files match this filter").color(theme::MUTED));
                    });
                }
            });
    }

    fn draw_file_row(&self, ui: &mut egui::Ui, index: usize, columns: FileColumns) {
        let Some(file) = self.files.get(index) else {
            return;
        };
        let progress = self.progress.get(index).cloned().unwrap_or_default();
        let selected_audio = mkv::parse_language_field(&self.preferences.audio_languages);
        let selected_subtitles = mkv::parse_language_field(&self.preferences.subtitle_languages);
        let (audio_count, subtitle_count, audio_codes, subtitle_codes) = file
            .metadata
            .as_ref()
            .map_or((0, 0, Vec::new(), Vec::new()), |metadata| {
                let (audio, subtitles) =
                    mkv::selected_tracks(metadata, &selected_audio, &selected_subtitles);
                (
                    audio.len(),
                    subtitles.len(),
                    track_languages(&metadata.audio_tracks, &audio),
                    track_languages(&metadata.subtitle_tracks, &subtitles),
                )
            });

        let show_progress = progress.status == FileStatus::Processing;
        let detail_color = match progress.status {
            FileStatus::Error => Some(theme::ERROR),
            FileStatus::Cancelled => Some(theme::WARNING),
            _ => None,
        };
        let show_detail = progress.detail.is_some() && detail_color.is_some();
        let row_height = 2.0 * FILE_ROW_PADDING
            + FILE_ROW_CONTENT_HEIGHT
            + if show_progress { 10.0 } else { 0.0 }
            + if show_detail { 17.0 } else { 0.0 };
        let (allocated_row, _) =
            ui.allocate_exact_size(egui::vec2(columns.table_width, row_height), Sense::hover());
        let row_rect = columns.table_rect(allocated_row.top(), row_height);
        ui.painter()
            .rect_filled(row_rect, CornerRadius::same(7), theme::SURFACE);
        ui.painter().rect_stroke(
            row_rect,
            CornerRadius::same(7),
            Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Inside,
        );

        let content_top = row_rect.top() + FILE_ROW_PADDING;
        let file_rect = columns.file.rect(content_top, 22.0);
        let name = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(file_rect)
                    .layout(Layout::left_to_right(Align::Center)),
                |cell| {
                    cell.add(
                        Label::new(
                            RichText::new(&file.name)
                                .size(12.0)
                                .strong()
                                .color(theme::TEXT),
                        )
                        .truncate(),
                    )
                },
            )
            .inner;
        name.on_hover_text(&file.name);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(columns.file.rect(content_top + 22.0, 18.0))
                .layout(Layout::left_to_right(Align::Center)),
            |cell| {
                cell.add(
                    Label::new(
                        RichText::new(format!(
                            "{} audio · {} subtitle{} matched",
                            audio_count,
                            subtitle_count,
                            if subtitle_count == 1 { "" } else { "s" }
                        ))
                        .size(10.0)
                        .color(theme::MUTED),
                    )
                    .truncate(),
                );
            },
        );

        let tracks_rect = columns.tracks.rect(content_top, FILE_ROW_CONTENT_HEIGHT);
        draw_track_line(
            ui,
            egui::Rect::from_min_size(tracks_rect.min, egui::vec2(tracks_rect.width(), 20.0)),
            "AUDIO",
            &audio_codes,
        );
        draw_track_line(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(tracks_rect.left(), content_top + 22.0),
                egui::vec2(tracks_rect.width(), 20.0),
            ),
            "SUBS",
            &subtitle_codes,
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(columns.size.rect(content_top, FILE_ROW_CONTENT_HEIGHT))
                .layout(Layout::right_to_left(Align::Center)),
            |cell| {
                cell.add(
                    Label::new(
                        RichText::new(format_size(file.size))
                            .monospace()
                            .size(10.5)
                            .color(theme::MUTED),
                    )
                    .truncate(),
                );
            },
        );
        status_pill(
            ui,
            columns.status.rect(content_top + 10.0, 24.0),
            progress.status,
            progress.detail.as_deref(),
        );

        let mut extra_top = content_top + FILE_ROW_CONTENT_HEIGHT;
        if show_progress {
            let progress_rect = columns
                .table_rect(extra_top + 4.0, 3.0)
                .shrink2(egui::vec2(TABLE_HORIZONTAL_PADDING, 0.0));
            ui.put(
                progress_rect,
                ProgressBar::new(progress.percent as f32 / 100.0)
                    .desired_width(progress_rect.width())
                    .desired_height(3.0)
                    .fill(theme::ACCENT)
                    .corner_radius(CornerRadius::same(2)),
            );
            extra_top += 10.0;
        }
        if show_detail
            && let (Some(detail), Some(color)) = (progress.detail.as_deref(), detail_color)
        {
            ui.put(
                columns
                    .table_rect(extra_top + 2.0, 14.0)
                    .shrink2(egui::vec2(TABLE_HORIZONTAL_PADDING, 0.0)),
                Label::new(RichText::new(detail).size(10.0).color(color)),
            );
        }
        ui.add_space(6.0);
    }

    fn update_worker_events(&mut self, ctx: &egui::Context) {
        self.receive_events();
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        self.drag_hovered = ctx.input(|input| !input.raw.hovered_files.is_empty());
        if let Some(path) = dropped.into_iter().next() {
            self.accept_drop(path, ctx);
        }
    }
}

impl eframe::App for LangMuxApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.update_worker_events(&ctx);
        self.handle_shortcuts(&ctx);
        if let Some(started) = self.batch_started {
            self.elapsed_seconds = started.elapsed().as_secs_f64();
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        self.draw_sidebar(root, &ctx);
        self.draw_topbar(root);
        self.draw_action_bar(root, &ctx);
        self.draw_logs(root, &ctx);
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(theme::BACKGROUND)
                    .inner_margin(Margin::same(22)),
            )
            .show(root, |ui| {
                self.draw_file_list(ui);
                if self.drag_hovered {
                    let rect = ui.clip_rect();
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(8),
                        theme::ACCENT.gamma_multiply(0.16),
                    );
                    ui.painter().rect_stroke(
                        rect.shrink(2.0),
                        CornerRadius::same(8),
                        Stroke::new(2.0, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
            });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PREFERENCES_KEY, &self.preferences);
    }
}

fn summary_stat(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).size(9.0).strong().color(theme::SUBTLE));
        ui.label(
            RichText::new(value)
                .monospace()
                .size(11.0)
                .color(theme::TEXT),
        );
    });
}

fn tool_status_row(ui: &mut egui::Ui, name: &str, tool: Option<&Tool>) {
    ui.horizontal(|ui| {
        let found = tool.is_some();
        ui.label(
            RichText::new("●")
                .color(if found { theme::SUCCESS } else { theme::ERROR })
                .size(10.0),
        );
        let (label, tooltip) = match tool {
            Some(tool) => (
                format!(
                    "{name} · {}",
                    tool.version.as_deref().unwrap_or("version unavailable")
                ),
                tool.path.display().to_string(),
            ),
            None => (
                format!("{name} unavailable"),
                "Not found in the application bundle, PATH, or platform defaults".to_owned(),
            ),
        };
        ui.add(
            Label::new(
                RichText::new(label)
                    .monospace()
                    .size(9.0)
                    .color(theme::MUTED),
            )
            .truncate(),
        )
        .on_hover_text(tooltip);
    });
}

const TABLE_COLUMN_GAP: f32 = 10.0;
const TABLE_HORIZONTAL_PADDING: f32 = 12.0;
const FILE_COLUMN_MIN_WIDTH: f32 = 120.0;
const TRACKS_COLUMN_WIDTH: f32 = 240.0;
const SIZE_COLUMN_WIDTH: f32 = 80.0;
const STATUS_COLUMN_WIDTH: f32 = 96.0;
const FILE_ROW_CONTENT_HEIGHT: f32 = 44.0;
const FILE_ROW_PADDING: f32 = 7.0;

#[derive(Clone, Copy)]
struct ColumnSpan {
    left: f32,
    right: f32,
}

impl ColumnSpan {
    fn rect(self, top: f32, height: f32) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(self.left, top),
            egui::pos2(self.right, top + height),
        )
    }
}

#[derive(Clone, Copy)]
struct FileColumns {
    table_left: f32,
    table_width: f32,
    file: ColumnSpan,
    tracks: ColumnSpan,
    size: ColumnSpan,
    status: ColumnSpan,
}

impl FileColumns {
    fn for_rect(table_rect: egui::Rect) -> Self {
        let left = table_rect.left() + TABLE_HORIZONTAL_PADDING;
        let right = table_rect.right() - TABLE_HORIZONTAL_PADDING;
        let column_width = (right - left - TABLE_COLUMN_GAP * 3.0).max(0.0);
        let status_width = STATUS_COLUMN_WIDTH.min(column_width);
        let size_width = SIZE_COLUMN_WIDTH.min((column_width - status_width).max(0.0));
        let tracks_width = TRACKS_COLUMN_WIDTH
            .min((column_width - status_width - size_width - FILE_COLUMN_MIN_WIDTH).max(0.0));

        let status_left = right - status_width;
        let size_right = status_left - TABLE_COLUMN_GAP;
        let size_left = size_right - size_width;
        let tracks_right = size_left - TABLE_COLUMN_GAP;
        let tracks_left = tracks_right - tracks_width;
        let file_right = tracks_left - TABLE_COLUMN_GAP;

        Self {
            table_left: table_rect.left(),
            table_width: table_rect.width(),
            file: ColumnSpan {
                left,
                right: file_right.max(left),
            },
            tracks: ColumnSpan {
                left: tracks_left,
                right: tracks_right,
            },
            size: ColumnSpan {
                left: size_left,
                right: size_right,
            },
            status: ColumnSpan {
                left: status_left,
                right,
            },
        }
    }

    fn table_rect(self, top: f32, height: f32) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(self.table_left, top),
            egui::vec2(self.table_width, height),
        )
    }
}

fn draw_file_header(ui: &mut egui::Ui, table_width: f32) -> FileColumns {
    let (header_rect, _) = ui.allocate_exact_size(egui::vec2(table_width, 24.0), Sense::hover());
    let columns = FileColumns::for_rect(header_rect);
    let top = header_rect.top() + 4.0;
    table_header_label(ui, columns.file.rect(top, 16.0), "FILE", Align::Min);
    table_header_label(ui, columns.tracks.rect(top, 16.0), "TRACKS", Align::Min);
    table_header_label(ui, columns.size.rect(top, 16.0), "SIZE", Align::Max);
    table_header_label(ui, columns.status.rect(top, 16.0), "STATUS", Align::Min);
    columns
}

fn table_header_label(ui: &mut egui::Ui, rect: egui::Rect, label: &str, align: Align) {
    let layout = if align == Align::Max {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    };
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect).layout(layout),
        |cell| {
            cell.add(
                Label::new(RichText::new(label).size(9.5).strong().color(theme::SUBTLE)).truncate(),
            );
        },
    );
}

fn track_languages(tracks: &[mkv::TrackInfo], selected_ids: &[u64]) -> Vec<String> {
    let mut languages = Vec::new();
    for track in tracks
        .iter()
        .filter(|track| selected_ids.contains(&track.id))
    {
        let language = track.language.to_ascii_lowercase();
        if !languages.contains(&language) {
            languages.push(language);
        }
    }
    languages
}

fn draw_track_line(ui: &mut egui::Ui, rect: egui::Rect, label: &str, languages: &[String]) {
    ui.put(
        egui::Rect::from_min_size(rect.min, egui::vec2(38.0, 18.0)),
        Label::new(RichText::new(label).size(8.0).strong().color(theme::SUBTLE)).truncate(),
    );

    let mut chip_left = rect.left() + 44.0;
    if languages.is_empty() {
        draw_track_chip(ui, rect, &mut chip_left, "—", theme::SUBTLE);
    } else {
        for language in languages {
            if !draw_track_chip(ui, rect, &mut chip_left, language, theme::TEXT) {
                break;
            }
        }
    }
}

fn draw_track_chip(
    ui: &mut egui::Ui,
    track_line: egui::Rect,
    chip_left: &mut f32,
    language: &str,
    color: Color32,
) -> bool {
    let text_width = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(
                language.to_owned(),
                FontId::new(9.0, FontFamily::Monospace),
                color,
            )
            .size()
            .x
    });
    let chip_width = text_width + 10.0;
    if *chip_left + chip_width > track_line.right() {
        return false;
    }

    let chip_rect = egui::Rect::from_min_size(
        egui::pos2(*chip_left, track_line.center().y - 9.0),
        egui::vec2(chip_width, 18.0),
    );
    ui.painter()
        .rect_filled(chip_rect, CornerRadius::same(4), theme::INSET);
    ui.painter().rect_stroke(
        chip_rect,
        CornerRadius::same(4),
        Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );
    ui.put(
        chip_rect.shrink2(egui::vec2(5.0, 0.0)),
        Label::new(RichText::new(language).monospace().size(9.0).color(color)).truncate(),
    );
    *chip_left = chip_rect.right() + 5.0;
    true
}

fn styled_text_input(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    width: f32,
    id_salt: &'static str,
) -> egui::Response {
    let response = ui.add_sized(
        [width, 34.0],
        TextEdit::singleline(value)
            .id_salt(id_salt)
            .hint_text(hint)
            .text_color(theme::TEXT)
            .background_color(theme::SURFACE)
            .vertical_align(Align::Center)
            .margin(Margin::symmetric(8, 4))
            .frame(
                Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(0)),
            ),
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(0.5),
            CornerRadius::same(6),
            Stroke::new(1.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response
}

fn draw_switch(ui: &mut egui::Ui, label: &str, value: &mut bool) {
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(34.0, 18.0), Sense::click());
        if response.clicked() {
            *value = !*value;
        }
        let fill = if *value { theme::ACCENT } else { theme::RAISED };
        let border = if *value { theme::ACCENT } else { theme::BORDER };
        ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(9),
            Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        let knob_x = if *value {
            rect.right() - 9.0
        } else {
            rect.left() + 9.0
        };
        ui.painter()
            .circle_filled(egui::pos2(knob_x, rect.center().y), 6.0, Color32::WHITE);
        ui.add_space(4.0);
        ui.label(RichText::new(label).size(11.0).color(theme::TEXT));
    });
}

fn middle_truncate(ui: &mut egui::Ui, text: &str, max_width: f32) -> String {
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
    if max_width <= measure("…".to_owned()) {
        return "…".to_owned();
    }

    let characters: Vec<char> = text.chars().collect();
    let mut prefix_count = characters.len().div_ceil(2);
    let mut suffix_count = characters.len() / 2;
    loop {
        let suffix_start = characters.len().saturating_sub(suffix_count);
        let candidate = format!(
            "{}…{}",
            characters[..prefix_count].iter().collect::<String>(),
            characters[suffix_start..].iter().collect::<String>()
        );
        if measure(candidate.clone()) <= max_width {
            return candidate;
        }
        if prefix_count >= suffix_count && prefix_count > 0 {
            prefix_count -= 1;
        } else if suffix_count > 0 {
            suffix_count -= 1;
        } else {
            return "…".to_owned();
        }
    }
}

fn status_pill(ui: &mut egui::Ui, rect: egui::Rect, status: FileStatus, detail: Option<&str>) {
    let (label, foreground, background) = match status {
        FileStatus::Pending => ("Pending", theme::MUTED, theme::RAISED),
        FileStatus::Processing => ("Working", theme::ACCENT, theme::ACCENT.gamma_multiply(0.14)),
        FileStatus::Success => ("Done", theme::SUCCESS, theme::SUCCESS.gamma_multiply(0.13)),
        FileStatus::Skipped => ("Skipped", theme::MUTED, theme::RAISED),
        FileStatus::Error => ("Failed", theme::ERROR, theme::ERROR.gamma_multiply(0.13)),
        FileStatus::Cancelled => (
            "Stopped",
            theme::WARNING,
            theme::WARNING.gamma_multiply(0.13),
        ),
    };
    let response = ui.put(
        rect,
        Button::new(RichText::new(label).size(9.5).color(foreground))
            .fill(background)
            .stroke(Stroke::new(1.0, foreground.gamma_multiply(0.25)))
            .corner_radius(CornerRadius::same(5)),
    );
    if let Some(detail) = detail {
        response.on_hover_text(detail);
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_owned();
    }
    let exponent = ((bytes as f64).ln() / 1024_f64.ln()).floor() as usize;
    let index = exponent.min(UNITS.len() - 1);
    let amount = bytes as f64 / 1024_f64.powi(index as i32);
    format!("{amount:.1} {}", UNITS[index])
}

fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "—".to_owned();
    }
    let seconds = seconds.round() as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn chrono_time() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| {
            let seconds = duration.as_secs() % 86_400;
            format!(
                "{:02}:{:02}:{:02}",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            )
        })
        .unwrap_or_else(|_| "00:00:00".to_owned())
}

fn visible_is_empty(files: &[FileInfo], query: &str) -> bool {
    !query.is_empty()
        && !files.iter().any(|file| {
            file.name.to_lowercase().contains(query)
                || file.path.to_string_lossy().to_lowercase().contains(query)
        })
}
