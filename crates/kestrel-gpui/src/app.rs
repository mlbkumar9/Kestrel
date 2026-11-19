//! Root view: application state, background work plumbing and the window layout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use gpui::{
    AnyElement, Context, Entity, ExternalPaths, FocusHandle, Focusable, MouseButton, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use kestrel_core::encoders::{self, EncoderSet};
use kestrel_core::formats::{Container, INPUT_EXTENSIONS};
use kestrel_core::inputs::{self, OutputTarget, PlannedOutput, SourceFile};
use kestrel_core::legacy::LegacyInstall;
use kestrel_core::mkv::{self, MediaMetadata};
use kestrel_core::presets::{self, Preset};
use kestrel_core::tools::Toolchain;
use kestrel_core::transcode::{self, Crop, JobResources, VideoSettings};
use kestrel_core::worker::{self, JobSpec, QueueSpec, WorkerControl, WorkerEvent};

use crate::actions::{
    AddFiles, AddToQueue, Dismiss, GoActivity, GoLibrary, GoMetrics, GoQueue, GoRemux, GoTranscode,
    OpenPalette, PaletteDown, PaletteUp,
};
use crate::palette::{Command, PaletteState};
use crate::state::activity::{ActivityLog, Level};
use crate::state::library::{Library, Probe};
use crate::state::probe::{chapter_names_from_csv, read_chapters};
use crate::state::queue::{JobKind, QueueState};
use crate::state::remux::{AddedFile, AddedKind, RemuxState, core_track_edits};
use crate::state::transcode::{ChapterRow, SubTrackForm, Tab, TranscodeForm};
use crate::state::{AppSettings, Page};
use crate::theme::{Appearance, Theme};
use crate::widgets::TagKind;
use crate::widgets::input::TextInput;

/// What a native picker was opened for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pick {
    Files,
    Folder,
    Subtitle,
    Audio,
    ImportSubtitle,
    RemuxDestination,
    TranscodeFolder,
    Chapters,
}

pub enum UiEvent {
    Probed {
        path: PathBuf,
        result: Box<Result<MediaMetadata, String>>,
        chapters: Vec<ChapterRow>,
    },
    Picked {
        purpose: Pick,
        paths: Vec<PathBuf>,
    },
    ChapterText(String),
    LegacyRemoved(Result<(), String>),
    Encoders(Result<EncoderSet, String>),
    Estimate(Result<u64, String>),
    Notice {
        text: String,
        error: bool,
    },
}

pub struct Toast {
    pub id: u64,
    pub text: String,
    pub kind: TagKind,
}

pub struct SaveDialog {
    pub input: Entity<TextInput>,
}

/// Screenshot and debugging switches read from the environment.
#[derive(Default)]
pub struct DebugOptions {
    pub page: Option<Page>,
    pub appearance: Option<Appearance>,
    pub tab: Option<Tab>,
    pub palette: bool,
    /// 1 queues the loaded files once probed, 2 also starts the queue.
    pub queue: u8,
    /// Retitles the second audio track of each source and makes it the default one, so the
    /// queued remux carries a track edit.
    pub edit: bool,
}

impl DebugOptions {
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok();
        Self {
            page: var("KESTREL_DEBUG_PAGE").and_then(|name| Page::from_name(&name)),
            appearance: var("KESTREL_DEBUG_THEME").and_then(|name| {
                Appearance::ALL
                    .into_iter()
                    .find(|appearance| appearance.label().eq_ignore_ascii_case(&name))
            }),
            tab: var("KESTREL_DEBUG_TAB").and_then(|name| {
                Tab::ALL
                    .into_iter()
                    .find(|tab| tab.label().eq_ignore_ascii_case(&name))
            }),
            palette: var("KESTREL_DEBUG_PALETTE").as_deref() == Some("1"),
            queue: match var("KESTREL_DEBUG_QUEUE").as_deref() {
                Some("1") => 1,
                Some("run") => 2,
                _ => 0,
            },
            edit: var("KESTREL_DEBUG_EDIT").as_deref() == Some("1"),
        }
    }
}

pub struct Kestrel {
    pub focus: FocusHandle,
    pub page: Page,
    pub appearance: Appearance,
    pub system_light: bool,
    pub collapsed: bool,
    pub tools: Toolchain,
    pub encoders: Option<EncoderSet>,
    /// Why the last encoder detection failed, shown in Settings and on jobs that need an encoder.
    pub encoder_error: Option<String>,
    pub presets: Vec<Preset>,
    pub library: Library,
    pub remux: RemuxState,
    /// Library index of the file shown in the Remux tracks table.
    pub remux_file: usize,
    pub form: TranscodeForm,
    pub tab: Tab,
    pub show_command: bool,
    pub measured: Option<u64>,
    pub queue: QueueState,
    pub activity: ActivityLog,
    pub settings: AppSettings,
    pub palette: Option<PaletteState>,
    pub save_dialog: Option<SaveDialog>,
    pub open_menu: Option<SharedString>,
    pub inputs: HashMap<SharedString, Entity<TextInput>>,
    pub toasts: Vec<Toast>,
    pub legacy: Option<LegacyInstall>,
    pub legacy_dismissed: bool,
    legacy_removing: bool,
    /// A queue start was requested while encoder detection was still running.
    start_after_detection: bool,
    debug_queue: u8,
    debug_edit: bool,
    pub control: WorkerControl,
    ui_tx: mpsc::Sender<UiEvent>,
    ui_rx: mpsc::Receiver<UiEvent>,
    worker_rx: Option<mpsc::Receiver<WorkerEvent>>,
    inflight: usize,
    polling: bool,
    next_toast: u64,
    next_preset: u32,
    _appearance_subscription: Option<Subscription>,
}

impl Kestrel {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        paths: Vec<PathBuf>,
        debug: DebugOptions,
    ) -> Self {
        let (ui_tx, ui_rx) = mpsc::channel();
        let tools = Toolchain::discover();
        let system_light = is_light(window.appearance());
        let appearance = debug.appearance.unwrap_or_default();
        let subscription = cx.observe_window_appearance(window, |this, window, cx| {
            this.system_light = is_light(window.appearance());
            this.apply_theme(cx);
            cx.notify();
        });
        let mut app = Self {
            focus: cx.focus_handle(),
            page: debug.page.unwrap_or_default(),
            appearance,
            system_light,
            collapsed: false,
            tools,
            encoders: None,
            encoder_error: None,
            presets: presets::builtins(&EncoderSet::default()),
            library: Library::default(),
            remux: RemuxState::default(),
            remux_file: 0,
            form: TranscodeForm::default(),
            tab: debug.tab.unwrap_or(Tab::Video),
            show_command: false,
            measured: None,
            queue: QueueState::default(),
            activity: ActivityLog::default(),
            settings: AppSettings::default(),
            palette: None,
            save_dialog: None,
            open_menu: None,
            inputs: HashMap::new(),
            toasts: Vec::new(),
            legacy: None,
            legacy_dismissed: false,
            legacy_removing: false,
            start_after_detection: false,
            debug_queue: debug.queue,
            debug_edit: debug.edit,
            control: WorkerControl::default(),
            ui_tx,
            ui_rx,
            worker_rx: None,
            inflight: 0,
            polling: false,
            next_toast: 0,
            next_preset: 0,
            _appearance_subscription: Some(subscription),
        };
        app.apply_theme(cx);
        app.detect_encoders(cx);
        app.detect_legacy();
        if !paths.is_empty() {
            app.add_paths(paths, cx);
        }
        if debug.palette {
            app.open_palette(window, cx);
        }
        window.focus(&app.focus);
        app
    }

    pub fn theme(&self) -> Theme {
        self.appearance.resolve(self.system_light)
    }

    pub fn apply_theme(&self, cx: &mut Context<Self>) {
        cx.set_global(self.theme());
    }

    pub fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        self.appearance = appearance;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        let next = if self.theme().light {
            Appearance::Dark
        } else {
            Appearance::Light
        };
        self.set_appearance(next, cx);
    }

    pub fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.open_menu = None;
        self.palette = None;
        cx.notify();
    }

    pub fn toast(&mut self, text: impl Into<String>, kind: TagKind, cx: &mut Context<Self>) {
        self.next_toast += 1;
        let id = self.next_toast;
        self.toasts.push(Toast {
            id,
            text: text.into(),
            kind,
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            this.update(cx, |this, cx| {
                this.toasts.retain(|toast| toast.id != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // ----- background work -----

    fn busy(&self) -> bool {
        self.inflight > 0 || self.worker_rx.is_some()
    }

    /// Starts the event poller if it is not running. It stops itself when no work is in flight.
    pub fn ensure_poller(&mut self, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let keep_going = this.update(cx, |this, cx| {
                    this.drain(cx);
                    let busy = this.busy();
                    if !busy {
                        this.polling = false;
                    }
                    busy
                });
                if !matches!(keep_going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn drain(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        while let Ok(event) = self.ui_rx.try_recv() {
            changed = true;
            self.on_ui_event(event, cx);
        }
        let mut worker_events = Vec::new();
        let mut disconnected = false;
        if let Some(receiver) = &self.worker_rx {
            loop {
                match receiver.try_recv() {
                    Ok(event) => worker_events.push(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for event in worker_events {
            changed = true;
            self.on_worker_event(event, cx);
        }
        if disconnected {
            self.worker_rx = None;
            self.queue.running = false;
        }
        if changed || self.queue.running {
            cx.notify();
        }
    }

    fn on_ui_event(&mut self, event: UiEvent, cx: &mut Context<Self>) {
        match event {
            UiEvent::Probed {
                path,
                result,
                chapters,
            } => {
                self.inflight = self.inflight.saturating_sub(1);
                let result = *result;
                if let Err(error) = &result {
                    self.activity.push(
                        Level::Warn,
                        format!("Could not read {}: {error}", path.display()),
                        None,
                    );
                }
                self.library.set_probe(&path, result, chapters);
                if self
                    .library
                    .current_source()
                    .is_some_and(|source| source.path == path)
                {
                    self.seed_form();
                }
                self.run_debug_queue(cx);
            }
            UiEvent::Picked { purpose, paths } => {
                self.inflight = self.inflight.saturating_sub(1);
                if !paths.is_empty() {
                    self.on_picked(purpose, paths, cx);
                }
            }
            UiEvent::ChapterText(text) => {
                self.inflight = self.inflight.saturating_sub(1);
                let applied = chapter_names_from_csv(&text)
                    .and_then(|names| self.form.apply_chapter_names(names));
                match applied {
                    Ok(()) => self.toast(
                        format!("Imported {} chapter names", self.form.chapters.len()),
                        TagKind::Ok,
                        cx,
                    ),
                    Err(error) => self.toast(error, TagKind::Warn, cx),
                }
            }
            UiEvent::Encoders(result) => {
                self.inflight = self.inflight.saturating_sub(1);
                match result {
                    Ok(set) => {
                        self.presets
                            .retain(|preset| !preset.builtin || preset.id != "hardware");
                        let hardware = presets::builtins(&set)
                            .into_iter()
                            .find(|preset| preset.id == "hardware");
                        if let Some(preset) = hardware {
                            self.presets
                                .insert(self.presets.iter().filter(|p| p.builtin).count(), preset);
                        }
                        self.encoder_error = None;
                        self.encoders = Some(set);
                    }
                    Err(error) => {
                        let message = format!("Encoder detection failed: {error}");
                        self.activity.push(Level::Warn, message.clone(), None);
                        self.encoder_error = Some(message);
                        self.encoders = Some(EncoderSet::default());
                    }
                }
                if std::mem::take(&mut self.start_after_detection) {
                    self.start_queue(cx);
                }
            }
            UiEvent::LegacyRemoved(result) => {
                self.inflight = self.inflight.saturating_sub(1);
                self.legacy_removing = false;
                match result {
                    Ok(()) => {
                        self.legacy = None;
                        self.toast("Removed the previous installation", TagKind::Ok, cx);
                    }
                    Err(error) => self.toast(error, TagKind::Bad, cx),
                }
            }
            UiEvent::Estimate(result) => {
                self.inflight = self.inflight.saturating_sub(1);
                match result {
                    Ok(bytes) => self.measured = Some(bytes),
                    Err(error) => self.toast(error, TagKind::Warn, cx),
                }
            }
            UiEvent::Notice { text, error } => {
                self.inflight = self.inflight.saturating_sub(1);
                if !text.is_empty() {
                    self.toast(text, if error { TagKind::Bad } else { TagKind::Ok }, cx);
                }
            }
        }
    }

    fn on_worker_event(&mut self, event: WorkerEvent, cx: &mut Context<Self>) {
        let job = self.queue.current().map(|job| job.name.clone());
        let finished = matches!(event, WorkerEvent::BatchFinished { .. });
        if let Some(text) = self.queue.apply(&event) {
            self.activity.push_classified(text, job);
        }
        if let WorkerEvent::BatchFinished {
            cancelled,
            elapsed_seconds,
        } = event
        {
            let text = if cancelled {
                "Queue stopped".to_owned()
            } else {
                format!(
                    "Queue finished in {}",
                    crate::state::format::format_clock(elapsed_seconds)
                )
            };
            self.activity.push(Level::Done, text.clone(), None);
            self.worker_rx = None;
            if self.settings.notify_when_done || self.queue.when_done == 1 {
                self.toast(text, TagKind::Ok, cx);
            }
            if self.queue.when_done == 2
                && let Some(job) = self.queue.jobs.iter().rev().find(|job| job.output.exists())
            {
                cx.reveal_path(&job.output);
            }
        }
        let _ = finished;
    }

    /// Asks for a destination on a worker thread and writes `text` there.
    pub fn export_text(
        &mut self,
        title: &'static str,
        file_name: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let sender = self.ui_tx.clone();
        self.inflight += 1;
        std::thread::spawn(move || {
            let event = match rfd::FileDialog::new()
                .set_title(title)
                .set_file_name(file_name)
                .save_file()
            {
                None => UiEvent::Notice {
                    text: String::new(),
                    error: false,
                },
                Some(path) => match std::fs::write(&path, text) {
                    Ok(()) => UiEvent::Notice {
                        text: format!("Saved {}", path.display()),
                        error: false,
                    },
                    Err(error) => UiEvent::Notice {
                        text: format!("Could not save {}: {error}", path.display()),
                        error: true,
                    },
                },
            };
            let _ = sender.send(event);
        });
        self.ensure_poller(cx);
    }

    // ----- sources -----

    pub fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let added = self.library.add_paths(&paths);
        if added.is_empty() {
            self.toast("No new supported media found", TagKind::Warn, cx);
            return;
        }
        self.toast(format!("Added {} file(s)", added.len()), TagKind::Ok, cx);
        self.probe(added, cx);
        cx.notify();
    }

    fn probe(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let mkvmerge = self.tools.mkvmerge.as_ref().map(|tool| tool.path.clone());
        let ffprobe = self.tools.ffprobe.as_ref().map(|tool| tool.path.clone());
        let sender = self.ui_tx.clone();
        self.inflight += paths.len();
        std::thread::spawn(move || {
            for path in paths {
                let result = mkv::read_metadata(mkvmerge.as_deref(), ffprobe.as_deref(), &path);
                let chapters = match (&result, &ffprobe) {
                    (Ok(meta), Some(ffprobe)) if meta.has_chapters => read_chapters(ffprobe, &path),
                    _ => Vec::new(),
                };
                let _ = sender.send(UiEvent::Probed {
                    path,
                    result: Box::new(result),
                    chapters,
                });
            }
        });
        self.ensure_poller(cx);
    }

    pub fn select_source(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.library.sources.len() {
            self.library.current = index;
            self.measured = None;
            self.seed_form();
            cx.notify();
        }
    }

    /// Refreshes track, chapter and output-name fields from the current source.
    pub fn seed_form(&mut self) {
        let Some(source) = self.library.current_source() else {
            return;
        };
        let Some(meta) = source.metadata().cloned() else {
            return;
        };
        let stem = source.stem();
        let chapters = source.chapters.clone();
        let folder = source
            .path
            .parent()
            .map(|parent| parent.join("converted").to_string_lossy().into_owned())
            .unwrap_or_default();
        self.form.seed_from_source(&meta, &stem, &folder);
        self.form.chapters = chapters;
    }

    pub fn detect_encoders(&mut self, cx: &mut Context<Self>) {
        let Some(ffmpeg) = self.tools.ffmpeg.as_ref().map(|tool| tool.path.clone()) else {
            self.encoders = Some(EncoderSet::default());
            return;
        };
        self.encoders = None;
        self.encoder_error = None;
        let sender = self.ui_tx.clone();
        self.inflight += 1;
        std::thread::spawn(move || {
            let _ = sender.send(UiEvent::Encoders(encoders::detect(&ffmpeg)));
        });
        self.ensure_poller(cx);
    }

    fn detect_legacy(&mut self) {
        #[cfg(debug_assertions)]
        if std::env::var("KESTREL_DEBUG_LEGACY_BANNER").as_deref() == Ok("1") {
            self.legacy = Some(kestrel_core::legacy::debug_sample());
            return;
        }
        self.legacy = kestrel_core::legacy::find_legacy_install();
    }

    /// Debug hook: once every source is probed, queue a remux and an encode for them.
    fn run_debug_queue(&mut self, cx: &mut Context<Self>) {
        if self.debug_queue == 0
            || self.library.sources.is_empty()
            || self
                .library
                .sources
                .iter()
                .any(|source| matches!(source.probe, Probe::Pending))
        {
            return;
        }
        let run = self.debug_queue == 2;
        self.debug_queue = 0;
        if self.debug_edit {
            for (path, meta) in self.ready_selected(cx) {
                if meta.audio_tracks.len() < 2 {
                    continue;
                }
                let first = usize::from(meta.video.is_some() || meta.video_codec.is_some());
                let edits = self.remux.edits_for(&path, &meta);
                edits.tracks[first].default = false;
                edits.tracks[first + 1].default = true;
                edits.tracks[first + 1].title = "Commentary".to_owned();
            }
        }
        self.queue_remux(cx);
        self.form.apply_to_all = true;
        self.queue_transcode(cx);
        if run {
            self.start_queue(cx);
        }
    }

    pub fn legacy_busy(&self) -> bool {
        self.legacy_removing
    }

    /// Uninstalls the detected LangMux install on a worker thread.
    pub fn remove_legacy(&mut self, cx: &mut Context<Self>) {
        let Some(install) = self.legacy.clone() else {
            return;
        };
        if self.legacy_removing {
            return;
        }
        self.legacy_removing = true;
        self.inflight += 1;
        let sender = self.ui_tx.clone();
        std::thread::spawn(move || {
            let result = kestrel_core::legacy::remove_legacy_install(&install);
            if result.is_ok() {
                kestrel_core::legacy::remove_legacy_registry_leftovers();
            }
            let _ = sender.send(UiEvent::LegacyRemoved(result));
        });
        self.ensure_poller(cx);
        cx.notify();
    }

    pub fn redetect_tools(&mut self, cx: &mut Context<Self>) {
        self.tools = Toolchain::discover();
        self.detect_encoders(cx);
        cx.notify();
    }

    // ----- native pickers -----

    pub fn pick(&mut self, purpose: Pick, cx: &mut Context<Self>) {
        let sender = self.ui_tx.clone();
        self.inflight += 1;
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new();
            let paths = match purpose {
                Pick::Files => dialog
                    .set_title("Add media files")
                    .add_filter("Media", INPUT_EXTENSIONS)
                    .pick_files()
                    .unwrap_or_default(),
                Pick::Folder => dialog
                    .set_title("Add a folder")
                    .pick_folder()
                    .into_iter()
                    .collect(),
                Pick::RemuxDestination | Pick::TranscodeFolder => dialog
                    .set_title("Choose output folder")
                    .pick_folder()
                    .into_iter()
                    .collect(),
                Pick::Subtitle => dialog
                    .set_title("Add subtitle file")
                    .add_filter("Subtitles", &["srt", "ass", "ssa", "vtt", "sup"])
                    .pick_files()
                    .unwrap_or_default(),
                Pick::ImportSubtitle => dialog
                    .set_title("Import subtitles")
                    .add_filter("Subtitles", &["srt", "ass", "ssa", "vtt"])
                    .pick_files()
                    .unwrap_or_default(),
                Pick::Audio => dialog
                    .set_title("Add audio file")
                    .add_filter(
                        "Audio",
                        &[
                            "aac", "ac3", "eac3", "dts", "flac", "mka", "m4a", "mp3", "ogg",
                            "opus", "wav", "thd",
                        ],
                    )
                    .pick_files()
                    .unwrap_or_default(),
                Pick::Chapters => dialog
                    .set_title("Import chapters")
                    .add_filter("Chapter list", &["csv", "txt"])
                    .pick_file()
                    .into_iter()
                    .collect(),
            };
            if purpose == Pick::Chapters
                && let Some(path) = paths.first()
            {
                let text = std::fs::read_to_string(path).unwrap_or_default();
                let _ = sender.send(UiEvent::Picked {
                    purpose,
                    paths: Vec::new(),
                });
                let _ = sender.send(UiEvent::ChapterText(text));
                return;
            }
            let _ = sender.send(UiEvent::Picked { purpose, paths });
        });
        // The chapter branch sends two events; account for the second.
        if purpose == Pick::Chapters {
            self.inflight += 1;
        }
        self.ensure_poller(cx);
    }

    fn on_picked(&mut self, purpose: Pick, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        match purpose {
            Pick::Files | Pick::Folder => self.add_paths(paths, cx),
            Pick::RemuxDestination => {
                self.remux.destination = paths[0].to_string_lossy().into_owned();
            }
            Pick::TranscodeFolder => {
                self.form.folder = paths[0].to_string_lossy().into_owned();
            }
            Pick::Subtitle | Pick::Audio => {
                let kind = if purpose == Pick::Audio {
                    AddedKind::Audio
                } else {
                    AddedKind::Subtitle
                };
                let Some(source) = self.library.sources.get(self.remux_file) else {
                    self.toast("Add a source file first", TagKind::Warn, cx);
                    return;
                };
                let (path, meta) = (source.path.clone(), source.metadata().cloned());
                let Some(meta) = meta else {
                    self.toast("The file is still being probed", TagKind::Warn, cx);
                    return;
                };
                let edits = self.remux.edits_for(&path, &meta);
                for file in paths {
                    edits.added.push(AddedFile::new(file, kind));
                }
            }
            Pick::ImportSubtitle => {
                for path in paths {
                    let added = AddedFile::new(path.clone(), AddedKind::Subtitle);
                    self.form.sub_tracks.push(SubTrackForm {
                        source: added.file_name(),
                        kind: added.codec_label(),
                        language: added.language,
                        forced_only: false,
                        burn: false,
                        default: false,
                        offset_ms: 0,
                        external: Some(path),
                    });
                }
            }
            Pick::Chapters => {}
        }
        cx.notify();
    }

    // ----- presets -----

    pub fn load_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(preset) = self.presets.get(index).cloned() {
            self.form.apply_job_settings(&preset.name, &preset.settings);
            self.measured = None;
            cx.notify();
        }
    }

    pub fn load_preset_and_open(&mut self, index: usize, cx: &mut Context<Self>) {
        self.load_preset(index, cx);
        self.go(Page::Transcode, cx);
    }

    pub fn save_preset(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            self.toast("Enter a preset name", TagKind::Warn, cx);
            return;
        }
        self.next_preset += 1;
        let preset = Preset {
            id: format!("user-{}", self.next_preset),
            name: name.to_owned(),
            description: "Saved from the Transcode page".to_owned(),
            builtin: false,
            settings: self.form.to_job_settings(),
        };
        self.form.preset = preset.name.clone();
        self.presets.push(preset);
        self.save_dialog = None;
        self.toast(format!("Saved preset {name}"), TagKind::Ok, cx);
    }

    pub fn selected_preset_index(&self) -> usize {
        self.presets
            .iter()
            .position(|preset| preset.name == self.form.preset)
            .unwrap_or(0)
    }

    // ----- queue -----

    fn ready_selected(&mut self, cx: &mut Context<Self>) -> Vec<(PathBuf, MediaMetadata)> {
        let selected = self.library.selected();
        if selected.is_empty() {
            self.toast("Select files in the Library first", TagKind::Warn, cx);
            return Vec::new();
        }
        let mut ready = Vec::new();
        let mut failure = None;
        for index in selected {
            let source = &self.library.sources[index];
            match &source.probe {
                Probe::Ready(meta) => ready.push((source.path.clone(), (**meta).clone())),
                Probe::Pending => {}
                Probe::Failed(error) => failure = Some(format!("{}: {error}", source.name)),
            }
        }
        if ready.is_empty() {
            let text = failure.unwrap_or_else(|| "Selected files are not readable yet".to_owned());
            self.toast(text, TagKind::Warn, cx);
        }
        ready
    }

    /// Queues one remux job per selected library file. Returns how many were added.
    pub fn queue_remux(&mut self, cx: &mut Context<Self>) -> usize {
        let ready = self.ready_selected(cx);
        let paths: Vec<PathBuf> = ready.iter().map(|(path, _)| path.clone()).collect();
        let planned = plan_outputs_for(&paths, &self.remux.destination, self.remux.container);
        let settings = self.remux.job_settings();
        let summary = self.remux.summary();
        let mut count = 0;
        for ((path, meta), plan) in ready.into_iter().zip(planned) {
            let mut settings = settings.clone();
            if let Some(edits) = self.remux.edits.get(&path) {
                match core_track_edits(&meta, edits) {
                    Ok(track_edits) => settings.track_edits = track_edits,
                    Err(error) => {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                        self.toast(
                            format!("{}: {error}", name.unwrap_or_default()),
                            TagKind::Bad,
                            cx,
                        );
                        continue;
                    }
                }
            }
            self.queue.add(
                path,
                plan.output,
                JobKind::Remux,
                settings,
                meta,
                summary.clone(),
            );
            count += 1;
        }
        if count > 0 {
            self.toast(
                format!("Added {count} remux job(s) to the queue"),
                TagKind::Ok,
                cx,
            );
        }
        count
    }

    /// Queues transcode jobs for the current source, or every selected source when applying to all.
    pub fn queue_transcode(&mut self, cx: &mut Context<Self>) -> usize {
        let targets: Vec<(PathBuf, MediaMetadata)> = if self.form.apply_to_all {
            self.ready_selected(cx)
        } else {
            match self.library.current_source() {
                Some(source) => match source.metadata() {
                    Some(meta) => vec![(source.path.clone(), meta.clone())],
                    None => {
                        self.toast("The source is still being probed", TagKind::Warn, cx);
                        Vec::new()
                    }
                },
                None => {
                    self.toast("Add a source in the Library first", TagKind::Warn, cx);
                    Vec::new()
                }
            }
        };
        let settings = self.form.to_job_settings();
        let summary = format!(
            "{} - {}",
            self.form.video_summary(),
            self.form.container.label()
        );
        let single = !self.form.apply_to_all;
        // The title field is seeded from the current source's file name. When applying to
        // several sources an untouched title follows each file instead of copying that name.
        let seeded_title = self
            .library
            .current_source()
            .and_then(|source| source.path.file_stem())
            .map(|stem| stem.to_string_lossy().trim().to_owned());
        let paths: Vec<PathBuf> = targets.iter().map(|(path, _)| path.clone()).collect();
        let name = (single && !self.form.save_as.trim().is_empty())
            .then(|| self.form.save_as.trim().to_owned());
        let outputs = plan_transcode_outputs(
            &paths,
            &self.form.folder,
            self.form.container,
            name.as_deref(),
        );
        let mut count = 0;
        for ((path, meta), output) in targets.into_iter().zip(outputs) {
            let mut settings = settings.clone();
            if !single && let Some(title) = per_file_title(&settings.title, &seeded_title, &path) {
                settings.title = Some(title);
            }
            self.queue.add(
                path,
                output,
                JobKind::Transcode,
                settings,
                meta,
                summary.clone(),
            );
            count += 1;
        }
        if count > 0 {
            self.toast(
                format!("Added {count} transcode job(s) to the queue"),
                TagKind::Ok,
                cx,
            );
        }
        count
    }

    pub fn start_queue(&mut self, cx: &mut Context<Self>) {
        if self.queue.running {
            self.toast("The queue is already running", TagKind::Info, cx);
            return;
        }
        let mut jobs: Vec<JobSpec> = self
            .queue
            .queued()
            .into_iter()
            .map(|job| JobSpec {
                execution: None,
                id: job.id,
                input: job.input.clone(),
                output: job.output.clone(),
                metadata: job.metadata.clone(),
                settings: job.settings.clone(),
            })
            .collect();
        if jobs.is_empty() {
            self.toast("The queue has no waiting jobs", TagKind::Info, cx);
            return;
        }
        if self.tools.ffmpeg.is_none() && self.tools.mkvmerge.is_none() {
            self.toast("FFmpeg and MKVToolNix were not found", TagKind::Bad, cx);
            return;
        }
        // Detection runs on a worker thread for a few seconds after launch. Starting with an
        // empty set would make every transcode fail with "encoder unavailable", so wait for it.
        let Some(encoders) = self.encoders.clone() else {
            self.start_after_detection = true;
            self.toast(
                "Detecting encoders; the queue starts when detection finishes",
                TagKind::Info,
                cx,
            );
            return;
        };
        // Without a detected encoder list every encode would fail as "unavailable". Say why.
        if let Some(reason) = self.encoder_error.clone() {
            for name in self.queue.fail_jobs_needing_encoder(&reason) {
                self.activity.push(
                    Level::Error,
                    format!("{name}: {reason}"),
                    Some(name.clone()),
                );
            }
            let kept: std::collections::HashSet<u64> =
                self.queue.queued().iter().map(|job| job.id).collect();
            jobs.retain(|job| kept.contains(&job.id));
            if jobs.is_empty() {
                self.toast(reason, TagKind::Bad, cx);
                cx.notify();
                return;
            }
        }
        let spec = QueueSpec {
            jobs,
            tools: self.tools.clone(),
            encoders,
            remux_engine: self.remux.engine,
            existing: self.remux.collision,
            dry_run: self.remux.dry_run,
        };
        let (sender, receiver) = mpsc::channel();
        self.worker_rx = Some(receiver);
        self.queue.begin_run();
        worker::start_queue(spec, self.control.clone(), sender, Arc::new(|| {}));
        self.ensure_poller(cx);
        cx.notify();
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if self.queue.running {
            self.queue.paused = !self.queue.paused;
            self.control.set_paused(self.queue.paused);
        } else {
            self.start_queue(cx);
        }
        cx.notify();
    }

    pub fn stop_queue(&mut self, cx: &mut Context<Self>) {
        self.control.stop();
        cx.notify();
    }

    pub fn measure_estimate(&mut self, cx: &mut Context<Self>) {
        let (Some(ffmpeg), Some(source)) = (
            self.tools.ffmpeg.as_ref().map(|tool| tool.path.clone()),
            self.library.current_source(),
        ) else {
            self.toast("Add a source and install FFmpeg first", TagKind::Warn, cx);
            return;
        };
        let Some(meta) = source.metadata().cloned() else {
            return;
        };
        let input = source.path.clone();
        let settings = self.form.to_job_settings();
        let set = self.encoders.clone().unwrap_or_default();
        let sender = self.ui_tx.clone();
        self.inflight += 1;
        std::thread::spawn(move || {
            let result =
                kestrel_core::analysis::estimate_size(&ffmpeg, &input, &meta, &settings, &set);
            let _ = sender.send(UiEvent::Estimate(result));
        });
        self.ensure_poller(cx);
    }

    /// The FFmpeg command(s) the current form would run for the current source, built by the
    /// same function the worker calls. Files the worker prepares at run time (extracted
    /// subtitles, chapter metadata, pass logs) appear under placeholder names.
    pub fn ffmpeg_preview(&self) -> String {
        let Some(source) = self.library.current_source() else {
            return "Add a source to preview the command.".to_owned();
        };
        let Some(meta) = source.metadata() else {
            return "Reading the source...".to_owned();
        };
        let Some(set) = &self.encoders else {
            return "Detecting encoders...".to_owned();
        };
        let output = Path::new(&self.form.folder).join(&self.form.save_as);
        let mut settings = self.form.to_job_settings();
        let mut notes = Vec::new();
        if let VideoSettings::Encode(encode) = &mut settings.video
            && matches!(encode.crop, Crop::Automatic | Crop::Conservative)
        {
            encode.crop = Crop::None;
            notes.push("# crop is detected when the job runs".to_owned());
        }
        let resources = JobResources {
            text_burn_in_file: Some("subtitle.ass".to_owned()),
            chapters_file: (!settings.chapter_names.is_empty())
                .then(|| PathBuf::from("chapters.ffmetadata")),
            pass_log_prefix: Some(PathBuf::from("pass")),
        };
        match transcode::build_ffmpeg_job_passes(
            &source.path,
            &output,
            meta,
            &settings,
            set,
            &resources,
        ) {
            Ok(passes) => {
                let quote = |arg: &std::ffi::OsString| {
                    let text = arg.to_string_lossy();
                    if text.contains(' ') || text.contains('"') {
                        format!("\"{}\"", text.replace('"', "\\\""))
                    } else {
                        text.into_owned()
                    }
                };
                let mut lines = notes;
                for (index, pass) in passes.iter().enumerate() {
                    if passes.len() > 1 {
                        lines.push(format!("# pass {}", index + 1));
                    }
                    let mut parts = vec!["ffmpeg".to_owned()];
                    parts.extend(pass.iter().map(quote));
                    lines.push(parts.join(" "));
                }
                lines.join(
                    "
",
                )
            }
            Err(error) => error,
        }
    }

    /// Notes from core about the current form and source (dropped streams, missing tracks).
    pub fn transcode_warnings(&self) -> Vec<String> {
        let Some(meta) = self
            .library
            .current_source()
            .and_then(|source| source.metadata())
        else {
            return Vec::new();
        };
        kestrel_core::compat::job_warnings(meta, &self.form.to_job_settings())
    }

    // ----- palette -----

    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(cx);
            input.bare = true;
            input.height = 26.0;
            input.font_size = 14.0;
            input.placeholder("Type a command, file or preset");
            input
        });
        cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            use crate::widgets::input::TextEvent;
            match event {
                TextEvent::Changed => {
                    if let Some(palette) = &mut this.palette {
                        palette.selected = 0;
                    }
                    cx.notify();
                }
                TextEvent::Submit => this.run_selected_palette_item(window, cx),
            }
        })
        .detach();
        window.focus(&input.focus_handle(cx));
        self.palette = Some(PaletteState { input, selected: 0 });
        self.open_menu = None;
        cx.notify();
    }

    pub fn close_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let had = self.palette.is_some() || self.save_dialog.is_some() || self.open_menu.is_some();
        self.palette = None;
        self.save_dialog = None;
        self.open_menu = None;
        window.focus(&self.focus);
        cx.notify();
        had
    }

    pub fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        window.focus(&self.focus);
        match command {
            Command::AddFiles => self.pick(Pick::Files, cx),
            Command::AddFolder => self.pick(Pick::Folder, cx),
            Command::Go(page) => self.go(page, cx),
            Command::Preset(index) => self.load_preset_and_open(index, cx),
            Command::StartQueue => {
                self.go(Page::Queue, cx);
                self.start_queue(cx);
            }
            Command::TogglePause => self.toggle_pause(cx),
            Command::ToggleTheme => self.toggle_theme(cx),
        }
        cx.notify();
    }

    fn run_selected_palette_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = &self.palette else {
            return;
        };
        let query = palette.input.read(cx).text().to_owned();
        let items = crate::palette::items(self, &query);
        if let Some(item) = items.get(palette.selected.min(items.len().saturating_sub(1))) {
            let command = item.command;
            self.run_command(command, window, cx);
        }
    }

    /// Ctrl+Enter: queue work for the page the user is on.
    pub fn add_to_queue_for_page(&mut self, cx: &mut Context<Self>) {
        let added = match self.page {
            Page::Remux | Page::Library => self.queue_remux(cx),
            Page::Transcode => self.queue_transcode(cx),
            _ => 0,
        };
        if added > 0 {
            cx.notify();
        }
    }
}

fn is_light(appearance: gpui::WindowAppearance) -> bool {
    matches!(
        appearance,
        gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
    )
}

/// Collision-free output paths for a batch, in `destination` or a `converted` folder next to the first source.
pub fn plan_outputs_for(
    paths: &[PathBuf],
    destination: &str,
    container: Container,
) -> Vec<PlannedOutput> {
    let sources: Vec<SourceFile> = paths
        .iter()
        .map(|path| SourceFile {
            path: path.clone(),
            relative: path.file_name().map(PathBuf::from).unwrap_or_default(),
        })
        .collect();
    let folder = if destination.trim().is_empty() {
        paths.first().and_then(|path| path.parent()).map_or_else(
            || PathBuf::from("converted"),
            |parent| parent.join("converted"),
        )
    } else {
        PathBuf::from(destination.trim())
    };
    inputs::plan_outputs(&sources, &OutputTarget::Folder(folder), container).unwrap_or_default()
}

/// The file's own stem when `title` is still the one seeded from the current source (compared
/// without surrounding spaces), so an untouched title follows each file of a batch.
fn per_file_title(title: &Option<String>, seeded: &Option<String>, path: &Path) -> Option<String> {
    let current = title.as_deref()?.trim();
    (Some(current) == seeded.as_deref().map(str::trim))
        .then(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .flatten()
}

/// Output path for each source of a Transcode batch. A single source may take the name typed
/// in the form; every other output gets the same collision-free planning as the Remux page, so
/// sources that share a file stem do not overwrite one another. With no folder set, outputs go
/// to a `converted` folder next to their source.
pub fn plan_transcode_outputs(
    paths: &[PathBuf],
    folder: &str,
    container: Container,
    name: Option<&str>,
) -> Vec<PathBuf> {
    let folder = folder.trim();
    let converted = |path: &PathBuf| {
        path.parent()
            .map(|parent| parent.join("converted"))
            .unwrap_or_default()
    };
    if let ([path], Some(name)) = (paths, name) {
        let directory = if folder.is_empty() {
            converted(path)
        } else {
            PathBuf::from(folder)
        };
        return vec![directory.join(name)];
    }
    if !folder.is_empty() {
        return plan_outputs_for(paths, folder, container)
            .into_iter()
            .map(|planned| planned.output)
            .collect();
    }
    // One plan per source folder, so each `converted` folder is planned on its own.
    let mut outputs = vec![PathBuf::new(); paths.len()];
    let mut groups: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        match groups.iter_mut().find(|(known, _)| *known == parent) {
            Some((_, members)) => members.push(index),
            None => groups.push((parent, vec![index])),
        }
    }
    for (_, members) in groups {
        let group: Vec<PathBuf> = members.iter().map(|index| paths[*index].clone()).collect();
        for (index, planned) in members
            .into_iter()
            .zip(plan_outputs_for(&group, "", container))
        {
            outputs[index] = planned.output;
        }
    }
    outputs
}

impl Render for Kestrel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme();
        let page: AnyElement = match self.page {
            Page::Library => self.render_library(window, cx).into_any_element(),
            Page::Remux => self.render_remux(window, cx).into_any_element(),
            Page::Transcode => self.render_transcode(window, cx).into_any_element(),
            Page::Queue => self.render_queue(window, cx).into_any_element(),
            Page::Activity => self.render_activity(window, cx).into_any_element(),
            Page::Metrics => self.render_metrics(window, cx).into_any_element(),
            Page::Settings => self.render_settings(window, cx).into_any_element(),
        };
        let sidebar = self.render_sidebar(window, cx);
        let header = self.render_header(cx);
        let banners = self.render_banners(cx);
        let footer = self.render_footer();
        let palette = self.render_palette(window, cx);
        let save_dialog = self.render_save_dialog(window, cx);
        let toasts = self.render_toasts();
        div()
            .relative()
            .flex()
            .size_full()
            .overflow_hidden()
            .font_family("Inter")
            .text_size(px(13.0))
            .line_height(gpui::relative(1.45))
            .bg(t.bg)
            .text_color(t.text)
            .key_context("Kestrel")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)),
            )
            .on_action(cx.listener(|this, _: &AddFiles, _, cx| this.pick(Pick::Files, cx)))
            .on_action(cx.listener(|this, _: &AddToQueue, _, cx| this.add_to_queue_for_page(cx)))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                this.close_overlays(window, cx);
            }))
            .on_action(cx.listener(|this, _: &GoLibrary, _, cx| this.go(Page::Library, cx)))
            .on_action(cx.listener(|this, _: &GoRemux, _, cx| this.go(Page::Remux, cx)))
            .on_action(cx.listener(|this, _: &GoTranscode, _, cx| this.go(Page::Transcode, cx)))
            .on_action(cx.listener(|this, _: &GoQueue, _, cx| this.go(Page::Queue, cx)))
            .on_action(cx.listener(|this, _: &GoActivity, _, cx| this.go(Page::Activity, cx)))
            .on_action(cx.listener(|this, _: &GoMetrics, _, cx| this.go(Page::Metrics, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if this.open_menu.take().is_some() {
                        cx.notify();
                    }
                    window.focus(&this.focus);
                }),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.add_paths(paths.paths().to_vec(), cx);
            }))
            .child(sidebar)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(t.bg)
                    .child(header)
                    .children(banners)
                    .child(div().flex().flex_1().min_h_0().child(page))
                    .child(footer),
            )
            .children(palette)
            .children(save_dialog)
            .child(toasts)
    }
}

impl Kestrel {
    pub fn palette_move(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(palette) = &self.palette else {
            return;
        };
        let query = palette.input.read(cx).text().to_owned();
        let count = crate::palette::items(self, &query).len();
        if let Some(palette) = &mut self.palette
            && count > 0
        {
            palette.selected = if down {
                (palette.selected + 1) % count
            } else {
                (palette.selected + count - 1) % count
            };
        }
        cx.notify();
    }
}

impl Focusable for Kestrel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

pub fn _unused(_: PaletteUp, _: PaletteDown) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn transcode_outputs_do_not_collide_for_sources_sharing_a_stem() {
        let sources = paths(&[
            "/media/a/movie.mkv",
            "/media/a/movie.mp4",
            "/media/b/movie.mkv",
        ]);
        let outputs = plan_transcode_outputs(&sources, "/out", Container::Mkv, None);
        assert_eq!(
            outputs,
            paths(&["/out/movie.mkv", "/out/movie (2).mkv", "/out/movie (3).mkv"])
        );
        // Without a folder each source folder gets its own `converted` folder, planned alone.
        let outputs = plan_transcode_outputs(&sources, "  ", Container::Mp4, None);
        assert_eq!(
            outputs,
            paths(&[
                "/media/a/converted/movie.mp4",
                "/media/a/converted/movie (2).mp4",
                "/media/b/converted/movie.mp4",
            ])
        );
    }

    #[test]
    fn a_single_source_takes_the_typed_name() {
        let one = paths(&["/media/a/movie.mkv"]);
        assert_eq!(
            plan_transcode_outputs(&one, "/out", Container::Mkv, Some("Final.mkv")),
            paths(&["/out/Final.mkv"])
        );
        assert_eq!(
            plan_transcode_outputs(&one, "", Container::Mkv, Some("Final.mkv")),
            paths(&["/media/a/converted/Final.mkv"])
        );
        assert_eq!(
            plan_transcode_outputs(&one, "", Container::Mkv, None),
            paths(&["/media/a/converted/movie.mkv"])
        );
    }

    #[test]
    fn an_untouched_title_follows_each_file_even_with_stray_spaces() {
        let seeded = Some("Movie One ".to_owned());
        let title = |text: &str| Some(text.to_owned());
        let path = Path::new("/media/Second.mkv");
        assert_eq!(
            per_file_title(&title("Movie One"), &seeded, path),
            Some("Second".to_owned())
        );
        assert_eq!(per_file_title(&title("My own title"), &seeded, path), None);
        assert_eq!(per_file_title(&None, &seeded, path), None);
    }
}
