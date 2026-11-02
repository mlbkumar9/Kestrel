use crate::analysis;
use crate::encoders::{self, EncoderSet};
use crate::inputs::{self, ExistingPolicy, InputItem, OutputTarget, SourceFile};
use crate::legacy::LegacyInstall;
use crate::mkv::{self, AvailableLanguages, FileInfo, MediaMetadata};
use crate::presets::Preset;
use crate::theme;
use crate::tools::Toolchain;
use crate::transcode::{Crop, JobSettings};
use crate::worker::{
    self, FileStatus, JobSpec, QueueSpec, RemuxEngine, WorkerControl, WorkerEvent,
};
use eframe::CreationContext;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Step {
    #[default]
    Source,
    Settings,
    Queue,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    #[default]
    Summary,
    Video,
    Audio,
    Subtitles,
    Metadata,
}

#[derive(Clone)]
pub(crate) struct SourceRow {
    pub source: SourceFile,
    pub info: FileInfo,
    pub checked: bool,
}

pub(crate) struct QueueEntry {
    pub id: u64,
    pub input: PathBuf,
    pub output: PathBuf,
    pub metadata: MediaMetadata,
    pub settings: JobSettings,
    pub preset: String,
    pub status: FileStatus,
    pub percent: u32,
    pub detail: Option<String>,
    pub speed: Option<f64>,
    pub eta: Option<f64>,
    pub original_size: u64,
    pub output_size: Option<u64>,
    pub checked: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    pub(crate) input_items: Vec<InputItem>,
    pub(crate) output_directory: String,
    pub(crate) audio_languages: String,
    pub(crate) subtitle_languages: String,
    #[serde(default, alias = "engine")]
    pub(crate) remux_engine: RemuxEngine,
    pub(crate) dry_run: bool,
    pub(crate) logs_open: bool,
    pub(crate) selected_preset_id: String,
    pub(crate) current_settings: JobSettings,
    pub(crate) user_presets: Vec<Preset>,
    pub(crate) existing: ExistingPolicy,
    pub(crate) include_subfolders: bool,
    pub(crate) last_input_dir: String,
    pub(crate) output_target: Option<OutputTarget>,
    pub(crate) input_directory: String,
    pub(crate) skip_existing: bool,
    pub(crate) legacy_banner_dismissed: bool,
    #[serde(default)]
    pub(crate) output_target_explicit: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            input_items: Vec::new(),
            output_directory: String::new(),
            audio_languages: "eng, jpn".to_owned(),
            subtitle_languages: "eng".to_owned(),
            remux_engine: RemuxEngine::MkvToolNix,
            dry_run: false,
            logs_open: false,
            selected_preset_id: "remux".to_owned(),
            current_settings: JobSettings::default(),
            user_presets: Vec::new(),
            existing: ExistingPolicy::Skip,
            include_subfolders: true,
            last_input_dir: String::new(),
            output_target: None,
            input_directory: String::new(),
            skip_existing: true,
            legacy_banner_dismissed: false,
            output_target_explicit: false,
        }
    }
}

fn migrate_preferences(preferences: &mut Preferences, from_legacy_record: bool) {
    if from_legacy_record {
        if preferences.input_items.is_empty() && !preferences.input_directory.is_empty() {
            preferences.input_items.push(InputItem::Folder {
                path: PathBuf::from(&preferences.input_directory),
                recursive: preferences.include_subfolders,
            });
        }
        preferences.output_target_explicit = !preferences.output_directory.is_empty()
            && Path::new(&preferences.output_directory)
                != mkv::default_output_dir(Path::new(&preferences.input_directory));
        preferences.existing = if preferences.skip_existing {
            ExistingPolicy::Skip
        } else {
            ExistingPolicy::Overwrite
        };
        preferences.input_directory.clear();
        preferences.skip_existing = true;
    }
    if preferences.output_target.is_none() && !preferences.output_directory.is_empty() {
        preferences.output_target = Some(OutputTarget::Folder(PathBuf::from(
            &preferences.output_directory,
        )));
    }
    preferences.current_settings.audio.languages =
        mkv::parse_language_field(&preferences.audio_languages);
    preferences.current_settings.subtitles.languages =
        mkv::parse_language_field(&preferences.subtitle_languages);
}

enum UiEvent {
    SourceAdded {
        generation: u64,
        source: SourceFile,
        info: FileInfo,
    },
    SourceMetadata {
        generation: u64,
        path: PathBuf,
        result: Result<MediaMetadata, String>,
    },
    ScanFinished {
        generation: u64,
    },
    PreviewReady {
        path: PathBuf,
        key: String,
        time_key: u64,
        result: Result<Vec<u8>, String>,
    },
    EstimateReady {
        result: Result<u64, String>,
    },
    CropReady {
        result: Option<Crop>,
    },
    LegacyChecked {
        result: Option<LegacyInstall>,
    },
    LegacyActionFinished {
        result: Result<(), String>,
        found: Option<LegacyInstall>,
    },
}

pub struct KestrelApp {
    pub(crate) preferences: Preferences,
    pub(crate) step: Step,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) sources: Vec<SourceRow>,
    pub(crate) queue: Vec<QueueEntry>,
    pub(crate) editing_job: Option<u64>,
    pub(crate) tools: Toolchain,
    pub(crate) encoders: EncoderSet,
    pub(crate) search: String,
    pub(crate) search_focus: bool,
    pub(crate) logs: Vec<String>,
    pub(crate) logs_filter: String,
    pub(crate) drag_hovered: bool,
    pub(crate) confirm_stop: bool,
    pub(crate) preview_time: f64,
    pub(crate) preview_source: Option<PathBuf>,
    pub(crate) preview_bytes: Option<Vec<u8>>,
    pub(crate) preview_texture: Option<egui::TextureHandle>,
    pub(crate) preview_error: Option<String>,
    pub(crate) size_estimate: Option<Result<u64, String>>,
    pub(crate) estimate_running: bool,
    pub(crate) queue_running: bool,
    pub(crate) paused: bool,
    pub(crate) toast: Option<(String, Instant)>,
    pub(crate) output_error: Option<String>,
    pub(crate) about_open: bool,
    pub(crate) advanced_open: bool,
    pub(crate) preset_name: String,
    pub(crate) preview_original: bool,
    pub(crate) scan_loading: bool,
    pub(crate) logo: Option<egui::TextureHandle>,
    pub(crate) legacy_install: Option<LegacyInstall>,
    pub(crate) legacy_action_running: bool,
    sender: mpsc::Sender<WorkerEvent>,
    receiver: mpsc::Receiver<WorkerEvent>,
    ui_sender: mpsc::Sender<UiEvent>,
    ui_receiver: mpsc::Receiver<UiEvent>,
    encoder_receiver: Option<mpsc::Receiver<Result<EncoderSet, String>>>,
    control: WorkerControl,
    scan_generation: u64,
    next_job_id: u64,
    preview_dirty_at: Option<Instant>,
    pub(crate) preview_pending: bool,
    preview_cache: HashMap<(PathBuf, u64, String), Vec<u8>>,
}

impl KestrelApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let logo = image::load_from_memory(include_bytes!("../assets/kestrel.png"))
            .ok()
            .map(|image| {
                let rgba = image.into_rgba8();
                cc.egui_ctx.load_texture(
                    "kestrel-logo",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [rgba.width() as usize, rgba.height() as usize],
                        rgba.as_raw(),
                    ),
                    egui::TextureOptions::LINEAR,
                )
            });
        let (mut preferences, from_legacy_record) = cc
            .storage
            .map_or_else(|| (Preferences::default(), false), load_preferences);
        migrate_preferences(&mut preferences, from_legacy_record);
        #[cfg(debug_assertions)]
        let tools = if std::env::var("KESTREL_DEBUG_NO_TOOLS").as_deref() == Ok("1") {
            Toolchain::default()
        } else {
            Toolchain::discover()
        };
        #[cfg(not(debug_assertions))]
        let tools = Toolchain::discover();
        let encoder_receiver = tools
            .ffmpeg
            .as_ref()
            .map(|ffmpeg| encoders::detect_async(ffmpeg.path.clone()));
        let (sender, receiver) = mpsc::channel();
        let (ui_sender, ui_receiver) = mpsc::channel();
        #[cfg(debug_assertions)]
        let legacy_install = (std::env::var("KESTREL_DEBUG_LEGACY_BANNER").as_deref() == Ok("1"))
            .then(crate::legacy::debug_sample);
        #[cfg(not(debug_assertions))]
        let legacy_install = None;
        let mut app = Self {
            preferences,
            step: Step::Source,
            settings_tab: SettingsTab::Summary,
            sources: Vec::new(),
            queue: Vec::new(),
            editing_job: None,
            tools,
            encoders: EncoderSet::default(),
            search: String::new(),
            search_focus: false,
            logs: Vec::new(),
            logs_filter: "All".to_owned(),
            drag_hovered: false,
            confirm_stop: false,
            preview_time: 0.0,
            preview_source: None,
            preview_bytes: None,
            preview_texture: None,
            preview_error: None,
            size_estimate: None,
            estimate_running: false,
            queue_running: false,
            paused: false,
            toast: None,
            output_error: None,
            about_open: false,
            advanced_open: false,
            preset_name: String::new(),
            preview_original: false,
            scan_loading: false,
            logo,
            legacy_install,
            legacy_action_running: false,
            sender,
            receiver,
            ui_sender,
            ui_receiver,
            encoder_receiver,
            control: WorkerControl::default(),
            scan_generation: 0,
            next_job_id: 1,
            preview_dirty_at: None,
            preview_pending: false,
            preview_cache: HashMap::new(),
        };
        #[cfg(windows)]
        if app.legacy_install.is_none() {
            app.check_legacy_install();
        }
        if !app.preferences.input_items.is_empty() {
            app.refresh_sources();
        }
        app
    }

    pub(crate) fn check_legacy_install(&self) {
        let sender = self.ui_sender.clone();
        std::thread::spawn(move || {
            let _ = sender.send(UiEvent::LegacyChecked {
                result: crate::legacy::find_legacy_install(),
            });
        });
    }

    pub(crate) fn remove_legacy_install(&mut self) {
        let Some(install) = self.legacy_install.clone() else {
            return;
        };
        self.legacy_action_running = true;
        let sender = self.ui_sender.clone();
        std::thread::spawn(move || {
            let result = crate::legacy::remove_legacy_install(&install);
            let found = crate::legacy::find_legacy_install();
            let _ = sender.send(UiEvent::LegacyActionFinished { result, found });
        });
    }

    pub(crate) fn push_log(&mut self, text: impl Into<String>) {
        self.logs.push(format!("{}  {}", clock_time(), text.into()));
        if self.logs.len() > 2000 {
            self.logs.drain(..self.logs.len() - 2000);
        }
    }

    pub(crate) fn add_paths(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            let item = if path.is_dir() {
                self.preferences.last_input_dir = path.to_string_lossy().into_owned();
                InputItem::Folder {
                    path,
                    recursive: self.preferences.include_subfolders,
                }
            } else if inputs::expand(&[InputItem::File(path.clone())], &[]).is_empty() {
                continue;
            } else {
                self.preferences.last_input_dir = path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_string_lossy()
                    .into_owned();
                InputItem::File(path)
            };
            if !self.preferences.input_items.contains(&item) {
                self.preferences.input_items.push(item);
            }
        }
        self.refresh_sources();
        self.step = Step::Source;
    }

    pub(crate) fn add_files_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if !self.preferences.last_input_dir.is_empty() {
            dialog = dialog.set_directory(&self.preferences.last_input_dir);
        }
        if let Some(paths) = dialog.pick_files() {
            self.add_paths(paths);
        }
    }

    pub(crate) fn add_folder_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if !self.preferences.last_input_dir.is_empty() {
            dialog = dialog.set_directory(&self.preferences.last_input_dir);
        }
        if let Some(path) = dialog.pick_folder() {
            self.add_paths(vec![path]);
        }
    }

    pub(crate) fn refresh_sources(&mut self) {
        self.scan_generation = self.scan_generation.wrapping_add(1);
        let generation = self.scan_generation;
        let mut items = self.preferences.input_items.clone();
        for item in &mut items {
            if let InputItem::Folder { recursive, .. } = item {
                *recursive = self.preferences.include_subfolders;
            }
        }
        self.sources.clear();
        self.scan_loading = true;
        self.search.clear();
        self.preview_bytes = None;
        self.preview_texture = None;
        self.preview_source = None;
        self.preview_pending = false;
        self.preferences.input_items = items.clone();
        self.preferences.input_directory =
            items.first().map_or_else(String::new, |item| match item {
                InputItem::File(path) => path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_string_lossy()
                    .into_owned(),
                InputItem::Folder { path, .. } => path.to_string_lossy().into_owned(),
            });
        let output_target = if self.preferences.output_target_explicit {
            self.preferences.output_target.clone()
        } else {
            Some(inputs::default_output_target(
                &items,
                self.preferences.current_settings.container,
            ))
        };
        let exclude = output_target
            .as_ref()
            .map(|target| {
                inputs::output_exclusions(
                    target,
                    &items,
                    self.preferences.current_settings.container,
                )
            })
            .unwrap_or_default();
        let sender = self.ui_sender.clone();
        let tools = self.tools.clone();
        self.push_log("Scanning input sources");
        std::thread::spawn(move || {
            for source in inputs::expand(&items, &exclude) {
                let info = FileInfo {
                    name: source
                        .relative
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: source.path.clone(),
                    size: std::fs::metadata(&source.path).map_or(0, |metadata| metadata.len()),
                    metadata: None,
                };
                if sender
                    .send(UiEvent::SourceAdded {
                        generation,
                        source: source.clone(),
                        info,
                    })
                    .is_err()
                {
                    return;
                }
                let result = mkv::read_metadata(
                    tools.mkvmerge.as_ref().map(|tool| tool.path.as_path()),
                    tools.ffprobe.as_ref().map(|tool| tool.path.as_path()),
                    &source.path,
                );
                if sender
                    .send(UiEvent::SourceMetadata {
                        generation,
                        path: source.path,
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            let _ = sender.send(UiEvent::ScanFinished { generation });
        });
    }

    pub(crate) fn clear_sources(&mut self) {
        clear_source_preferences(&mut self.preferences);
        self.preferences.output_target = None;
        self.preferences.output_target_explicit = false;
        self.preferences.output_directory.clear();
        self.sources.clear();
        self.scan_loading = false;
        self.search.clear();
        self.preview_bytes = None;
        self.push_log("Source list cleared");
    }

    pub(crate) fn selected_source_indices(&self) -> Vec<usize> {
        let selected: Vec<usize> = self
            .sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| source.checked.then_some(index))
            .collect();
        if selected.is_empty() {
            (0..self.sources.len()).collect()
        } else {
            selected
        }
    }

    pub(crate) fn available_languages(&self) -> AvailableLanguages {
        let mut audio = BTreeSet::new();
        let mut subtitles = BTreeSet::new();
        for source in &self.sources {
            if let Some(metadata) = &source.info.metadata {
                audio.extend(
                    metadata
                        .audio_tracks
                        .iter()
                        .map(|track| track.language.clone()),
                );
                subtitles.extend(
                    metadata
                        .subtitle_tracks
                        .iter()
                        .map(|track| track.language.clone()),
                );
            }
        }
        AvailableLanguages {
            audio: audio.into_iter().filter(|code| !code.is_empty()).collect(),
            subtitles: subtitles
                .into_iter()
                .filter(|code| !code.is_empty())
                .collect(),
        }
    }

    pub(crate) fn output_target_for(&self, selected: &[usize]) -> OutputTarget {
        let selected_paths = selected
            .iter()
            .filter_map(|index| self.sources.get(*index))
            .map(|source| source.source.path.clone())
            .collect::<Vec<_>>();
        effective_output_target(
            self.preferences.output_target.as_ref(),
            self.preferences.output_target_explicit,
            &selected_paths,
            &self.preferences.input_items,
            self.preferences.current_settings.container,
        )
    }

    pub(crate) fn set_output_target(&mut self, target: OutputTarget) {
        self.preferences.output_directory = match &target {
            OutputTarget::File(path) => path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_string_lossy()
                .into_owned(),
            OutputTarget::Folder(path) => path.to_string_lossy().into_owned(),
        };
        self.preferences.output_target = Some(target);
        self.preferences.output_target_explicit = true;
        self.output_error = None;
    }

    pub(crate) fn browse_output(&mut self, selected: &[usize]) {
        let target = self.output_target_for(selected);
        match target {
            OutputTarget::File(path) if selected.len() == 1 => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(path.file_name().unwrap_or_default().to_string_lossy())
                    .save_file()
                {
                    self.set_output_target(OutputTarget::File(path));
                }
            }
            _ => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.set_output_target(OutputTarget::Folder(path));
                }
            }
        }
    }

    pub(crate) fn reset_output_target(&mut self) {
        self.preferences.output_target = None;
        self.preferences.output_target_explicit = false;
        self.preferences.output_directory.clear();
        self.output_error = None;
    }

    pub(crate) fn add_to_queue(&mut self, start_now: bool, ctx: &egui::Context) {
        self.output_error = None;
        let selected = self.selected_source_indices();
        if selected.is_empty() {
            self.output_error = Some("Add at least one source file to continue.".to_owned());
            self.step = Step::Source;
            return;
        }
        let sources = selected
            .iter()
            .map(|index| self.sources[*index].source.clone())
            .collect::<Vec<_>>();
        let target = self.output_target_for(&selected);
        let plans = match inputs::plan_outputs(
            &sources,
            &target,
            self.preferences.current_settings.container,
        ) {
            Ok(plans) => plans,
            Err(error) => {
                self.output_error = Some(error);
                self.step = Step::Source;
                return;
            }
        };
        for (index, plan) in selected.iter().zip(plans) {
            let source = &self.sources[*index];
            let id = self.next_job_id;
            self.next_job_id = self.next_job_id.saturating_add(1);
            self.queue.push(QueueEntry {
                id,
                input: plan.input,
                output: plan.output,
                metadata: source.info.metadata.clone().unwrap_or_default(),
                settings: self.preferences.current_settings.clone(),
                preset: self.preferences.selected_preset_id.clone(),
                status: FileStatus::Pending,
                percent: 0,
                detail: None,
                speed: None,
                eta: None,
                original_size: source.info.size,
                output_size: None,
                checked: false,
            });
        }
        self.push_log(format!(
            "Added {} to the queue",
            crate::ui::widgets::count_label(selected.len(), "job", "jobs")
        ));
        self.step = Step::Queue;
        if start_now {
            self.start_queue(ctx);
        }
    }

    pub(crate) fn start_queue(&mut self, ctx: &egui::Context) {
        if self.queue_running {
            return;
        }
        let jobs = self
            .queue
            .iter()
            .filter(|entry| entry.status == FileStatus::Pending)
            .map(|entry| JobSpec {
                id: entry.id,
                input: entry.input.clone(),
                output: entry.output.clone(),
                metadata: entry.metadata.clone(),
                settings: entry.settings.clone(),
            })
            .collect::<Vec<_>>();
        if jobs.is_empty() {
            self.push_log("There are no pending jobs to start");
            return;
        }
        self.control = WorkerControl::default();
        self.queue_running = true;
        self.paused = false;
        worker::start_queue(
            QueueSpec {
                jobs,
                tools: self.tools.clone(),
                encoders: self.encoders.clone(),
                remux_engine: self.preferences.remux_engine,
                existing: self.preferences.existing,
                dry_run: self.preferences.dry_run,
            },
            self.control.clone(),
            self.sender.clone(),
            ctx.clone(),
        );
    }

    pub(crate) fn toggle_pause(&mut self) {
        if self.queue_running {
            self.paused = !self.paused;
            self.control.set_paused(self.paused);
        }
    }

    pub(crate) fn stop_queue(&mut self) {
        if self.queue_running {
            self.control.stop();
            self.paused = false;
            self.push_log("Stopping the active job");
        }
    }

    pub(crate) fn retry_job(&mut self, id: u64) {
        if let Some(job) = self.queue.iter_mut().find(|job| job.id == id) {
            job.status = FileStatus::Pending;
            job.percent = 0;
            job.detail = None;
            job.output_size = None;
        }
    }

    pub(crate) fn duplicate_job(&mut self, id: u64) {
        if let Some(index) = self.queue.iter().position(|job| job.id == id) {
            let source = &self.queue[index];
            let new_id = self.next_job_id;
            self.next_job_id = self.next_job_id.saturating_add(1);
            self.queue.push(QueueEntry {
                id: new_id,
                input: source.input.clone(),
                output: source.output.clone(),
                metadata: source.metadata.clone(),
                settings: source.settings.clone(),
                preset: source.preset.clone(),
                status: FileStatus::Pending,
                percent: 0,
                detail: None,
                speed: None,
                eta: None,
                original_size: source.original_size,
                output_size: None,
                checked: false,
            });
        }
    }

    pub(crate) fn edit_job(&mut self, id: u64) {
        if let Some(job) = self.queue.iter().find(|job| job.id == id) {
            self.preferences.current_settings = job.settings.clone();
            self.preferences.selected_preset_id = job.preset.clone();
            self.editing_job = Some(id);
            self.step = Step::Settings;
        }
    }

    pub(crate) fn update_edited_job(&mut self) {
        if let Some(id) = self.editing_job.take()
            && let Some(job) = self.queue.iter_mut().find(|job| job.id == id)
        {
            job.settings = self.preferences.current_settings.clone();
            job.preset = self.preferences.selected_preset_id.clone();
            job.status = FileStatus::Pending;
            job.percent = 0;
        }
        self.step = Step::Queue;
    }

    pub(crate) fn remove_selected_queue(&mut self) {
        if self.queue_running {
            return;
        }
        self.queue.retain(|job| !job.checked);
    }

    pub(crate) fn move_job(&mut self, id: u64, offset: isize) {
        let Some(index) = self.queue.iter().position(|job| job.id == id) else {
            return;
        };
        let target = (index as isize + offset).clamp(0, self.queue.len() as isize - 1) as usize;
        self.queue.swap(index, target);
    }

    pub(crate) fn estimate_size(&mut self, source_index: usize) {
        let Some(source) = self.sources.get(source_index) else {
            return;
        };
        let (Some(ffmpeg), Some(metadata)) =
            (self.tools.ffmpeg.clone(), source.info.metadata.clone())
        else {
            self.size_estimate = Some(Err("Source metadata or FFmpeg is unavailable".to_owned()));
            return;
        };
        let input = source.source.path.clone();
        let mut settings = self.preferences.current_settings.clone();
        if self.preview_original {
            settings.video = crate::transcode::VideoSettings::Copy;
            settings.subtitles.burn_in = crate::transcode::BurnIn::None;
        }
        let encoders = self.encoders.clone();
        let sender = self.ui_sender.clone();
        self.size_estimate = None;
        self.estimate_running = true;
        std::thread::spawn(move || {
            let result =
                analysis::estimate_size(&ffmpeg.path, &input, &metadata, &settings, &encoders);
            let _ = sender.send(UiEvent::EstimateReady { result });
        });
    }

    pub(crate) fn detect_crop(&self, source_index: usize) {
        let Some(source) = self.sources.get(source_index) else {
            return;
        };
        let (Some(ffmpeg), Some(metadata)) =
            (self.tools.ffmpeg.clone(), source.info.metadata.clone())
        else {
            return;
        };
        let input = source.source.path.clone();
        let duration = metadata.duration.unwrap_or_default();
        let sender = self.ui_sender.clone();
        std::thread::spawn(move || {
            let result = analysis::detect_crop(&ffmpeg.path, &input, duration);
            let _ = sender.send(UiEvent::CropReady { result });
        });
    }

    pub(crate) fn mark_preview_dirty(&mut self) {
        self.preview_dirty_at = Some(Instant::now());
        self.preview_error = None;
    }

    fn request_preview(&mut self) {
        if self.preview_pending {
            return;
        }
        let Some(index) = self
            .preview_source
            .as_ref()
            .and_then(|path| {
                self.sources
                    .iter()
                    .position(|source| &source.source.path == path)
            })
            .or_else(|| self.selected_source_indices().first().copied())
        else {
            return;
        };
        let Some(source) = self.sources.get(index) else {
            return;
        };
        let (Some(ffmpeg), Some(metadata)) =
            (self.tools.ffmpeg.clone(), source.info.metadata.clone())
        else {
            return;
        };
        let path = source.source.path.clone();
        let key = format!(
            "{}:{:?}",
            if self.preview_original {
                "original"
            } else {
                "filtered"
            },
            self.preferences.current_settings
        );
        let time_key = (self.preview_time.max(0.0) * 4.0).round() as u64;
        if let Some(bytes) = self
            .preview_cache
            .get(&(path.clone(), time_key, key.clone()))
            .cloned()
        {
            self.preview_bytes = Some(bytes);
            self.preview_source = Some(path);
            self.preview_dirty_at = None;
            return;
        }
        self.preview_source = Some(path.clone());
        self.preview_pending = true;
        self.preview_dirty_at = None;
        let mut settings = self.preferences.current_settings.clone();
        if self.preview_original {
            settings.video = crate::transcode::VideoSettings::Copy;
            settings.subtitles.burn_in = crate::transcode::BurnIn::None;
        }
        let sender = self.ui_sender.clone();
        let time = self.preview_time;
        std::thread::spawn(move || {
            let result =
                analysis::extract_frame(&ffmpeg.path, &path, time, &metadata, &settings, 720);
            let _ = sender.send(UiEvent::PreviewReady {
                path,
                key,
                time_key,
                result,
            });
        });
    }

    fn handle_ui_events(&mut self) {
        let ui_events = self.ui_receiver.try_iter().collect::<Vec<_>>();
        for event in ui_events {
            match event {
                UiEvent::SourceAdded {
                    generation,
                    source,
                    info,
                } if generation == self.scan_generation => {
                    self.sources.push(SourceRow {
                        source,
                        info,
                        checked: false,
                    });
                }
                UiEvent::SourceMetadata {
                    generation,
                    path,
                    result,
                } if generation == self.scan_generation => {
                    if let Some(source) = self
                        .sources
                        .iter_mut()
                        .find(|source| source.source.path == path)
                    {
                        match result {
                            Ok(metadata) => source.info.metadata = Some(metadata),
                            Err(error) => self.push_log(format!(
                                "Metadata unavailable for {}: {error}",
                                path.display()
                            )),
                        }
                    }
                }
                UiEvent::ScanFinished { generation } if generation == self.scan_generation => {
                    self.scan_loading = false;
                    self.sources
                        .sort_by(|left, right| left.source.relative.cmp(&right.source.relative));
                    self.push_log(format!(
                        "Loaded {}",
                        crate::ui::widgets::count_label(
                            self.sources.len(),
                            "source file",
                            "source files"
                        )
                    ));
                }
                UiEvent::PreviewReady {
                    path,
                    key,
                    time_key,
                    result,
                } => {
                    self.preview_pending = false;
                    match result {
                        Ok(bytes) => {
                            self.preview_cache
                                .insert((path, time_key, key), bytes.clone());
                            self.preview_bytes = Some(bytes);
                            self.preview_error = None;
                            self.preview_texture = None;
                        }
                        Err(error) => self.preview_error = Some(error),
                    }
                }
                UiEvent::EstimateReady { result } => {
                    self.estimate_running = false;
                    self.size_estimate = Some(result);
                }
                UiEvent::CropReady { result } => {
                    if let Some(crop @ Crop::Custom { .. }) = result
                        && let crate::transcode::VideoSettings::Encode(encode) =
                            &mut self.preferences.current_settings.video
                    {
                        encode.crop = crop;
                    }
                }
                UiEvent::LegacyChecked { result } => {
                    self.legacy_install = result;
                }
                UiEvent::LegacyActionFinished { result, found } => {
                    self.legacy_action_running = false;
                    self.legacy_install = found;
                    match result {
                        Ok(()) => self.push_log("Removed the older per-user installation"),
                        Err(error) => {
                            self.push_log(format!("Could not remove older installation: {error}"))
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(receiver) = &self.encoder_receiver
            && let Ok(result) = receiver.try_recv()
        {
            match result {
                Ok(encoders) => self.encoders = encoders,
                Err(error) => self.push_log(error),
            }
            self.encoder_receiver = None;
        }
        let worker_events = self.receiver.try_iter().collect::<Vec<_>>();
        for event in worker_events {
            match event {
                WorkerEvent::JobUpdated {
                    job_id,
                    status,
                    percent,
                    detail,
                } => {
                    if let Some(job) = self.queue.iter_mut().find(|job| job.id == job_id) {
                        job.status = status;
                        job.percent = percent;
                        job.detail = detail;
                        if status == FileStatus::Success {
                            job.output_size = std::fs::metadata(&job.output)
                                .ok()
                                .map(|metadata| metadata.len());
                        }
                    }
                }
                WorkerEvent::Progress {
                    job_id,
                    percent,
                    speed,
                    eta_seconds,
                    ..
                } => {
                    if let Some(job) = self.queue.iter_mut().find(|job| job.id == job_id) {
                        job.percent = percent;
                        if speed.is_some() {
                            job.speed = speed;
                        }
                        if eta_seconds.is_some() {
                            job.eta = eta_seconds;
                        }
                    }
                }
                WorkerEvent::Log(message) => self.push_log(message),
                WorkerEvent::BatchFinished { cancelled, .. } => {
                    self.queue_running = false;
                    self.paused = false;
                    self.toast = Some((
                        if cancelled {
                            "Queue stopped".to_owned()
                        } else {
                            "Queue finished".to_owned()
                        },
                        Instant::now(),
                    ));
                    self.push_log(if cancelled {
                        "Queue stopped"
                    } else {
                        "Queue finished"
                    });
                }
                _ => {}
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let ctrl = egui::Modifiers::CTRL;
        if ctx.input_mut(|input| input.consume_key(ctrl, egui::Key::O)) && !self.queue_running {
            self.add_files_dialog();
        }
        if ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::O)
        }) && !self.queue_running
        {
            self.add_folder_dialog();
        }
        for (key, step) in [
            (egui::Key::Num1, Step::Source),
            (egui::Key::Num2, Step::Settings),
            (egui::Key::Num3, Step::Queue),
        ] {
            if ctx.input_mut(|input| input.consume_key(ctrl, key)) {
                self.step = step;
            }
        }
        if ctx.input_mut(|input| input.consume_key(ctrl, egui::Key::Enter)) {
            self.add_to_queue(true, ctx);
        }
        if !typing
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Space))
            && self.queue_running
        {
            self.toggle_pause();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && self.queue_running
        {
            self.confirm_stop = true;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
            if self.step == Step::Queue {
                self.remove_selected_queue();
            } else {
                self.sources.retain(|source| !source.checked);
            }
        }
        if ctx.input_mut(|input| input.consume_key(ctrl, egui::Key::F)) {
            self.search_focus = true;
            self.step = Step::Source;
        }
    }
}

impl eframe::App for KestrelApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.handle_ui_events();
        self.handle_shortcuts(&ctx);
        self.drag_hovered = ctx.input(|input| !input.raw.hovered_files.is_empty());
        let dropped = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect::<Vec<_>>()
        });
        if !dropped.is_empty() {
            self.add_paths(dropped);
        }
        if self
            .preview_dirty_at
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(250))
        {
            self.request_preview();
        }
        crate::ui::shell::show(self, root, &ctx);
        if self.queue_running || self.preview_dirty_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.preferences.audio_languages =
            self.preferences.current_settings.audio.languages.join(", ");
        self.preferences.subtitle_languages = self
            .preferences
            .current_settings
            .subtitles
            .languages
            .join(", ");
        eframe::set_value(storage, crate::branding::PREFERENCES_KEY, &self.preferences);
    }
}

fn clock_time() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() % 86_400);
    format!("{:02}:{:02}", seconds / 3600, seconds / 60 % 60)
}

fn clear_source_preferences(preferences: &mut Preferences) {
    preferences.input_items.clear();
    preferences.input_directory.clear();
}

fn load_preferences(storage: &dyn eframe::Storage) -> (Preferences, bool) {
    if let Some(preferences) = eframe::get_value(storage, crate::branding::PREFERENCES_KEY) {
        return (preferences, false);
    }
    crate::branding::LEGACY_PREFERENCES_KEYS
        .iter()
        .find_map(|key| {
            eframe::get_value(storage, key).map(|preferences| {
                (
                    preferences,
                    *key == crate::branding::LEGACY_V2_PREFERENCES_KEY,
                )
            })
        })
        .unwrap_or_else(|| (Preferences::default(), false))
}

fn effective_output_target(
    stored_target: Option<&OutputTarget>,
    target_is_explicit: bool,
    selected_paths: &[PathBuf],
    input_items: &[InputItem],
    container: crate::formats::Container,
) -> OutputTarget {
    if target_is_explicit && let Some(target) = stored_target {
        return match (target, selected_paths.len()) {
            (OutputTarget::File(path), 1) => OutputTarget::File(path.clone()),
            (OutputTarget::File(path), _) => OutputTarget::Folder(
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf(),
            ),
            (OutputTarget::Folder(path), _) => OutputTarget::Folder(path.clone()),
        };
    }
    if let [path] = selected_paths {
        return inputs::default_output_target(&[InputItem::File(path.clone())], container);
    }
    inputs::default_output_target(input_items, container)
}

#[cfg(test)]
mod tests {
    use super::{
        Preferences, clear_source_preferences, effective_output_target, load_preferences,
        migrate_preferences,
    };
    use crate::formats::Container;
    use crate::inputs::{ExistingPolicy, InputItem, OutputTarget};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Default)]
    struct MemoryStorage(HashMap<String, String>);

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }

        fn flush(&mut self) {}
    }

    #[test]
    fn loads_preferences_from_a_legacy_langmux_key() {
        let mut storage = MemoryStorage::default();
        eframe::set_value(
            &mut storage,
            crate::branding::LEGACY_V2_PREFERENCES_KEY,
            &Preferences {
                input_directory: "/movies".to_owned(),
                output_directory: "/exports".to_owned(),
                skip_existing: false,
                ..Preferences::default()
            },
        );
        let (mut preferences, from_legacy) = load_preferences(&storage);
        assert!(from_legacy);
        migrate_preferences(&mut preferences, from_legacy);
        assert_eq!(
            preferences.input_items,
            [InputItem::Folder {
                path: PathBuf::from("/movies"),
                recursive: true,
            }]
        );
        assert_eq!(preferences.existing, ExistingPolicy::Overwrite);
        assert!(preferences.output_target_explicit);
        assert_eq!(
            preferences.output_target,
            Some(OutputTarget::Folder(PathBuf::from("/exports")))
        );
        assert_eq!(preferences.output_directory, "/exports");
    }

    #[test]
    fn app_preferences_v3_fallback_is_not_migrated_as_a_v1_v2_record() {
        let mut storage = MemoryStorage::default();
        eframe::set_value(
            &mut storage,
            "app.preferences.v3",
            &Preferences {
                input_items: vec![InputItem::Folder {
                    path: PathBuf::from("/movies"),
                    recursive: true,
                }],
                input_directory: "/movies".to_owned(),
                output_directory: "/custom-output".to_owned(),
                output_target: Some(OutputTarget::Folder(PathBuf::from("/movies/converted"))),
                output_target_explicit: false,
                existing: ExistingPolicy::Rename,
                skip_existing: true,
                ..Preferences::default()
            },
        );

        let (mut preferences, from_legacy) = load_preferences(&storage);
        assert!(!from_legacy);
        migrate_preferences(&mut preferences, from_legacy);

        assert_eq!(preferences.existing, ExistingPolicy::Rename);
        assert!(preferences.skip_existing);
        assert_eq!(preferences.output_directory, "/custom-output");
        assert!(!preferences.output_target_explicit);
        assert_eq!(
            preferences.output_target,
            Some(OutputTarget::Folder(PathBuf::from("/movies/converted",)))
        );
        assert_eq!(
            effective_output_target(
                preferences.output_target.as_ref(),
                preferences.output_target_explicit,
                &[],
                &preferences.input_items,
                Container::Mkv,
            ),
            OutputTarget::Folder(PathBuf::from("/movies/converted"))
        );
    }

    #[test]
    fn legacy_fields_migrate_only_from_the_legacy_record() {
        let mut current = Preferences {
            input_directory: "/v".to_owned(),
            skip_existing: false,
            ..Preferences::default()
        };
        migrate_preferences(&mut current, false);
        assert!(current.input_items.is_empty());
        assert_eq!(current.existing, ExistingPolicy::Skip);

        migrate_preferences(&mut current, true);
        assert_eq!(
            current.input_items,
            [InputItem::Folder {
                path: PathBuf::from("/v"),
                recursive: true
            }]
        );
        assert_eq!(current.existing, ExistingPolicy::Overwrite);
        assert!(current.input_directory.is_empty());
        assert!(current.skip_existing);
    }

    #[test]
    fn legacy_custom_output_directory_remains_explicit_for_jobs() {
        let mut preferences = Preferences {
            input_directory: "/movies".to_owned(),
            output_directory: "/exports".to_owned(),
            ..Preferences::default()
        };
        migrate_preferences(&mut preferences, true);

        assert!(preferences.output_target_explicit);
        let target = effective_output_target(
            preferences.output_target.as_ref(),
            preferences.output_target_explicit,
            &[PathBuf::from("/movies/film.mkv")],
            &preferences.input_items,
            Container::Mkv,
        );
        assert_eq!(target, OutputTarget::Folder(PathBuf::from("/exports")));
    }

    #[test]
    fn legacy_default_output_directory_remains_inferred() {
        let mut preferences = Preferences {
            input_directory: "/movies".to_owned(),
            output_directory: "/movies/converted".to_owned(),
            ..Preferences::default()
        };
        migrate_preferences(&mut preferences, true);

        assert!(!preferences.output_target_explicit);
        let target = effective_output_target(
            preferences.output_target.as_ref(),
            preferences.output_target_explicit,
            &[PathBuf::from("/movies/film.mkv")],
            &preferences.input_items,
            Container::Mkv,
        );
        assert_eq!(
            target,
            OutputTarget::File(PathBuf::from("/movies/converted/film.mkv"))
        );
    }

    #[test]
    fn clearing_upgraded_sources_does_not_restore_them_after_restart() {
        let mut preferences = Preferences {
            input_directory: "/v".to_owned(),
            ..Preferences::default()
        };
        migrate_preferences(&mut preferences, true);
        clear_source_preferences(&mut preferences);
        migrate_preferences(&mut preferences, false);
        assert!(preferences.input_items.is_empty());
        assert!(preferences.input_directory.is_empty());
    }

    #[test]
    fn skip_choice_after_legacy_upgrade_survives_restart() {
        let mut preferences = Preferences {
            skip_existing: false,
            ..Preferences::default()
        };
        migrate_preferences(&mut preferences, true);
        assert_eq!(preferences.existing, ExistingPolicy::Overwrite);
        preferences.existing = ExistingPolicy::Skip;
        migrate_preferences(&mut preferences, false);
        assert_eq!(preferences.existing, ExistingPolicy::Skip);
    }

    #[test]
    fn inferred_single_file_target_tracks_the_current_selection() {
        let stored = OutputTarget::File(PathBuf::from("/v/converted/a.mkv"));
        let target = |path: &str| {
            effective_output_target(
                Some(&stored),
                false,
                &[PathBuf::from(path)],
                &[InputItem::File(PathBuf::from(path))],
                Container::Mkv,
            )
        };
        assert_eq!(
            target("/v/a.mkv"),
            OutputTarget::File(PathBuf::from("/v/converted/a.mkv"))
        );
        assert_eq!(
            target("/v/b.mkv"),
            OutputTarget::File(PathBuf::from("/v/converted/b.mkv"))
        );
    }
}
