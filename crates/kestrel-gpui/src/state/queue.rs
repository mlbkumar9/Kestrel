//! Queue jobs and the reducers that fold worker events into them.

use std::path::PathBuf;
use std::time::Instant;

use kestrel_core::mkv::MediaMetadata;
use kestrel_core::transcode::JobSettings;
use kestrel_core::worker::{FileStatus, WorkerEvent};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobKind {
    Remux,
    Transcode,
}

impl JobKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Remux => "Remux",
            Self::Transcode => "Transcode",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobStatus {
    Queued,
    Encoding,
    Done,
    Skipped,
    Failed(String),
    Cancelled,
}

impl JobStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Encoding => "Encoding",
            Self::Done => "Done",
            Self::Skipped => "Skipped",
            Self::Failed(_) => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Queued | Self::Encoding)
    }
}

#[derive(Clone, Debug)]
pub struct QueueJob {
    pub id: u64,
    pub name: String,
    pub input: PathBuf,
    pub output: PathBuf,
    pub kind: JobKind,
    pub settings: JobSettings,
    pub metadata: MediaMetadata,
    pub summary: String,
    pub status: JobStatus,
    pub percent: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveStats {
    pub fps: Option<f64>,
    pub speed: Option<f64>,
    pub eta_seconds: Option<f64>,
}

/// What to do when the queue finishes.
pub const WHEN_DONE: [&str; 3] = ["Nothing", "Notify", "Open output folder"];

#[derive(Debug, Default)]
pub struct QueueState {
    pub jobs: Vec<QueueJob>,
    next_id: u64,
    pub running: bool,
    pub paused: bool,
    pub live: LiveStats,
    pub started: Option<Instant>,
    pub when_done: usize,
}

impl QueueState {
    pub fn add(
        &mut self,
        input: PathBuf,
        output: PathBuf,
        kind: JobKind,
        settings: JobSettings,
        metadata: MediaMetadata,
        summary: String,
    ) -> u64 {
        self.next_id += 1;
        let name = input
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.jobs.push(QueueJob {
            id: self.next_id,
            name,
            input,
            output,
            kind,
            settings,
            metadata,
            summary,
            status: JobStatus::Queued,
            percent: 0,
        });
        self.next_id
    }

    /// Jobs that are running or waiting, shown as the sidebar badge.
    pub fn active_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|job| job.status.is_active())
            .count()
    }

    pub fn queued(&self) -> Vec<&QueueJob> {
        self.jobs
            .iter()
            .filter(|job| job.status == JobStatus::Queued)
            .collect()
    }

    pub fn current(&self) -> Option<&QueueJob> {
        self.jobs
            .iter()
            .find(|job| job.status == JobStatus::Encoding)
    }

    /// Position of the running job among the jobs of this run, as `(index, total)`.
    pub fn position(&self) -> Option<(usize, usize)> {
        let current = self
            .jobs
            .iter()
            .position(|job| job.status == JobStatus::Encoding)?;
        let total = self
            .jobs
            .iter()
            .filter(|job| job.status != JobStatus::Cancelled)
            .count();
        Some((current + 1, total))
    }

    pub fn find_mut(&mut self, id: u64) -> Option<&mut QueueJob> {
        self.jobs.iter_mut().find(|job| job.id == id)
    }

    pub fn remove(&mut self, id: u64) {
        self.jobs
            .retain(|job| job.id != id || job.status == JobStatus::Encoding);
    }

    pub fn retry(&mut self, id: u64) {
        if let Some(job) = self.find_mut(id)
            && matches!(
                job.status,
                JobStatus::Failed(_) | JobStatus::Cancelled | JobStatus::Skipped
            )
        {
            job.status = JobStatus::Queued;
            job.percent = 0;
        }
    }

    pub fn clear_finished(&mut self) {
        self.jobs.retain(|job| job.status.is_active());
    }

    pub fn begin_run(&mut self) {
        self.running = true;
        self.paused = false;
        self.started = Some(Instant::now());
        self.live = LiveStats::default();
    }

    pub fn elapsed_seconds(&self) -> f64 {
        self.started
            .map_or(0.0, |started| started.elapsed().as_secs_f64())
    }

    /// Folds one worker event into the queue. Returns log text for events that carry any.
    pub fn apply(&mut self, event: &WorkerEvent) -> Option<String> {
        match event {
            WorkerEvent::JobUpdated {
                job_id,
                status,
                percent,
                detail,
            } => {
                let detail = detail.clone();
                if let Some(job) = self.find_mut(*job_id) {
                    job.percent = (*percent).min(100);
                    job.status = match status {
                        FileStatus::Pending => JobStatus::Queued,
                        FileStatus::Processing => JobStatus::Encoding,
                        FileStatus::Success => {
                            job.percent = 100;
                            JobStatus::Done
                        }
                        FileStatus::Skipped => JobStatus::Skipped,
                        FileStatus::Error => {
                            JobStatus::Failed(detail.clone().unwrap_or_else(|| "Failed".into()))
                        }
                        FileStatus::Cancelled => JobStatus::Cancelled,
                    };
                    let name = job.name.clone();
                    if matches!(status, FileStatus::Error | FileStatus::Skipped) {
                        return detail.map(|text| format!("{name}: {text}"));
                    }
                }
                None
            }
            WorkerEvent::Progress {
                job_id,
                percent,
                fps,
                speed,
                eta_seconds,
            } => {
                if let Some(job) = self.find_mut(*job_id) {
                    job.percent = (*percent).min(100);
                    job.status = JobStatus::Encoding;
                }
                self.live = LiveStats {
                    fps: *fps,
                    speed: *speed,
                    eta_seconds: *eta_seconds,
                };
                None
            }
            WorkerEvent::Log(text) => Some(text.clone()),
            WorkerEvent::BatchFinished { cancelled, .. } => {
                self.running = false;
                self.paused = false;
                self.live = LiveStats::default();
                if *cancelled {
                    for job in &mut self.jobs {
                        if job.status.is_active() {
                            job.status = JobStatus::Cancelled;
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_with_jobs(count: usize) -> QueueState {
        let mut queue = QueueState::default();
        for index in 0..count {
            queue.add(
                PathBuf::from(format!("in{index}.mkv")),
                PathBuf::from(format!("out{index}.mkv")),
                JobKind::Transcode,
                JobSettings::default(),
                MediaMetadata::default(),
                "x265".to_owned(),
            );
        }
        queue
    }

    #[test]
    fn badge_counts_active_and_queued_jobs() {
        let mut queue = queue_with_jobs(3);
        assert_eq!(queue.active_count(), 3);
        queue.apply(&WorkerEvent::JobUpdated {
            job_id: 1,
            status: FileStatus::Success,
            percent: 100,
            detail: None,
        });
        queue.apply(&WorkerEvent::JobUpdated {
            job_id: 2,
            status: FileStatus::Error,
            percent: 0,
            detail: Some("boom".to_owned()),
        });
        assert_eq!(queue.active_count(), 1);
        assert_eq!(queue.jobs[1].status, JobStatus::Failed("boom".to_owned()));
    }

    #[test]
    fn progress_updates_the_running_job_and_live_stats() {
        let mut queue = queue_with_jobs(2);
        queue.apply(&WorkerEvent::Progress {
            job_id: 2,
            percent: 58,
            fps: Some(71.4),
            speed: Some(1.9),
            eta_seconds: Some(862.0),
        });
        assert_eq!(queue.current().map(|job| job.id), Some(2));
        assert_eq!(queue.jobs[1].percent, 58);
        assert_eq!(queue.live.fps, Some(71.4));
        assert_eq!(queue.position(), Some((2, 2)));
    }

    #[test]
    fn cancelled_batch_cancels_waiting_jobs_and_retry_requeues() {
        let mut queue = queue_with_jobs(2);
        queue.begin_run();
        queue.apply(&WorkerEvent::BatchFinished {
            cancelled: true,
            elapsed_seconds: 1.0,
        });
        assert!(!queue.running);
        assert_eq!(queue.jobs[0].status, JobStatus::Cancelled);
        queue.retry(1);
        assert_eq!(queue.jobs[0].status, JobStatus::Queued);
        queue.remove(2);
        assert_eq!(queue.jobs.len(), 1);
    }

    #[test]
    fn running_jobs_are_not_removed() {
        let mut queue = queue_with_jobs(1);
        queue.jobs[0].status = JobStatus::Encoding;
        queue.remove(1);
        assert_eq!(queue.jobs.len(), 1);
    }
}
