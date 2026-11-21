//! View model of the Metrics page: the files to compare, the metric choices, the run in flight
//! and the results table.

use std::path::PathBuf;
use std::sync::Arc;

use kestrel_core::metrics::{
    FileReport, MODEL_4K, MODEL_HD, MetricRange, MetricsControl, MetricsError, MetricsOutcome,
    MetricsProgress, MetricsSettings, VmafModel,
};
use serde::{Deserialize, Serialize};

/// VMAF model choices of the select; index 0 lets Kestrel pick by the reference's size.
pub const MODELS: [&str; 3] = ["Auto", MODEL_HD, MODEL_4K];
pub const DEFAULT_WORST_FRAMES: usize = 5;
pub const MAX_WORST_FRAMES: usize = 50;

/// The Metrics page choices that persist. Every field has a default, so settings files written
/// before the page existed (4.0) load unchanged.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct MetricsPrefs {
    pub vmaf: bool,
    pub psnr: bool,
    pub ssim: bool,
    pub xpsnr: bool,
    /// Index into [`MODELS`].
    pub model: usize,
    pub worst_frames: usize,
}

impl Default for MetricsPrefs {
    fn default() -> Self {
        Self {
            vmaf: true,
            psnr: true,
            ssim: true,
            xpsnr: true,
            model: 0,
            worst_frames: DEFAULT_WORST_FRAMES,
        }
    }
}

impl MetricsPrefs {
    /// Stored values clamped to what the page can show.
    pub fn sanitized(mut self) -> Self {
        self.model = self.model.min(MODELS.len() - 1);
        self.worst_frames = self.worst_frames.clamp(1, MAX_WORST_FRAMES);
        self
    }

    pub fn vmaf_model(&self) -> VmafModel {
        match self.model {
            0 => VmafModel::Auto,
            index => VmafModel::Named(MODELS[index.min(MODELS.len() - 1)].to_owned()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetricsPhase {
    Idle,
    Running,
    Done,
    Failed(String),
}

/// One row of the results table, already formatted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultRow {
    pub name: String,
    pub frames: String,
    pub vmaf_mean: String,
    pub vmaf_min: String,
    pub vmaf_low: String,
    pub psnr: String,
    pub ssim: String,
    pub xpsnr: String,
    /// Why the file could not be measured.
    pub error: Option<String>,
}

fn number(value: Option<f64>, decimals: usize) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value:.decimals$}"))
}

impl ResultRow {
    pub fn of(report: &FileReport) -> Self {
        let name = report.name();
        match &report.outcome {
            Err(error) => Self {
                name,
                frames: "-".to_owned(),
                vmaf_mean: "-".to_owned(),
                vmaf_min: "-".to_owned(),
                vmaf_low: "-".to_owned(),
                psnr: "-".to_owned(),
                ssim: "-".to_owned(),
                xpsnr: "-".to_owned(),
                error: Some(error.clone()),
            },
            Ok(metrics) => {
                let summary = &metrics.summary;
                let vmaf = summary.vmaf.as_ref();
                Self {
                    name,
                    frames: summary.frames.to_string(),
                    vmaf_mean: number(vmaf.map(|v| v.mean), 2),
                    vmaf_min: number(vmaf.map(|v| v.min), 2),
                    vmaf_low: number(vmaf.map(|v| v.p1), 2),
                    psnr: number(summary.psnr.as_ref().map(|s| s.mean), 2),
                    ssim: number(summary.ssim.as_ref().map(|s| s.mean), 4),
                    xpsnr: number(summary.xpsnr.as_ref().map(|s| s.mean), 2),
                    error: None,
                }
            }
        }
    }
}

/// Range from the two text fields, in seconds. Both empty (or 0 and empty) means the whole clip.
pub fn parse_range(start: &str, duration: &str) -> Result<Option<MetricRange>, String> {
    let seconds = |text: &str, what: &str| -> Result<Option<f64>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        text.parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(Some)
            .ok_or_else(|| format!("The range {what} must be a number of seconds"))
    };
    let start = seconds(start, "start")?.unwrap_or(0.0);
    let duration = seconds(duration, "duration")?.filter(|value| *value > 0.0);
    Ok((start > 0.0 || duration.is_some()).then_some(MetricRange {
        start_seconds: start,
        duration_seconds: duration,
    }))
}

/// File names for the per-frame CSVs, `<stem>_frames.csv`, made unique when two sources share a
/// stem (`a.mkv` and `a.mp4`).
pub fn csv_file_names(names: &[String]) -> Vec<String> {
    let mut used: Vec<String> = Vec::new();
    names
        .iter()
        .map(|name| {
            let stem = std::path::Path::new(name)
                .file_stem()
                .map_or_else(|| name.clone(), |stem| stem.to_string_lossy().into_owned());
            let mut candidate = format!("{stem}_frames.csv");
            let mut copy = 1;
            while used.contains(&candidate) {
                copy += 1;
                candidate = format!("{stem}_frames_{copy}.csv");
            }
            used.push(candidate.clone());
            candidate
        })
        .collect()
}

pub struct MetricsState {
    pub reference: Option<PathBuf>,
    pub distorted: Vec<PathBuf>,
    pub prefs: MetricsPrefs,
    pub range_start: String,
    pub range_duration: String,
    pub phase: MetricsPhase,
    /// Progress of each file of the current run, 0..=1.
    pub file_progress: Vec<f32>,
    pub overall: f32,
    pub outcome: Option<MetricsOutcome>,
    pub show_log: bool,
    /// Bumped by every start, so a late event of an old run is recognised.
    run: u64,
    control: Option<Arc<MetricsControl>>,
}

impl Default for MetricsState {
    fn default() -> Self {
        Self {
            reference: None,
            distorted: Vec::new(),
            prefs: MetricsPrefs::default(),
            range_start: String::new(),
            range_duration: String::new(),
            phase: MetricsPhase::Idle,
            file_progress: Vec::new(),
            overall: 0.0,
            outcome: None,
            show_log: false,
            run: 0,
            control: None,
        }
    }
}

impl Drop for MetricsState {
    /// Closing the window stops a running measurement.
    fn drop(&mut self) {
        self.cancel();
    }
}

impl MetricsState {
    pub fn is_running(&self) -> bool {
        self.phase == MetricsPhase::Running
    }

    /// Files that may be measured: at least one metric is on and both sides are chosen.
    pub fn can_run(&self) -> bool {
        let prefs = &self.prefs;
        self.reference.is_some()
            && !self.distorted.is_empty()
            && (prefs.vmaf || prefs.psnr || prefs.ssim || prefs.xpsnr)
    }

    pub fn has_results(&self) -> bool {
        self.outcome
            .as_ref()
            .is_some_and(|outcome| outcome.files.iter().any(|file| file.outcome.is_ok()))
    }

    /// Adds files that are not the reference and not listed yet. Returns how many were added.
    pub fn add_distorted(&mut self, paths: Vec<PathBuf>) -> usize {
        let before = self.distorted.len();
        for path in paths {
            if self.reference.as_ref() != Some(&path) && !self.distorted.contains(&path) {
                self.distorted.push(path);
            }
        }
        self.distorted.len() - before
    }

    pub fn remove_distorted(&mut self, index: usize) {
        if !self.is_running() && index < self.distorted.len() {
            self.distorted.remove(index);
        }
    }

    pub fn set_reference(&mut self, path: PathBuf) {
        self.distorted.retain(|file| *file != path);
        self.reference = Some(path);
    }

    /// The core settings for a run, or what is missing.
    pub fn settings(&self) -> Result<MetricsSettings, String> {
        let reference = self
            .reference
            .clone()
            .ok_or_else(|| "Choose a reference file".to_owned())?;
        let mut settings = MetricsSettings::new(reference, self.distorted.clone());
        let prefs = &self.prefs;
        settings.vmaf = prefs.vmaf;
        settings.psnr = prefs.psnr;
        settings.ssim = prefs.ssim;
        settings.xpsnr = prefs.xpsnr;
        settings.model = prefs.vmaf_model();
        settings.range = parse_range(&self.range_start, &self.range_duration)?;
        settings.validate()?;
        Ok(settings)
    }

    /// Starts a run: earlier results are dropped. Returns the run number and its cancel handle.
    pub fn begin(&mut self) -> (u64, Arc<MetricsControl>) {
        self.run += 1;
        let control = Arc::new(MetricsControl::default());
        self.control = Some(control.clone());
        self.phase = MetricsPhase::Running;
        self.file_progress = vec![0.0; self.distorted.len()];
        self.overall = 0.0;
        self.outcome = None;
        (self.run, control)
    }

    pub fn progress(&mut self, run: u64, progress: MetricsProgress) {
        if run != self.run || !self.is_running() {
            return;
        }
        for (index, slot) in self.file_progress.iter_mut().enumerate() {
            *slot = match index.cmp(&progress.file_index) {
                std::cmp::Ordering::Less => 1.0,
                std::cmp::Ordering::Equal => progress.file_fraction as f32,
                std::cmp::Ordering::Greater => 0.0,
            };
        }
        self.overall = progress.overall() as f32;
    }

    /// Records the end of a run. Returns the message to show, if any. The result of a run that
    /// was replaced meanwhile is ignored.
    pub fn finish(
        &mut self,
        run: u64,
        result: Result<MetricsOutcome, MetricsError>,
    ) -> Option<String> {
        if run != self.run || !self.is_running() {
            return None;
        }
        self.control = None;
        match result {
            Ok(outcome) => {
                let failed = outcome
                    .files
                    .iter()
                    .filter(|file| file.outcome.is_err())
                    .count();
                self.file_progress.iter_mut().for_each(|slot| *slot = 1.0);
                self.overall = 1.0;
                self.outcome = Some(outcome);
                self.phase = MetricsPhase::Done;
                (failed > 0).then(|| format!("{failed} file(s) could not be measured"))
            }
            Err(MetricsError::Cancelled) => {
                self.phase = MetricsPhase::Idle;
                self.file_progress.clear();
                self.overall = 0.0;
                None
            }
            Err(MetricsError::Failed(error)) => {
                self.phase = MetricsPhase::Failed(error.clone());
                Some(error)
            }
        }
    }

    /// Stops the run in flight. Its end arrives as a cancelled finish.
    pub fn cancel(&mut self) {
        if let Some(control) = &self.control {
            control.cancel();
        }
    }

    pub fn rows(&self) -> Vec<ResultRow> {
        self.outcome
            .as_ref()
            .map(|outcome| outcome.files.iter().map(ResultRow::of).collect())
            .unwrap_or_default()
    }

    /// Every FFmpeg command line of the last run, one per line.
    pub fn command_log(&self) -> String {
        self.outcome
            .as_ref()
            .map(|outcome| outcome.commands.join("\n"))
            .unwrap_or_default()
    }

    /// Adds command lines of an export to the log of the finished run `run`.
    pub fn log_commands(&mut self, run: u64, commands: Vec<String>) {
        if run == self.run
            && let Some(outcome) = self.outcome.as_mut()
        {
            outcome.commands.extend(commands);
        }
    }

    pub fn run_number(&self) -> u64 {
        self.run
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::metrics::{FileMetrics, FrameRow, summarize};

    fn report(name: &str, scores: &[f64]) -> FileReport {
        let rows: Vec<FrameRow> = scores
            .iter()
            .enumerate()
            .map(|(frame, score)| FrameRow {
                frame,
                vmaf: Some(*score),
                psnr: Some(score / 2.0),
                ssim: Some(0.9),
                xpsnr: None,
            })
            .collect();
        let summary = summarize(&rows, Some(MODEL_HD.to_owned()));
        FileReport {
            distorted: PathBuf::from(name),
            outcome: Ok(FileMetrics { rows, summary }),
        }
    }

    fn outcome(files: Vec<FileReport>) -> MetricsOutcome {
        MetricsOutcome {
            reference: PathBuf::from("ref.mkv"),
            range: None,
            files,
            commands: vec!["ffmpeg a".to_owned()],
        }
    }

    #[test]
    fn csv_names_follow_the_stem_and_stay_unique() {
        let names = ["a.mkv", "b.mp4", "a.mp4"].map(str::to_owned);
        assert_eq!(
            csv_file_names(&names),
            ["a_frames.csv", "b_frames.csv", "a_frames_2.csv"]
        );
    }

    #[test]
    fn range_text_becomes_seconds_or_no_range() {
        assert_eq!(parse_range("", ""), Ok(None));
        assert_eq!(parse_range("0", ""), Ok(None));
        assert_eq!(parse_range("0", "0"), Ok(None));
        assert_eq!(
            parse_range(" 12.5 ", "30"),
            Ok(Some(MetricRange {
                start_seconds: 12.5,
                duration_seconds: Some(30.0)
            }))
        );
        assert_eq!(
            parse_range("", "10"),
            Ok(Some(MetricRange {
                start_seconds: 0.0,
                duration_seconds: Some(10.0)
            }))
        );
        assert_eq!(
            parse_range("5", ""),
            Ok(Some(MetricRange {
                start_seconds: 5.0,
                duration_seconds: None
            }))
        );
        assert!(parse_range("abc", "").unwrap_err().contains("start"));
        assert!(parse_range("", "-3").unwrap_err().contains("duration"));
    }

    #[test]
    fn prefs_map_to_the_model_and_clamp_stored_values() {
        let mut prefs = MetricsPrefs::default();
        assert_eq!(prefs.vmaf_model(), VmafModel::Auto);
        prefs.model = 2;
        assert_eq!(prefs.vmaf_model(), VmafModel::Named(MODEL_4K.to_owned()));
        let wild = MetricsPrefs {
            model: 99,
            worst_frames: 0,
            ..MetricsPrefs::default()
        }
        .sanitized();
        assert_eq!((wild.model, wild.worst_frames), (2, 1));
        let wild = MetricsPrefs {
            worst_frames: 1000,
            ..MetricsPrefs::default()
        }
        .sanitized();
        assert_eq!(wild.worst_frames, MAX_WORST_FRAMES);
    }

    #[test]
    fn files_are_deduplicated_and_never_include_the_reference() {
        let mut state = MetricsState::default();
        state.set_reference(PathBuf::from("ref.mkv"));
        let added = state.add_distorted(vec![
            PathBuf::from("a.mkv"),
            PathBuf::from("ref.mkv"),
            PathBuf::from("a.mkv"),
            PathBuf::from("b.mkv"),
        ]);
        assert_eq!(added, 2);
        assert_eq!(state.distorted.len(), 2);
        // Choosing a listed file as the reference takes it out of the list.
        state.set_reference(PathBuf::from("b.mkv"));
        assert_eq!(state.distorted, [PathBuf::from("a.mkv")]);
        state.remove_distorted(0);
        assert!(state.distorted.is_empty() && !state.can_run());
    }

    #[test]
    fn settings_carry_the_page_choices() {
        let mut state = MetricsState::default();
        assert_eq!(state.settings().unwrap_err(), "Choose a reference file");
        state.set_reference(PathBuf::from("ref.mkv"));
        assert!(state.settings().unwrap_err().contains("at least one file"));
        state.add_distorted(vec![PathBuf::from("a.mkv")]);
        state.prefs.xpsnr = false;
        state.prefs.model = 1;
        state.range_start = "2".to_owned();
        let settings = state.settings().unwrap();
        assert!(settings.vmaf && settings.psnr && settings.ssim && !settings.xpsnr);
        assert_eq!(settings.model, VmafModel::Named(MODEL_HD.to_owned()));
        assert_eq!(settings.range.unwrap().start_seconds, 2.0);
        state.prefs.vmaf = false;
        state.prefs.psnr = false;
        state.prefs.ssim = false;
        assert!(!state.can_run());
        assert!(
            state
                .settings()
                .unwrap_err()
                .contains("at least one metric")
        );
    }

    #[test]
    fn a_run_goes_from_progress_to_results() {
        let mut state = MetricsState::default();
        state.set_reference(PathBuf::from("ref.mkv"));
        state.add_distorted(vec![PathBuf::from("a.mkv"), PathBuf::from("b.mkv")]);
        let (run, control) = state.begin();
        assert!(state.is_running() && !control.is_cancelled());
        state.progress(
            run,
            MetricsProgress {
                file_index: 1,
                file_count: 2,
                file_fraction: 0.5,
            },
        );
        assert_eq!(state.file_progress, [1.0, 0.5]);
        assert_eq!(state.overall, 0.75);
        // Progress of another run is ignored.
        state.progress(
            run + 1,
            MetricsProgress {
                file_index: 0,
                file_count: 2,
                file_fraction: 0.1,
            },
        );
        assert_eq!(state.file_progress, [1.0, 0.5]);
        let message = state.finish(
            run,
            Ok(outcome(vec![
                report("a.mkv", &[80.0, 90.0]),
                FileReport {
                    distorted: PathBuf::from("b.mkv"),
                    outcome: Err("FFmpeg failed: bad".to_owned()),
                },
            ])),
        );
        assert_eq!(message.as_deref(), Some("1 file(s) could not be measured"));
        assert_eq!(state.phase, MetricsPhase::Done);
        assert!(state.has_results());
        let rows = state.rows();
        assert_eq!(rows[0].name, "a.mkv");
        assert_eq!(rows[0].vmaf_mean, "85.00");
        assert_eq!(rows[0].vmaf_min, "80.00");
        assert_eq!(rows[0].vmaf_low, "80.10");
        assert_eq!(rows[0].psnr, "42.50");
        assert_eq!(rows[0].ssim, "0.9000");
        assert_eq!(rows[0].xpsnr, "-");
        assert_eq!(rows[1].error.as_deref(), Some("FFmpeg failed: bad"));
        assert_eq!(state.command_log(), "ffmpeg a");
        state.log_commands(run, vec!["ffmpeg b".to_owned()]);
        state.log_commands(run + 5, vec!["ignored".to_owned()]);
        assert_eq!(state.command_log(), "ffmpeg a\nffmpeg b");
        // Starting again drops the old results.
        state.begin();
        assert!(state.outcome.is_none() && state.rows().is_empty());
    }

    #[test]
    fn cancel_stops_the_run_and_failure_keeps_its_message() {
        let mut state = MetricsState::default();
        state.set_reference(PathBuf::from("ref.mkv"));
        state.add_distorted(vec![PathBuf::from("a.mkv")]);
        let (run, control) = state.begin();
        state.cancel();
        assert!(control.is_cancelled());
        assert!(state.is_running(), "the worker still has to report");
        assert_eq!(state.finish(run, Err(MetricsError::Cancelled)), None);
        assert_eq!(state.phase, MetricsPhase::Idle);
        let (run, _) = state.begin();
        let message = state.finish(
            run,
            Err(MetricsError::Failed("FFmpeg was not found".to_owned())),
        );
        assert_eq!(message.as_deref(), Some("FFmpeg was not found"));
        assert_eq!(
            state.phase,
            MetricsPhase::Failed("FFmpeg was not found".to_owned())
        );
        // A late result of the finished run changes nothing.
        assert_eq!(state.finish(run, Err(MetricsError::Cancelled)), None);
        assert!(matches!(state.phase, MetricsPhase::Failed(_)));
    }
}
