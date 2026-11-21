//! Persistence: maps the view state onto the stored `Preferences` record and back.
//!
//! The record stays in the format `gpui.ron` always had (see [`super::storage`]), so files
//! written by earlier builds load unchanged. Three things live there:
//!
//! * `Preferences` carries what both apps understand: theme flag, remux languages, engine,
//!   collision policy, destination, the remux snapshot in `current_settings`, and user presets.
//! * [`GpuiExtras`] (a second key in the same file) carries what only this app has: the exact
//!   appearance, the Settings page behaviour values and the Transcode form's last-used settings.
//!   Older builds ignore the key.
//! * Anything else in the loaded record is written back untouched.
//!
//! Saving is driven by the view: [`Persistence::observe`] is told the current [`View`] after
//! each change and says when a write should be scheduled; [`Persistence::flush`] writes
//! atomically. Nothing here polls or runs on a timer.

use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use kestrel_core::formats::Container;
use kestrel_core::inputs::{ExistingPolicy, OutputTarget};
use kestrel_core::mkv::parse_language_field;
use kestrel_core::presets::Preset;
use kestrel_core::settings::{Preferences, Storage};
use kestrel_core::transcode::JobSettings;
use kestrel_core::worker::RemuxEngine;
use serde::{Deserialize, Serialize};

use super::AppSettings;
use super::metrics::MetricsPrefs;
use super::remux::RemuxState;
use super::storage::{FileStorage, SettingsPaths};
use super::transcode::TranscodeForm;

/// Key of [`GpuiExtras`] in the settings file.
pub const EXTRAS_KEY: &str = "kestrel.gpui.v4";

/// Values only this app stores. Every field has a default so older and newer files load.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct GpuiExtras {
    /// `Dark`, `Light` or `System`.
    pub appearance: String,
    pub parallel_jobs: usize,
    pub priority: usize,
    pub notify_when_done: bool,
    pub keep_source_dates: bool,
    /// Name shown for the Transcode form's loaded preset.
    pub transcode_preset: String,
    /// Transcode form as of the last save, without source-bound rows (tracks, imported files,
    /// chapter names, title).
    pub transcode: Option<JobSettings>,
    /// Metrics page: metric toggles, VMAF model and worst-frame count. Absent in 4.0 files.
    #[serde(default)]
    pub metrics: MetricsPrefs,
}

impl Default for GpuiExtras {
    fn default() -> Self {
        let app = AppSettings::default();
        Self {
            appearance: String::new(),
            parallel_jobs: app.parallel_jobs,
            priority: app.priority,
            notify_when_done: app.notify_when_done,
            keep_source_dates: app.keep_source_dates,
            transcode_preset: String::new(),
            transcode: None,
            metrics: MetricsPrefs::default(),
        }
    }
}

/// The remux values that persist.
#[derive(Clone, Debug, PartialEq)]
pub struct RemuxPrefs {
    pub container: Container,
    pub engine: RemuxEngine,
    pub destination: String,
    pub collision: ExistingPolicy,
    pub include_subtitles: bool,
    pub keep_chapters: bool,
    pub keep_metadata: bool,
    pub audio_langs: BTreeSet<String>,
    pub sub_langs: BTreeSet<String>,
}

impl RemuxPrefs {
    pub fn of(state: &RemuxState) -> Self {
        Self {
            container: state.container,
            engine: state.engine,
            destination: state.destination.clone(),
            collision: state.collision,
            include_subtitles: state.include_subtitles,
            keep_chapters: state.keep_chapters,
            keep_metadata: state.keep_metadata,
            audio_langs: state.audio_langs.clone(),
            sub_langs: state.sub_langs.clone(),
        }
    }

    /// Puts the stored values into `state`. Selected languages no source carries show up as
    /// the "+ code" chips, so they are listed as extras too.
    pub fn apply(&self, state: &mut RemuxState) {
        state.container = self.container;
        state.engine = self.engine;
        state.destination.clone_from(&self.destination);
        state.collision = self.collision;
        state.include_subtitles = self.include_subtitles;
        state.keep_chapters = self.keep_chapters;
        state.keep_metadata = self.keep_metadata;
        state.audio_langs.clone_from(&self.audio_langs);
        state.sub_langs.clone_from(&self.sub_langs);
        state.extra_audio = self.audio_langs.iter().cloned().collect();
        state.extra_subs = self.sub_langs.iter().cloned().collect();
    }

    /// The remux snapshot stored in `Preferences::current_settings`.
    fn stored_settings(&self) -> JobSettings {
        let mut settings = JobSettings {
            container: self.container,
            keep_chapters: self.keep_chapters,
            keep_metadata: self.keep_metadata,
            ..JobSettings::default()
        };
        settings.audio.languages = self.audio_langs.iter().cloned().collect();
        settings.subtitles.languages = self.sub_langs.iter().cloned().collect();
        settings.subtitles.include = self.include_subtitles;
        settings
    }
}

/// Everything that is saved, as one comparable value.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// `Dark`, `Light` or `System`.
    pub appearance: String,
    pub app: AppSettings,
    pub remux: RemuxPrefs,
    pub form: TranscodeForm,
    /// User presets only.
    pub presets: Vec<Preset>,
    pub selected_preset_id: String,
    pub legacy_dismissed: bool,
    pub metrics: MetricsPrefs,
}

/// What a loaded file says to apply to the view. `None` or empty means "leave the default".
#[derive(Debug, Default)]
pub struct Restored {
    pub appearance: Option<String>,
    pub app: Option<AppSettings>,
    pub remux: Option<RemuxPrefs>,
    /// Transcode form: preset name and last-used settings.
    pub form: Option<(String, JobSettings)>,
    pub presets: Vec<Preset>,
    pub legacy_dismissed: bool,
    pub metrics: Option<MetricsPrefs>,
}

/// Stored settings as found on disk.
pub struct Loaded {
    preferences: Preferences,
    extras: Option<GpuiExtras>,
    /// No settings file of any app existed: the page defaults stay in force.
    fresh: bool,
}

/// Remux snapshot of stored settings: stream copy only, keeping just the choices the Remux
/// page exposes. Imported transcode presets (an egui encode preset) therefore never turn a
/// remux into an encode, and the stored transcode preferences stay untouched.
pub fn remux_settings(stored: &JobSettings) -> JobSettings {
    let mut settings = JobSettings {
        container: stored.container,
        keep_chapters: stored.keep_chapters,
        keep_metadata: stored.keep_metadata,
        ..JobSettings::default()
    };
    settings.audio.languages = stored.audio.languages.clone();
    settings.subtitles.languages = stored.subtitles.languages.clone();
    settings.subtitles.include = stored.subtitles.include;
    settings
}

impl Loaded {
    fn defaults() -> Self {
        Self {
            preferences: Preferences::default(),
            extras: None,
            fresh: true,
        }
    }

    pub fn restore(&self) -> Restored {
        if self.fresh {
            return Restored::default();
        }
        let prefs = &self.preferences;
        let stored = remux_settings(&prefs.current_settings);
        let destination = match (&prefs.output_target, prefs.output_target_explicit) {
            (Some(OutputTarget::Folder(path)), true) => path.to_string_lossy().into_owned(),
            (Some(OutputTarget::File(path)), true) => path
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let languages =
            |text: &str| -> BTreeSet<String> { parse_language_field(text).into_iter().collect() };
        let remux = RemuxPrefs {
            container: stored.container,
            engine: prefs.remux_engine,
            destination,
            collision: prefs.existing,
            include_subtitles: stored.subtitles.include,
            keep_chapters: stored.keep_chapters,
            keep_metadata: stored.keep_metadata,
            audio_langs: languages(&prefs.audio_languages),
            sub_langs: languages(&prefs.subtitle_languages),
        };
        let extras = self.extras.as_ref();
        let appearance = extras
            .map(|extras| extras.appearance.clone())
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| if prefs.light_theme { "Light" } else { "Dark" }.to_owned());
        Restored {
            appearance: Some(appearance),
            app: extras.map(|extras| AppSettings {
                parallel_jobs: extras.parallel_jobs,
                priority: extras.priority,
                notify_when_done: extras.notify_when_done,
                keep_source_dates: extras.keep_source_dates,
            }),
            remux: Some(remux),
            form: extras.and_then(|extras| {
                extras.transcode.clone().map(|settings| {
                    let name = if extras.transcode_preset.is_empty() {
                        "Last used".to_owned()
                    } else {
                        extras.transcode_preset.clone()
                    };
                    (name, settings)
                })
            }),
            presets: prefs
                .user_presets
                .iter()
                .filter(|preset| !preset.builtin)
                .cloned()
                .collect(),
            legacy_dismissed: prefs.legacy_banner_dismissed,
            metrics: extras.map(|extras| extras.metrics.clone()),
        }
    }
}

/// Owns the settings file and decides when a write is due.
pub struct Persistence {
    storage: Option<FileStorage>,
    /// The record as loaded; saves keep the fields this app does not manage.
    base: Preferences,
    latest: Option<View>,
    dirty: bool,
    read_only: bool,
    /// Record text of the last save (or of the loaded state), to skip writing identical files.
    written: Option<(String, String)>,
}

impl Persistence {
    /// Opens the settings file, importing egui settings on the first run. On any load error
    /// the file is left alone, persistence stays off for the session and the message says so.
    pub fn open(paths: io::Result<SettingsPaths>) -> (Self, Loaded, Option<String>) {
        let opened = paths.and_then(|paths| {
            let fresh = !paths.gpui.exists()
                && !paths.egui.exists()
                && !paths.legacy_egui.iter().any(|path| path.exists());
            FileStorage::open(&paths).map(|(storage, preferences)| (storage, preferences, fresh))
        });
        match opened {
            Ok((storage, preferences, fresh)) => {
                let extras = storage
                    .get_string(EXTRAS_KEY)
                    .and_then(|text| ron::from_str::<GpuiExtras>(&text).ok());
                let persistence = Self {
                    storage: Some(storage),
                    base: preferences.clone(),
                    latest: None,
                    dirty: false,
                    read_only: false,
                    written: None,
                };
                (
                    persistence,
                    Loaded {
                        preferences,
                        extras,
                        fresh,
                    },
                    None,
                )
            }
            Err(error) => (
                Self {
                    storage: None,
                    base: Preferences::default(),
                    latest: None,
                    dirty: false,
                    read_only: false,
                    written: None,
                },
                Loaded::defaults(),
                Some(format!(
                    "Settings could not be loaded; persistence is disabled: {error}"
                )),
            ),
        }
    }

    pub fn enabled(&self) -> bool {
        self.storage.is_some() && !self.read_only
    }

    /// Stops all writes for this session (debug runs must not rewrite the user's settings).
    pub fn set_read_only(&mut self) {
        self.read_only = true;
    }

    pub fn path(&self) -> Option<&Path> {
        self.storage.as_ref().map(FileStorage::path)
    }

    /// Records the current view. Returns true when it differs from the last one seen and no
    /// write was already pending, which is when the caller should schedule a flush.
    pub fn observe(&mut self, view: View) -> bool {
        if !self.enabled() || self.latest.as_ref() == Some(&view) {
            return false;
        }
        if self.latest.is_none() {
            // First look: this is the loaded state, there is nothing to save yet.
            self.written = self.record_texts(&view).ok();
            self.latest = Some(view);
            return false;
        }
        self.latest = Some(view);
        !std::mem::replace(&mut self.dirty, true)
    }

    /// Writes the latest view atomically. Returns whether a file was written; identical
    /// content is not rewritten.
    pub fn flush(&mut self) -> io::Result<bool> {
        if !self.enabled() || !std::mem::take(&mut self.dirty) {
            return Ok(false);
        }
        let Some(view) = self.latest.clone() else {
            return Ok(false);
        };
        let (preferences, texts) = self.records(&view)?;
        if self.written.as_ref() == Some(&texts) {
            return Ok(false);
        }
        let storage = self
            .storage
            .as_mut()
            .ok_or_else(|| io::Error::other("Settings persistence is unavailable"))?;
        storage.set_string(EXTRAS_KEY, texts.1.clone());
        storage.save(&preferences)?;
        self.base = preferences;
        self.written = Some(texts);
        Ok(true)
    }

    fn record_texts(&self, view: &View) -> io::Result<(String, String)> {
        self.records(view).map(|(_, texts)| texts)
    }

    fn records(&self, view: &View) -> io::Result<(Preferences, (String, String))> {
        let mut preferences = self.base.clone();
        let remux = &view.remux;
        let join = |set: &BTreeSet<String>| set.iter().cloned().collect::<Vec<_>>().join(", ");
        preferences.light_theme = view.appearance == "Light";
        preferences.audio_languages = join(&remux.audio_langs);
        preferences.subtitle_languages = join(&remux.sub_langs);
        preferences.remux_engine = remux.engine;
        preferences.existing = remux.collision;
        let destination = remux.destination.trim();
        preferences.output_directory = destination.to_owned();
        preferences.output_target =
            (!destination.is_empty()).then(|| OutputTarget::Folder(destination.into()));
        preferences.output_target_explicit = !destination.is_empty();
        preferences.current_settings = remux.stored_settings();
        preferences.user_presets.clone_from(&view.presets);
        preferences
            .selected_preset_id
            .clone_from(&view.selected_preset_id);
        preferences.legacy_banner_dismissed = view.legacy_dismissed;

        let mut transcode = view.form.to_job_settings();
        transcode.audio.tracks.clear();
        transcode.subtitles.external.clear();
        transcode.chapter_names.clear();
        transcode.title = None;
        let extras = GpuiExtras {
            appearance: view.appearance.clone(),
            parallel_jobs: view.app.parallel_jobs,
            priority: view.app.priority,
            notify_when_done: view.app.notify_when_done,
            keep_source_dates: view.app.keep_source_dates,
            transcode_preset: view.form.preset.clone(),
            transcode: Some(transcode),
            metrics: view.metrics.clone(),
        };
        let preferences_text = ron::ser::to_string(&preferences).map_err(io::Error::other)?;
        let extras_text = ron::ser::to_string(&extras).map_err(io::Error::other)?;
        Ok((preferences, (preferences_text, extras_text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::transcode::{AudioChoice, BurnIn, VideoSettings};
    use std::fs;

    fn paths(root: &Path) -> SettingsPaths {
        SettingsPaths {
            gpui: root.join("config/gpui.ron"),
            egui: root.join("egui/app.ron"),
            legacy_egui: Vec::new(),
        }
    }

    fn view() -> View {
        View {
            appearance: "Dark".to_owned(),
            app: AppSettings::default(),
            remux: RemuxPrefs::of(&RemuxState::default()),
            form: TranscodeForm::default(),
            presets: Vec::new(),
            selected_preset_id: "smaller".to_owned(),
            legacy_dismissed: false,
            metrics: MetricsPrefs::default(),
        }
    }

    #[test]
    fn remux_snapshot_drops_imported_encoding_preset() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.egui.parent().unwrap()).unwrap();
        fs::write(&paths.egui, include_str!("fixtures/egui_encode.ron")).unwrap();
        let (_, prefs) = FileStorage::open(&paths).unwrap();
        let stored = &prefs.current_settings;
        assert!(matches!(stored.video, VideoSettings::Encode(_)));
        assert!(!stored.is_remux_only());

        let remux = remux_settings(stored);
        assert!(remux.is_remux_only());
        assert_eq!(remux.video, VideoSettings::Copy);
        assert_eq!(remux.audio.codec, AudioChoice::Passthrough);
        assert_eq!(remux.subtitles.burn_in, BurnIn::None);
        assert_eq!(remux.container, stored.container);
        assert_eq!(remux.audio.languages, vec!["fra"]);
        assert_eq!(remux.subtitles.languages, vec!["deu"]);
        assert!(!remux.subtitles.include);
        assert!(!remux.keep_chapters);
        assert!(!remux.keep_metadata);
        assert!(matches!(stored.video, VideoSettings::Encode(_)));
    }

    #[test]
    fn egui_import_restores_the_remux_page_and_leaves_the_transcode_form_alone() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.egui.parent().unwrap()).unwrap();
        fs::write(&paths.egui, include_str!("fixtures/egui_encode.ron")).unwrap();
        let (persistence, loaded, error) = Persistence::open(Ok(paths));
        assert!(error.is_none() && persistence.enabled());
        let restored = loaded.restore();
        let remux = restored.remux.unwrap();
        assert_eq!(remux.container, Container::Mp4);
        assert!(!remux.include_subtitles && !remux.keep_chapters && !remux.keep_metadata);
        assert_eq!(remux.audio_langs, BTreeSet::from(["fra".to_owned()]));
        assert_eq!(remux.sub_langs, BTreeSet::from(["deu".to_owned()]));
        assert_eq!(restored.appearance.as_deref(), Some("Dark"));
        assert!(restored.form.is_none() && restored.app.is_none());
        // The stored encode preset must not turn a restored remux into an encode.
        let mut state = RemuxState::default();
        remux.apply(&mut state);
        assert!(state.job_settings().is_remux_only());
        assert_eq!(state.job_settings().video, VideoSettings::Copy);
    }

    #[test]
    fn a_gpui_file_written_by_main_still_loads() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.gpui.parent().unwrap()).unwrap();
        fs::write(&paths.gpui, include_str!("fixtures/main_gpui.ron")).unwrap();
        let (_, loaded, error) = Persistence::open(Ok(paths));
        assert!(error.is_none());
        let restored = loaded.restore();
        assert_eq!(restored.appearance.as_deref(), Some("Light"));
        let remux = restored.remux.unwrap();
        assert_eq!(remux.container, Container::Mp4);
        assert_eq!(remux.engine, RemuxEngine::Ffmpeg);
        assert_eq!(remux.collision, ExistingPolicy::Overwrite);
        assert_eq!(remux.destination, "D:/Out");
        assert!(!remux.include_subtitles && !remux.keep_chapters);
        assert_eq!(
            remux.audio_langs,
            BTreeSet::from(["fra".to_owned(), "jpn".to_owned()])
        );
        assert_eq!(remux.sub_langs, BTreeSet::from(["spa".to_owned()]));
        assert_eq!(restored.presets.len(), 1);
        assert_eq!(restored.presets[0].name, "My x264");
        assert!(matches!(
            restored.presets[0].settings.video,
            VideoSettings::Encode(_)
        ));
        assert!(restored.legacy_dismissed);
        // Main had no extras: the page defaults for those stay.
        assert!(restored.app.is_none() && restored.form.is_none());
    }

    #[test]
    fn saved_state_round_trips_through_the_file() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let (mut persistence, loaded, _) = Persistence::open(Ok(paths.clone()));
        assert!(loaded.fresh && loaded.restore().remux.is_none());
        assert!(!persistence.observe(view()));

        let mut changed = view();
        changed.appearance = "System".to_owned();
        changed.app = AppSettings {
            parallel_jobs: 2,
            priority: 1,
            notify_when_done: false,
            keep_source_dates: false,
        };
        changed.remux = RemuxPrefs {
            container: Container::Mov,
            engine: RemuxEngine::Ffmpeg,
            destination: "E:/Converted".to_owned(),
            collision: ExistingPolicy::Skip,
            include_subtitles: false,
            keep_chapters: false,
            keep_metadata: true,
            audio_langs: BTreeSet::from(["eng".to_owned(), "xyz".to_owned()]),
            sub_langs: BTreeSet::from(["jpn".to_owned()]),
        };
        changed.form.rf = 17;
        changed.form.copy_video = true;
        changed.form.preset = "My preset".to_owned();
        changed.form.tag_title = "Per file title".to_owned();
        changed.presets = vec![Preset {
            id: "user-3".to_owned(),
            name: "My preset".to_owned(),
            description: "Saved from the Transcode page".to_owned(),
            builtin: false,
            settings: changed.form.to_job_settings(),
        }];
        assert!(persistence.observe(changed.clone()));
        assert!(persistence.flush().unwrap());

        let (_, loaded, error) = Persistence::open(Ok(paths));
        assert!(error.is_none());
        let restored = loaded.restore();
        assert_eq!(restored.appearance.as_deref(), Some("System"));
        assert_eq!(restored.app, Some(changed.app.clone()));
        assert_eq!(restored.remux.as_ref(), Some(&changed.remux));
        assert_eq!(restored.presets, changed.presets);
        let (name, settings) = restored.form.unwrap();
        assert_eq!(name, "My preset");
        assert_eq!(settings.video, VideoSettings::Copy);
        // Source-bound values are not stored with the last-used settings.
        assert_eq!(settings.title, None);
        let mut form = TranscodeForm::default();
        form.apply_job_settings(&name, &settings);
        assert!(form.copy_video && form.preset == "My preset");
    }

    #[test]
    fn a_4_0_settings_file_without_the_metrics_fields_still_loads() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.gpui.parent().unwrap()).unwrap();
        fs::write(&paths.gpui, include_str!("fixtures/v4_0_gpui.ron")).unwrap();
        let (_, loaded, error) = Persistence::open(Ok(paths));
        assert!(error.is_none(), "{error:?}");
        let restored = loaded.restore();
        // The 4.0 extras are read in full ...
        assert_eq!(restored.appearance.as_deref(), Some("System"));
        let app = restored.app.expect("the 4.0 extras load");
        assert_eq!((app.parallel_jobs, app.priority), (2, 1));
        assert!(!app.notify_when_done);
        // ... and the Metrics page falls back to its defaults.
        assert_eq!(restored.metrics, Some(MetricsPrefs::default()));
        // The same text parses straight into the extras record.
        let extras: GpuiExtras = ron::from_str(
            r#"(appearance:"Dark",parallel_jobs:0,priority:0,notify_when_done:true,keep_source_dates:true,transcode_preset:"",transcode:None)"#,
        )
        .unwrap();
        assert_eq!(extras.metrics, MetricsPrefs::default());
        // A partial metrics record keeps the defaults of the fields it lacks.
        let partial: GpuiExtras =
            ron::from_str(r#"(metrics:(ssim:false,worst_frames:9))"#).unwrap();
        assert!(partial.metrics.vmaf && partial.metrics.psnr && partial.metrics.xpsnr);
        assert!(!partial.metrics.ssim);
        assert_eq!(partial.metrics.worst_frames, 9);
    }

    #[test]
    fn metrics_choices_round_trip_through_the_file() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let (mut persistence, _, _) = Persistence::open(Ok(paths.clone()));
        assert!(!persistence.observe(view()));
        let mut changed = view();
        changed.metrics = MetricsPrefs {
            vmaf: true,
            psnr: false,
            ssim: true,
            xpsnr: false,
            model: 2,
            worst_frames: 8,
        };
        assert!(persistence.observe(changed.clone()));
        assert!(persistence.flush().unwrap());
        let (_, loaded, _) = Persistence::open(Ok(paths));
        assert_eq!(loaded.restore().metrics, Some(changed.metrics));
    }

    #[test]
    fn writes_are_deduplicated_and_only_follow_changes() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let (mut persistence, _, _) = Persistence::open(Ok(paths.clone()));
        assert!(
            !persistence.observe(view()),
            "the first view is the baseline"
        );
        assert!(!persistence.flush().unwrap(), "nothing changed yet");
        let mut next = view();
        next.remux.container = Container::Mp4;
        assert!(
            persistence.observe(next.clone()),
            "a change schedules a write"
        );
        next.legacy_dismissed = true;
        assert!(
            !persistence.observe(next),
            "a second change joins the pending write"
        );
        assert!(persistence.flush().unwrap());
        assert!(!persistence.flush().unwrap(), "nothing is pending any more");
        // A change reverted before the write is due leaves the file as it is.
        let mut other = view();
        other.remux.container = Container::Mp4;
        other.legacy_dismissed = true;
        other.appearance = "Light".to_owned();
        assert!(persistence.observe(other));
        let mut reverted = view();
        reverted.remux.container = Container::Mp4;
        reverted.legacy_dismissed = true;
        assert!(!persistence.observe(reverted));
        assert!(!persistence.flush().unwrap());
        assert_eq!(
            fs::read_dir(paths.gpui.parent().unwrap()).unwrap().count(),
            1,
            "atomic writes leave no temporary files"
        );
    }

    #[test]
    fn a_corrupt_file_is_kept_and_persistence_is_disabled() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.gpui.parent().unwrap()).unwrap();
        fs::write(&paths.gpui, "this is not ron").unwrap();
        let (mut persistence, loaded, error) = Persistence::open(Ok(paths.clone()));
        let message = error.expect("a load error is reported");
        assert!(message.contains("persistence is disabled"), "{message}");
        assert!(!persistence.enabled());
        assert!(loaded.restore().remux.is_none());
        assert!(!persistence.observe(view()));
        let mut changed = view();
        changed.appearance = "Light".to_owned();
        assert!(!persistence.observe(changed));
        assert!(!persistence.flush().unwrap());
        assert_eq!(fs::read_to_string(&paths.gpui).unwrap(), "this is not ron");
    }

    #[test]
    fn unavailable_settings_directories_disable_persistence() {
        let (persistence, _, error) = Persistence::open(Err(io::Error::other(
            "Configuration directory is unavailable",
        )));
        assert!(!persistence.enabled());
        assert!(error.unwrap().contains("Configuration directory"));
    }

    #[test]
    fn read_only_sessions_never_write() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let (mut persistence, _, _) = Persistence::open(Ok(paths.clone()));
        persistence.set_read_only();
        assert!(!persistence.observe(view()));
        let mut changed = view();
        changed.remux.container = Container::WebM;
        assert!(!persistence.observe(changed));
        assert!(!persistence.flush().unwrap());
    }
}
