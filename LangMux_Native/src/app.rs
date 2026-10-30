use crate::mkv::{self, AvailableLanguages, BatchStats, FileInfo};
use crate::theme;
use crate::worker::{self, BatchSpec, FileStatus, WorkerControl, WorkerEvent};
use eframe::CreationContext;
use eframe::egui;
use eframe::egui::{
    Align, Button, Color32, CornerRadius, Frame, Key, Label, Layout, Margin, Panel, ProgressBar,
    RichText, ScrollArea, Stroke, TextEdit,
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
    files: Vec<FileInfo>,
    progress: Vec<FileProgress>,
    stats: BatchStats,
    languages: Option<AvailableLanguages>,
    scan_loading: bool,
    mkvmerge: Option<PathBuf>,
    mkvmerge_error: Option<String>,
    sender: mpsc::Sender<WorkerEvent>,
    receiver: mpsc::Receiver<WorkerEvent>,
    control: WorkerControl,
    is_running: bool,
    is_paused: bool,
    batch_started: Option<Instant>,
    elapsed_seconds: f64,
    filter: String,
    logs: Vec<String>,
    drag_hovered: bool,
}

impl LangMuxApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let preferences = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, PREFERENCES_KEY))
            .unwrap_or_default();
        let (sender, receiver) = mpsc::channel();
        let mkvmerge = mkv::find_mkvmerge().ok();
        let mkvmerge_error = mkvmerge
            .is_none()
            .then(|| "mkvmerge not found. Install MKVToolNix and add it to PATH.".to_owned());
        let mut app = Self {
            preferences,
            files: Vec::new(),
            progress: Vec::new(),
            stats: BatchStats::default(),
            languages: None,
            scan_loading: false,
            mkvmerge,
            mkvmerge_error,
            sender,
            receiver,
            control: WorkerControl::default(),
            is_running: false,
            is_paused: false,
            batch_started: None,
            elapsed_seconds: 0.0,
            filter: String::new(),
            logs: Vec::new(),
            drag_hovered: false,
        };
        if app.mkvmerge.is_some() {
            app.push_log("MKVToolNix is ready");
        } else if let Some(error) = app.mkvmerge_error.clone() {
            app.push_log(error);
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
        self.languages = None;
        self.scan_loading = true;
        self.push_log(format!("Scanning {}", directory.display()));
        worker::scan_directory(directory, self.mkvmerge.clone(), self.sender.clone(), ctx);
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
        let Some(mkvmerge) = self.mkvmerge.clone() else {
            let message = self
                .mkvmerge_error
                .clone()
                .unwrap_or_else(|| "mkvmerge is unavailable".to_owned());
            self.push_log(message);
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
        self.batch_started = Some(Instant::now());
        self.is_running = true;
        self.is_paused = false;
        worker::start_batch(
            BatchSpec {
                files: self.files.clone(),
                output_directory,
                audio_languages,
                subtitle_languages,
                skip_existing: self.preferences.skip_existing,
                dry_run: self.preferences.dry_run,
            },
            mkvmerge,
            self.control.clone(),
            self.sender.clone(),
            ctx.clone(),
        );
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
                    ui.add_sized(
                        [34.0, 34.0],
                        Button::new(RichText::new("LM").strong().color(Color32::WHITE))
                            .fill(theme::ACCENT)
                            .corner_radius(CornerRadius::same(theme::CORNER)),
                    );
                    ui.vertical(|ui| {
                        ui.label(RichText::new("LangMux").size(17.0).strong());
                        ui.label(
                            RichText::new("LANGUAGE REMUXER")
                                .size(9.5)
                                .color(theme::MUTED),
                        );
                    });
                });
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(10.0);

                let content_height = (ui.available_height() - 76.0).max(240.0);
                ScrollArea::vertical()
                    .max_height(content_height)
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
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let indicator = if self.mkvmerge.is_some() {
                        theme::SUCCESS
                    } else {
                        theme::ERROR
                    };
                    ui.label(RichText::new("●").color(indicator).size(11.0));
                    ui.label(
                        RichText::new(if self.mkvmerge.is_some() {
                            "MKVToolNix detected"
                        } else {
                            "mkvmerge unavailable"
                        })
                        .size(11.0)
                        .color(if self.mkvmerge.is_some() {
                            theme::MUTED
                        } else {
                            theme::ERROR
                        }),
                    );
                });
                if let Some(path) = &self.mkvmerge {
                    ui.add(
                        Label::new(
                            RichText::new(path.to_string_lossy())
                                .monospace()
                                .size(9.5)
                                .color(theme::SUBTLE),
                        )
                        .truncate(),
                    );
                }
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
            let response =
                ui.add_sized(
                    [ui.available_width() - 76.0, 34.0],
                    Label::new(RichText::new(shown).monospace().size(10.5).color(
                        if path.is_empty() {
                            theme::SUBTLE
                        } else {
                            theme::TEXT
                        },
                    ))
                    .truncate(),
                );
            response.on_hover_text(shown);
            let clicked = ui.add_sized(
                [68.0, 34.0],
                Button::new(if input { "Browse" } else { "Choose" })
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
        ui.add_enabled(
            !self.is_running,
            TextEdit::singleline(selected)
                .hint_text(if audio { "eng, jpn" } else { "eng" })
                .desired_width(f32::INFINITY),
        );
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
            ui.checkbox(&mut self.preferences.skip_existing, "Skip existing outputs");
            ui.checkbox(&mut self.preferences.dry_run, "Dry run — preview only");
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
                        "ESTIMATE",
                        &self
                            .stats
                            .estimated_seconds
                            .map(format_duration)
                            .unwrap_or_else(|| "—".to_owned()),
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
                            .add(Button::new(log_label).corner_radius(CornerRadius::same(6)))
                            .clicked()
                        {
                            self.preferences.logs_open = !self.preferences.logs_open;
                        }
                        ui.add_sized(
                            [230.0, 30.0],
                            TextEdit::singleline(&mut self.filter)
                                .hint_text("Filter files")
                                .desired_width(230.0),
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
                                [104.0, 38.0],
                                Button::new(if self.is_paused { "Resume" } else { "Pause" })
                                    .corner_radius(CornerRadius::same(6)),
                            )
                            .clicked()
                        {
                            self.toggle_pause();
                        }
                        if ui
                            .add_sized(
                                [104.0, 38.0],
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
        if let Some(error) = &self.mkvmerge_error {
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

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("FILE")
                    .size(9.5)
                    .strong()
                    .color(theme::SUBTLE),
            );
            ui.add_space(ui.available_width() * 0.34);
            ui.label(
                RichText::new("SIZE")
                    .size(9.5)
                    .strong()
                    .color(theme::SUBTLE),
            );
            ui.add_space(30.0);
            ui.label(
                RichText::new("TRACKS")
                    .size(9.5)
                    .strong()
                    .color(theme::SUBTLE),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new("STATUS")
                        .size(9.5)
                        .strong()
                        .color(theme::SUBTLE),
                );
            });
        });
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for index in visible {
                    self.draw_file_row(ui, index);
                }
                if visible_is_empty(&self.files, &query) {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No files match this filter").color(theme::MUTED));
                    });
                }
            });
    }

    fn draw_file_row(&self, ui: &mut egui::Ui, index: usize) {
        let Some(file) = self.files.get(index) else {
            return;
        };
        let progress = self.progress.get(index).cloned().unwrap_or_default();
        let audio_languages = mkv::parse_language_field(&self.preferences.audio_languages);
        let subtitle_languages = mkv::parse_language_field(&self.preferences.subtitle_languages);
        let (audio_count, subtitle_count) = file.metadata.as_ref().map_or((0, 0), |metadata| {
            let (audio, subtitles) =
                mkv::selected_tracks(metadata, &audio_languages, &subtitle_languages);
            (audio.len(), subtitles.len())
        });

        Frame::new()
            .fill(theme::SURFACE)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(CornerRadius::same(7))
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&file.name)
                                    .size(12.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                        });
                        ui.label(
                            RichText::new(format!(
                                "{} audio · {} subtitle{} matched",
                                audio_count,
                                subtitle_count,
                                if subtitle_count == 1 { "" } else { "s" }
                            ))
                            .size(10.0)
                            .color(theme::MUTED),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        status_pill(ui, progress.status, progress.detail.as_deref());
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(format_size(file.size))
                                .monospace()
                                .size(10.5)
                                .color(theme::MUTED),
                        );
                    });
                });
                if progress.status == FileStatus::Processing {
                    ui.add_space(7.0);
                    ui.add(
                        ProgressBar::new(progress.percent as f32 / 100.0)
                            .desired_width(ui.available_width())
                            .desired_height(3.0)
                            .fill(theme::ACCENT)
                            .corner_radius(CornerRadius::same(2)),
                    );
                }
                if let Some(detail) = progress.detail
                    && progress.status == FileStatus::Error
                {
                    ui.add_space(4.0);
                    ui.label(RichText::new(detail).size(10.0).color(theme::ERROR));
                }
            });
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

fn status_pill(ui: &mut egui::Ui, status: FileStatus, detail: Option<&str>) {
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
    let response = ui.add(
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
