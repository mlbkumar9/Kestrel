use super::chips::LanguageSelection;
use kestrel_core::{
    encoders::EncoderSet,
    inputs::{InputItem, SourceFile},
    legacy::LegacyInstall,
    mkv::{BatchStats, MediaMetadata},
    settings::Preferences,
    tools::Toolchain,
    worker::{FileStatus, JobSpec, WorkerEvent},
};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

#[derive(Clone, Debug)]
pub struct SourceEntry {
    pub source: SourceFile,
    pub size: u64,
    pub metadata: Option<MediaMetadata>,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct QueueJob {
    pub spec: JobSpec,
    pub status: FileStatus,
    pub percent: u32,
    pub detail: Option<String>,
    pub fps: Option<f64>,
    pub speed: Option<f64>,
    pub eta_seconds: Option<f64>,
    pub command_line: Option<String>,
}
impl QueueJob {
    fn new(spec: JobSpec) -> Self {
        Self {
            spec,
            status: FileStatus::Pending,
            percent: 0,
            detail: None,
            fps: None,
            speed: None,
            eta_seconds: None,
            command_line: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct LogLine {
    pub timestamp: SystemTime,
    pub job_id: Option<u64>,
    pub text: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QueuePhase {
    #[default]
    Idle,
    Running,
    Paused,
    Stopping,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueControls {
    pub start: bool,
    pub pause: bool,
    pub resume: bool,
    pub stop: bool,
    pub edit: bool,
}
#[derive(Clone, Debug, Default)]
pub struct BatchState {
    pub phase: QueuePhase,
    pub started_at: Option<SystemTime>,
    pub elapsed_seconds: f64,
    pub cancelled: bool,
    pub job_ids: Vec<u64>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueStats {
    pub total: usize,
    pub pending: usize,
    pub processing: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled: usize,
}
pub struct StateData {
    pub preferences: Preferences,
    pub tools: Toolchain,
    pub encoders: EncoderSet,
    pub tools_ready: bool,
    pub sources: Vec<SourceEntry>,
    pub scanning: bool,
    pub source_stats: BatchStats,
    pub languages: LanguageSelection,
    pub queue: Vec<QueueJob>,
    pub logs: Vec<LogLine>,
    pub batch: BatchState,
    pub legacy_install: Option<LegacyInstall>,
    next_job_id: u64,
    active_job: Option<u64>,
}
impl StateData {
    pub fn new(preferences: Preferences) -> Self {
        let languages = LanguageSelection::new(
            &preferences.audio_languages,
            &preferences.subtitle_languages,
        );
        Self {
            preferences,
            tools: Toolchain::default(),
            encoders: EncoderSet::default(),
            tools_ready: false,
            sources: vec![],
            scanning: false,
            source_stats: BatchStats::default(),
            languages,
            queue: vec![],
            logs: vec![],
            batch: BatchState::default(),
            legacy_install: None,
            next_job_id: 1,
            active_job: None,
        }
    }
    pub fn inputs(&self) -> &[InputItem] {
        &self.preferences.input_items
    }
    pub fn controls(&self) -> QueueControls {
        let idle = self.batch.phase == QueuePhase::Idle;
        QueueControls {
            start: idle
                && self.tools_ready
                && self.queue.iter().any(|j| j.status == FileStatus::Pending),
            pause: self.batch.phase == QueuePhase::Running,
            resume: self.batch.phase == QueuePhase::Paused,
            stop: matches!(self.batch.phase, QueuePhase::Running | QueuePhase::Paused),
            edit: idle,
        }
    }
    pub fn elapsed(&self, now: SystemTime) -> Duration {
        if self.batch.phase == QueuePhase::Idle {
            Duration::from_secs_f64(self.batch.elapsed_seconds.max(0.))
        } else {
            self.batch
                .started_at
                .and_then(|s| now.duration_since(s).ok())
                .unwrap_or_default()
        }
    }
    pub fn queue_stats(&self) -> QueueStats {
        let mut stats = QueueStats {
            total: self.queue.len(),
            ..Default::default()
        };
        for job in &self.queue {
            match job.status {
                FileStatus::Pending => stats.pending += 1,
                FileStatus::Processing => stats.processing += 1,
                FileStatus::Success => stats.succeeded += 1,
                FileStatus::Skipped => stats.skipped += 1,
                FileStatus::Error => stats.failed += 1,
                FileStatus::Cancelled => stats.cancelled += 1,
            }
        }
        stats
    }
    pub fn log(&mut self, job_id: Option<u64>, text: impl Into<String>, timestamp: SystemTime) {
        self.logs.push(LogLine {
            timestamp,
            job_id,
            text: text.into(),
        });
    }
    pub fn enqueue(&mut self, mut spec: JobSpec) -> u64 {
        spec.id = self.next_job_id;
        self.next_job_id += 1;
        let id = spec.id;
        self.queue.push(QueueJob::new(spec));
        id
    }
    pub fn retry(&mut self, id: u64) -> bool {
        if !self.controls().edit {
            return false;
        }
        if let Some(job) = self.queue.iter_mut().find(|j| {
            j.spec.id == id && matches!(j.status, FileStatus::Error | FileStatus::Cancelled)
        }) {
            *job = QueueJob::new(job.spec.clone());
            true
        } else {
            false
        }
    }
    pub fn remove(&mut self, id: u64) -> bool {
        if !self.controls().edit {
            return false;
        }
        let before = self.queue.len();
        self.queue.retain(|j| j.spec.id != id);
        before != self.queue.len()
    }
    pub fn clear_finished(&mut self) -> bool {
        if !self.controls().edit {
            return false;
        }
        let before = self.queue.len();
        self.queue.retain(|j| !finished(j.status));
        before != self.queue.len()
    }
    pub fn begin(&mut self, now: SystemTime) -> bool {
        if !self.controls().start {
            return false;
        }
        self.batch = BatchState {
            phase: QueuePhase::Running,
            started_at: Some(now),
            job_ids: self
                .queue
                .iter()
                .filter(|j| j.status == FileStatus::Pending)
                .map(|j| j.spec.id)
                .collect(),
            ..Default::default()
        };
        self.active_job = None;
        true
    }
    pub fn pause(&mut self) -> bool {
        if !self.controls().pause {
            return false;
        }
        self.batch.phase = QueuePhase::Paused;
        true
    }
    pub fn resume(&mut self) -> bool {
        if !self.controls().resume {
            return false;
        }
        self.batch.phase = QueuePhase::Running;
        true
    }
    pub fn stop(&mut self) -> bool {
        if !self.controls().stop {
            return false;
        }
        self.batch.phase = QueuePhase::Stopping;
        true
    }
    pub fn refresh_source_stats(&mut self) {
        self.source_stats = BatchStats {
            total_files: self.sources.len(),
            total_size: self.sources.iter().map(|s| s.size).sum(),
        };
    }
}
pub fn finished(status: FileStatus) -> bool {
    matches!(
        status,
        FileStatus::Success | FileStatus::Skipped | FileStatus::Error | FileStatus::Cancelled
    )
}

pub fn apply(data: &mut StateData, event: WorkerEvent, now: SystemTime) {
    match event {
        WorkerEvent::JobUpdated {
            job_id,
            status,
            percent,
            detail,
        } => {
            if let Some(job) = data.queue.iter_mut().find(|j| j.spec.id == job_id) {
                if finished(job.status) {
                    return;
                }
                job.status = status;
                job.percent = percent.min(100);
                job.detail = detail.clone();
                if finished(status) {
                    job.eta_seconds = None;
                    job.fps = None;
                    job.speed = None;
                }
                data.active_job = Some(job_id);
                if let Some(detail) = detail {
                    data.log(Some(job_id), detail, now);
                }
            }
        }
        WorkerEvent::FileUpdated {
            index,
            status,
            percent,
            detail,
        } => {
            if let Some(&job_id) = data.batch.job_ids.get(index) {
                apply(
                    data,
                    WorkerEvent::JobUpdated {
                        job_id,
                        status,
                        percent,
                        detail,
                    },
                    now,
                );
            }
        }
        WorkerEvent::Progress {
            job_id,
            percent,
            fps,
            speed,
            eta_seconds,
        } => {
            if let Some(job) = data
                .queue
                .iter_mut()
                .find(|j| j.spec.id == job_id && j.status == FileStatus::Processing)
            {
                job.percent = job.percent.max(percent.min(100));
                job.fps = fps;
                job.speed = speed;
                job.eta_seconds = eta_seconds;
            }
        }
        WorkerEvent::Command {
            job_id, display, ..
        } => {
            if let Some(job) = data.queue.iter_mut().find(|j| j.spec.id == job_id) {
                job.command_line = Some(display);
            }
        }
        WorkerEvent::Log(text) => {
            let job_id = data.active_job;
            data.log(job_id, text, now);
        }
        WorkerEvent::BatchFinished {
            cancelled,
            elapsed_seconds,
        } => {
            for job in &mut data.queue {
                if data.batch.job_ids.contains(&job.spec.id) && !finished(job.status) {
                    job.status = if cancelled {
                        FileStatus::Cancelled
                    } else {
                        FileStatus::Error
                    };
                    job.detail = Some(
                        if cancelled {
                            "Stopped by user"
                        } else {
                            "Worker ended without a job result"
                        }
                        .into(),
                    );
                    job.eta_seconds = None;
                    job.speed = None;
                    job.fps = None;
                }
            }
            data.batch.phase = QueuePhase::Idle;
            data.batch.elapsed_seconds = if elapsed_seconds.is_finite() {
                elapsed_seconds.max(0.)
            } else {
                0.
            };
            data.batch.cancelled = cancelled;
            data.active_job = None;
        }
        WorkerEvent::ScanFiles { files, .. } => {
            data.sources = files
                .into_iter()
                .map(|f| SourceEntry {
                    source: SourceFile {
                        relative: PathBuf::from(&f.name),
                        path: f.path,
                    },
                    size: f.size,
                    metadata: f.metadata,
                    error: None,
                })
                .collect();
            data.refresh_source_stats();
        }
        WorkerEvent::ScanLanguages {
            metadata, error, ..
        } => {
            for (path, metadata) in metadata {
                if let Some(source) = data.sources.iter_mut().find(|s| s.source.path == path) {
                    source.metadata = Some(metadata);
                    source.error = None;
                }
            }
            data.scanning = false;
            if let Some(error) = error {
                data.log(None, error, now);
            }
        }
        WorkerEvent::ScanFailed { error, .. } => {
            data.scanning = false;
            data.log(None, error, now);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::transcode::JobSettings;
    fn job() -> JobSpec {
        JobSpec {
            execution: None,
            id: 99,
            input: "input.mkv".into(),
            output: "output.mkv".into(),
            metadata: MediaMetadata::default(),
            settings: JobSettings::default(),
        }
    }
    fn data() -> StateData {
        let mut d = StateData::new(Preferences::default());
        d.tools_ready = true;
        d
    }
    const NOW: SystemTime = SystemTime::UNIX_EPOCH;
    #[test]
    fn controls_and_retry_remove_transitions() {
        let mut d = data();
        assert!(!d.begin(NOW));
        assert!(!d.pause());
        assert!(!d.retry(1));
        assert!(!d.remove(1));
        let id = d.enqueue(job());
        assert!(d.begin(NOW));
        assert!(!d.begin(NOW));
        assert!(!d.remove(id));
        assert!(d.pause());
        assert!(d.controls().resume);
        assert!(d.resume());
        assert!(d.stop());
        assert!(!d.stop());
        apply(
            &mut d,
            WorkerEvent::BatchFinished {
                cancelled: true,
                elapsed_seconds: 2.,
            },
            NOW,
        );
        assert_eq!(d.queue[0].status, FileStatus::Cancelled);
        assert_eq!(d.elapsed(NOW), Duration::from_secs(2));
        assert!(d.retry(id));
        assert_eq!(d.queue[0].status, FileStatus::Pending);
        assert!(d.remove(id));
        assert!(d.queue.is_empty());
    }
    #[test]
    fn progress_commands_and_terminal_status_are_preserved() {
        let mut d = data();
        let id = d.enqueue(job());
        d.begin(NOW);
        apply(
            &mut d,
            WorkerEvent::JobUpdated {
                job_id: id,
                status: FileStatus::Processing,
                percent: 0,
                detail: None,
            },
            NOW,
        );
        apply(
            &mut d,
            WorkerEvent::Command {
                job_id: id,
                executable: "ffmpeg".into(),
                arguments: vec![],
                display: "ffmpeg -i input.mkv output.mkv".into(),
            },
            NOW,
        );
        apply(
            &mut d,
            WorkerEvent::Log("input.mkv: ffmpeg -i input.mkv output.mkv".into()),
            NOW,
        );
        apply(
            &mut d,
            WorkerEvent::Progress {
                job_id: id,
                percent: 250,
                fps: Some(24.),
                speed: Some(2.),
                eta_seconds: Some(3.),
            },
            NOW,
        );
        assert_eq!(d.queue[0].percent, 100);
        assert_eq!(d.logs[0].job_id, Some(id));
        assert_eq!(d.logs[0].timestamp, NOW);
        assert_eq!(
            d.queue[0].command_line.as_deref(),
            Some("ffmpeg -i input.mkv output.mkv")
        );
        apply(
            &mut d,
            WorkerEvent::JobUpdated {
                job_id: id,
                status: FileStatus::Success,
                percent: 100,
                detail: None,
            },
            NOW,
        );
        apply(
            &mut d,
            WorkerEvent::Progress {
                job_id: id,
                percent: 1,
                fps: None,
                speed: None,
                eta_seconds: Some(100.),
            },
            NOW,
        );
        assert_eq!(d.queue[0].status, FileStatus::Success);
        assert_eq!(d.queue[0].eta_seconds, None);
        apply(
            &mut d,
            WorkerEvent::BatchFinished {
                cancelled: false,
                elapsed_seconds: 4.,
            },
            NOW,
        );
        assert!(!d.retry(id));
        assert!(d.clear_finished());
        assert!(d.queue.is_empty());
    }
    #[test]
    fn settings_snapshots_and_unknown_events() {
        let mut d = data();
        let mut snapshot = job();
        snapshot.execution = Some(kestrel_core::worker::JobExecution {
            remux_engine: kestrel_core::worker::RemuxEngine::Ffmpeg,
            existing: kestrel_core::inputs::ExistingPolicy::Rename,
            dry_run: true,
            output_target: kestrel_core::inputs::OutputTarget::File("original.mkv".into()),
        });
        let id = d.enqueue(snapshot);
        d.preferences.remux_engine = kestrel_core::worker::RemuxEngine::MkvToolNix;
        d.preferences.existing = kestrel_core::inputs::ExistingPolicy::Overwrite;
        d.preferences.dry_run = false;
        d.preferences.output_target =
            Some(kestrel_core::inputs::OutputTarget::Folder("changed".into()));
        d.preferences.current_settings.title = Some("Changed".into());
        assert_eq!(d.queue[0].spec.settings.title, None);
        let execution = d.queue[0].spec.execution.as_ref().unwrap();
        assert_eq!(
            execution.remux_engine,
            kestrel_core::worker::RemuxEngine::Ffmpeg
        );
        assert_eq!(
            execution.existing,
            kestrel_core::inputs::ExistingPolicy::Rename
        );
        assert!(execution.dry_run);
        assert_eq!(
            execution.output_target,
            kestrel_core::inputs::OutputTarget::File("original.mkv".into())
        );
        apply(
            &mut d,
            WorkerEvent::JobUpdated {
                job_id: 999,
                status: FileStatus::Error,
                percent: 0,
                detail: Some("unknown".into()),
            },
            NOW,
        );
        assert_eq!(d.queue_stats().pending, 1);
        assert!(d.logs.is_empty());
        d.begin(NOW);
        apply(
            &mut d,
            WorkerEvent::BatchFinished {
                cancelled: false,
                elapsed_seconds: f64::NAN,
            },
            NOW,
        );
        assert_eq!(d.queue[0].status, FileStatus::Error);
        assert!(d.retry(id));
        assert!(d.queue[0].command_line.is_none());
    }
    #[test]
    fn scan_errors_and_empty_metadata() {
        let mut d = data();
        d.scanning = true;
        apply(
            &mut d,
            WorkerEvent::ScanFailed {
                directory: "missing".into(),
                error: "Not found".into(),
            },
            NOW,
        );
        assert!(!d.scanning);
        assert_eq!(d.logs[0].text, "Not found");
        assert_eq!(d.source_stats.total_files, 0);
        assert_eq!(d.elapsed(NOW), Duration::ZERO);
    }
}
