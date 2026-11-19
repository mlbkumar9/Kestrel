pub mod chips;
pub mod reducer;
pub mod storage;

use async_channel::Sender;
use chips::{LanguageChip, TrackKind};
use gpui::{Context, Task};
use kestrel_core::{
    encoders::{self, EncoderSet},
    inputs::{self, InputItem, OutputTarget},
    legacy::LegacyInstall,
    mkv,
    settings::{self, Preferences},
    tools::Toolchain,
    worker::{self, JobSpec, QueueSpec, WorkerControl, WorkerEvent},
};
use reducer::{SourceEntry, StateData};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::SystemTime,
};
use storage::{FileStorage, SettingsPaths};

enum Event {
    Ready {
        tools: Toolchain,
        encoders: EncoderSet,
        legacy: Option<LegacyInstall>,
        error: Option<String>,
    },
    Expanded {
        generation: u64,
        sources: Vec<SourceEntry>,
    },
    Probed {
        generation: u64,
        path: PathBuf,
        result: Result<mkv::MediaMetadata, String>,
    },
    ScanFinished {
        generation: u64,
    },
    Worker {
        run: u64,
        event: WorkerEvent,
    },
    WorkerDisconnected {
        run: u64,
    },
}
pub struct AppState {
    data: StateData,
    storage: Option<FileStorage>,
    sender: Sender<Event>,
    control: WorkerControl,
    scan_generation: u64,
    run_generation: u64,
    _events: Task<()>,
}
impl AppState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (storage, preferences, error) =
            match SettingsPaths::discover().and_then(|paths| FileStorage::open(&paths)) {
                Ok((storage, preferences)) => (Some(storage), preferences, None),
                Err(error) => (
                    None,
                    Preferences::default(),
                    Some(format!(
                        "Settings could not be loaded; persistence is disabled: {error}"
                    )),
                ),
            };
        let mut data = StateData::new(preferences);
        if let Some(error) = error {
            data.log(None, error, SystemTime::now());
        }
        let (sender, receiver) = async_channel::unbounded();
        let events = cx.spawn(async move |entity, cx| {
            while let Ok(event) = receiver.recv().await {
                if entity
                    .update(cx, |state, cx| state.receive(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        discover(sender.clone());
        Self {
            data,
            storage,
            sender,
            control: WorkerControl::default(),
            scan_generation: 0,
            run_generation: 0,
            _events: events,
        }
    }
    pub fn data(&self) -> &StateData {
        &self.data
    }
    pub fn settings_path(&self) -> Option<&Path> {
        self.storage.as_ref().map(|s| s.path())
    }
    pub fn update_preferences(
        &mut self,
        change: impl FnOnce(&mut Preferences),
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        let old_inputs = self.data.preferences.input_items.clone();
        let old_removed = self.data.preferences.removed_sources.clone();
        let old_target = self.data.preferences.output_target.clone();
        let old_explicit = self.data.preferences.output_target_explicit;
        let old_container = self.data.preferences.current_settings.container;
        change(&mut self.data.preferences);
        settings::migrate_preferences(&mut self.data.preferences, false);
        self.data.languages.audio =
            mkv::parse_language_field(&self.data.preferences.audio_languages);
        self.data.languages.subtitles =
            mkv::parse_language_field(&self.data.preferences.subtitle_languages);
        let result = self.persist();
        if old_inputs != self.data.preferences.input_items
            || old_removed != self.data.preferences.removed_sources
            || old_target != self.data.preferences.output_target
            || old_explicit != self.data.preferences.output_target_explicit
            || old_container != self.data.preferences.current_settings.container
        {
            self.rescan(cx);
        }
        cx.notify();
        result
    }
    fn persist(&mut self) -> io::Result<()> {
        let result = match &mut self.storage {
            Some(storage) => storage.save(&self.data.preferences),
            None => Err(io::Error::other("Settings persistence is unavailable")),
        };
        if let Err(error) = &result {
            self.data.log(
                None,
                format!("Could not save settings: {error}"),
                SystemTime::now(),
            );
        }
        result
    }
    pub fn add_inputs(&mut self, items: Vec<InputItem>, cx: &mut Context<Self>) -> io::Result<()> {
        self.update_preferences(
            move |prefs| {
                for item in items {
                    settings::add_input_item(prefs, item);
                }
            },
            cx,
        )
    }
    pub fn remove_sources(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) -> io::Result<()> {
        self.update_preferences(|prefs| settings::remove_sources(prefs, paths), cx)
    }
    pub fn clear_sources(&mut self, cx: &mut Context<Self>) -> io::Result<()> {
        self.update_preferences(settings::clear_source_preferences, cx)
    }
    pub fn set_output_target(
        &mut self,
        target: Option<OutputTarget>,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        self.update_preferences(
            move |prefs| {
                prefs.output_directory = match &target {
                    Some(OutputTarget::Folder(p)) => p.to_string_lossy().into_owned(),
                    Some(OutputTarget::File(p)) => p
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .to_string_lossy()
                        .into_owned(),
                    None => String::new(),
                };
                prefs.output_target_explicit = target.is_some();
                prefs.output_target = target;
            },
            cx,
        )
    }
    pub fn language_chips(&self, kind: TrackKind) -> Vec<LanguageChip> {
        self.data.languages.chips(
            kind,
            self.data.sources.iter().filter_map(|s| s.metadata.as_ref()),
        )
    }
    pub fn toggle_language(
        &mut self,
        kind: TrackKind,
        code: &str,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        self.data.languages.toggle(kind, code);
        self.save_languages(cx)
    }
    pub fn add_language(
        &mut self,
        kind: TrackKind,
        code: &str,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        self.data.languages.add(kind, code);
        self.save_languages(cx)
    }
    fn save_languages(&mut self, cx: &mut Context<Self>) -> io::Result<()> {
        self.data.preferences.audio_languages = self.data.languages.audio.join(", ");
        self.data.preferences.subtitle_languages = self.data.languages.subtitles.join(", ");
        settings::migrate_preferences(&mut self.data.preferences, false);
        let result = self.persist();
        cx.notify();
        result
    }
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        self.scan_generation += 1;
        self.data.sources.clear();
        self.data.refresh_source_stats();
        if !self.data.tools_ready {
            cx.notify();
            return;
        }
        let generation = self.scan_generation;
        self.data.scanning = true;
        let preferences = self.data.preferences.clone();
        let tools = self.data.tools.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || scan(preferences, tools, generation, sender));
        cx.notify();
    }
    pub fn enqueue_sources(
        &mut self,
        paths: &[PathBuf],
        target: Option<OutputTarget>,
        cx: &mut Context<Self>,
    ) -> Result<Vec<u64>, String> {
        if self.data.scanning {
            return Err("Wait for source scanning to finish".into());
        }
        if paths.is_empty() {
            return Err("Select at least one source file".into());
        }
        let mut selected = Vec::new();
        for path in paths {
            if selected
                .iter()
                .any(|s: &&SourceEntry| s.source.path == *path)
            {
                continue;
            }
            let source = self
                .data
                .sources
                .iter()
                .find(|s| s.source.path == *path)
                .ok_or_else(|| format!("Source is unavailable: {}", path.display()))?;
            if source.metadata.is_none() {
                return Err(format!("Metadata is unavailable: {}", path.display()));
            }
            selected.push(source);
        }
        let prefs = &self.data.preferences;
        let target = target.unwrap_or_else(|| {
            settings::effective_output_target(
                prefs.output_target.as_ref(),
                prefs.output_target_explicit,
                paths,
                &prefs.input_items,
                prefs.current_settings.container,
            )
        });
        let sources = selected
            .iter()
            .map(|s| s.source.clone())
            .collect::<Vec<_>>();
        let plans = inputs::plan_outputs(&sources, &target, prefs.current_settings.container)?;
        let specs = selected
            .into_iter()
            .zip(plans)
            .map(|(s, p)| JobSpec {
                id: 0,
                input: p.input,
                output: p.output,
                metadata: s.metadata.clone().unwrap(),
                settings: prefs.current_settings.clone(),
            })
            .collect::<Vec<_>>();
        let ids = specs
            .into_iter()
            .map(|spec| self.data.enqueue(spec))
            .collect();
        cx.notify();
        Ok(ids)
    }
    pub fn start_queue(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.data.begin(SystemTime::now()) {
            return false;
        }
        self.run_generation += 1;
        let run = self.run_generation;
        self.control = WorkerControl::default();
        let jobs = self
            .data
            .queue
            .iter()
            .filter(|j| self.data.batch.job_ids.contains(&j.spec.id))
            .map(|j| j.spec.clone())
            .collect();
        let spec = QueueSpec {
            jobs,
            tools: self.data.tools.clone(),
            encoders: self.data.encoders.clone(),
            remux_engine: self.data.preferences.remux_engine,
            existing: self.data.preferences.existing,
            dry_run: self.data.preferences.dry_run,
        };
        let (sender, receiver) = mpsc::channel();
        let events = self.sender.clone();
        worker::start_queue(spec, self.control.clone(), sender, Arc::new(|| {}));
        std::thread::spawn(move || {
            for event in receiver {
                if events.send_blocking(Event::Worker { run, event }).is_err() {
                    return;
                }
            }
            let _ = events.send_blocking(Event::WorkerDisconnected { run });
        });
        cx.notify();
        true
    }
    pub fn pause_queue(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.data.pause() {
            return false;
        }
        self.control.set_paused(true);
        cx.notify();
        true
    }
    pub fn resume_queue(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.data.resume() {
            return false;
        }
        self.control.set_paused(false);
        cx.notify();
        true
    }
    pub fn stop_queue(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.data.stop() {
            return false;
        }
        self.control.stop();
        cx.notify();
        true
    }
    pub fn retry_job(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let changed = self.data.retry(id);
        if changed {
            cx.notify();
        }
        changed
    }
    pub fn remove_job(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let changed = self.data.remove(id);
        if changed {
            cx.notify();
        }
        changed
    }
    pub fn clear_finished(&mut self, cx: &mut Context<Self>) -> bool {
        let changed = self.data.clear_finished();
        if changed {
            cx.notify();
        }
        changed
    }
    pub fn clear_logs(&mut self, cx: &mut Context<Self>) {
        self.data.logs.clear();
        cx.notify();
    }
    fn receive(&mut self, event: Event, cx: &mut Context<Self>) {
        let now = SystemTime::now();
        match event {
            Event::Ready {
                tools,
                encoders,
                legacy,
                error,
            } => {
                self.data.tools = tools;
                self.data.encoders = encoders;
                self.data.legacy_install = legacy;
                self.data.tools_ready = true;
                if let Some(error) = error {
                    self.data.log(None, error, now);
                }
                self.rescan(cx);
            }
            Event::Expanded {
                generation,
                sources,
            } if generation == self.scan_generation => {
                self.data.sources = sources;
                self.data.refresh_source_stats();
            }
            Event::Probed {
                generation,
                path,
                result,
            } if generation == self.scan_generation => {
                if let Some(source) = self.data.sources.iter_mut().find(|s| s.source.path == path) {
                    match result {
                        Ok(metadata) => {
                            source.metadata = Some(metadata);
                            source.error = None;
                        }
                        Err(error) => {
                            source.error = Some(error.clone());
                            self.data
                                .log(None, format!("{}: {error}", path.display()), now);
                        }
                    }
                }
            }
            Event::ScanFinished { generation } if generation == self.scan_generation => {
                self.data.scanning = false
            }
            Event::Worker { run, event } if run == self.run_generation => {
                reducer::apply(&mut self.data, event, now)
            }
            Event::WorkerDisconnected { run }
                if run == self.run_generation
                    && self.data.batch.phase != reducer::QueuePhase::Idle =>
            {
                self.data.log(
                    None,
                    "Queue worker disconnected without a batch result",
                    now,
                );
                let elapsed_seconds = self.data.elapsed(now).as_secs_f64();
                reducer::apply(
                    &mut self.data,
                    WorkerEvent::BatchFinished {
                        cancelled: self.control.is_cancelled(),
                        elapsed_seconds,
                    },
                    now,
                );
            }
            _ => return,
        }
        cx.notify();
    }
}
impl Drop for AppState {
    fn drop(&mut self) {
        self.control.stop();
    }
}

fn discover(sender: Sender<Event>) {
    std::thread::spawn(move || {
        let tools = Toolchain::discover();
        let (encoders, error) = match tools
            .ffmpeg
            .as_ref()
            .map(|tool| encoders::detect(&tool.path))
            .transpose()
        {
            Ok(encoders) => (encoders.unwrap_or_default(), None),
            Err(error) => (EncoderSet::default(), Some(error)),
        };
        #[cfg(windows)]
        let legacy = kestrel_core::legacy::find_legacy_install();
        #[cfg(not(windows))]
        let legacy = None;
        let _ = sender.send_blocking(Event::Ready {
            tools,
            encoders,
            legacy,
            error,
        });
    });
}
fn scan(preferences: Preferences, tools: Toolchain, generation: u64, sender: Sender<Event>) {
    let target = inputs::default_output_target(
        &preferences.input_items,
        preferences.current_settings.container,
    );
    let target = preferences
        .output_target
        .as_ref()
        .filter(|_| preferences.output_target_explicit)
        .unwrap_or(&target);
    let exclusions = inputs::output_exclusions(
        target,
        &preferences.input_items,
        preferences.current_settings.container,
    );
    let sources = inputs::expand(
        &preferences.input_items,
        &exclusions,
        &preferences.removed_sources,
    );
    let entries = sources
        .iter()
        .map(|source| SourceEntry {
            source: source.clone(),
            size: std::fs::metadata(&source.path)
                .map(|m| m.len())
                .unwrap_or(0),
            metadata: None,
            error: None,
        })
        .collect();
    if sender
        .send_blocking(Event::Expanded {
            generation,
            sources: entries,
        })
        .is_err()
    {
        return;
    }
    for source in sources {
        let result = mkv::read_metadata(
            tools.mkvmerge.as_ref().map(|t| t.path.as_path()),
            tools.ffprobe.as_ref().map(|t| t.path.as_path()),
            &source.path,
        );
        if sender
            .send_blocking(Event::Probed {
                generation,
                path: source.path,
                result,
            })
            .is_err()
        {
            return;
        }
    }
    let _ = sender.send_blocking(Event::ScanFinished { generation });
}
