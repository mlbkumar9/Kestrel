/// Callback invoked after an event is sent to wake the host event loop.
pub type EventNotifier = std::sync::Arc<dyn Fn() + Send + Sync>;

use crate::encoders::EncoderSet;
use crate::formats::Container;
use crate::inputs::ExistingPolicy;
use crate::mkv::{self, AvailableLanguages, BatchStats, FileInfo, MediaMetadata};
use crate::subtitles::{absolute_path, prepare_text_subtitle};
use crate::tools::{self, Toolchain};
use crate::transcode::{JobResources, JobSettings, build_ffmpeg_job_passes, chapters};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum RemuxEngine {
    #[default]
    MkvToolNix,
    Ffmpeg,
}

impl RemuxEngine {
    pub fn label(self) -> &'static str {
        match self {
            Self::MkvToolNix => "MKVToolNix",
            Self::Ffmpeg => "FFmpeg",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileStatus {
    #[default]
    Pending,
    Processing,
    Success,
    Skipped,
    Error,
    Cancelled,
}

/// Scan and queue updates delivered to the caller over its event channel.
pub enum WorkerEvent {
    ScanFiles {
        directory: PathBuf,
        files: Vec<FileInfo>,
        stats: BatchStats,
    },
    ScanLanguages {
        directory: PathBuf,
        languages: AvailableLanguages,
        metadata: Vec<(PathBuf, MediaMetadata)>,
        error: Option<String>,
    },
    ScanFailed {
        directory: PathBuf,
        error: String,
    },
    FileUpdated {
        index: usize,
        status: FileStatus,
        percent: u32,
        detail: Option<String>,
    },
    JobUpdated {
        job_id: u64,
        status: FileStatus,
        percent: u32,
        detail: Option<String>,
    },
    Progress {
        job_id: u64,
        percent: u32,
        fps: Option<f64>,
        speed: Option<f64>,
        eta_seconds: Option<f64>,
    },
    Command {
        job_id: u64,
        executable: PathBuf,
        arguments: Vec<OsString>,
        display: String,
    },
    Log(String),
    BatchFinished {
        cancelled: bool,
        elapsed_seconds: f64,
    },
}

#[derive(Clone)]
pub struct WorkerControl {
    jobs: Arc<Mutex<JobControl>>,
    paused: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    active_child: Arc<Mutex<Option<Child>>>,
}

#[derive(Default)]
struct JobControl {
    active: Option<u64>,
    cancelled: std::collections::HashSet<u64>,
}

impl Default for WorkerControl {
    fn default() -> Self {
        Self {
            jobs: Default::default(),
            paused: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            active_child: Arc::new(Mutex::new(None)),
        }
    }
}

impl WorkerControl {
    pub fn reset(&self) {
        if let Ok(mut jobs) = self.jobs.lock() {
            *jobs = JobControl::default();
        }
        self.paused.store(false, Ordering::SeqCst);
        self.cancelled.store(false, Ordering::SeqCst);
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.active_child.lock()
            && let Some(child) = guard.as_mut()
        {
            let _ = child.kill();
        }
    }
    pub fn stop_job(&self, id: u64) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.cancelled.insert(id);
            if jobs.active == Some(id)
                && let Ok(mut child) = self.active_child.lock()
                && let Some(child) = child.as_mut()
            {
                let _ = child.kill();
            }
        }
    }
    fn job_stopped(&self, id: u64) -> bool {
        self.jobs
            .lock()
            .is_ok_and(|jobs| jobs.cancelled.contains(&id))
    }
}

pub struct BatchSpec {
    pub files: Vec<FileInfo>,
    pub tools: Toolchain,
    pub engine: RemuxEngine,
    pub output_directory: PathBuf,
    pub audio_languages: Vec<String>,
    pub subtitle_languages: Vec<String>,
    pub skip_existing: bool,
    pub dry_run: bool,
}

#[derive(Clone)]
/// One queue job, including source metadata and an independent settings snapshot.
pub struct JobSpec {
    pub execution: Option<JobExecution>,
    pub id: u64,
    pub input: PathBuf,
    pub output: PathBuf,
    pub metadata: MediaMetadata,
    pub settings: JobSettings,
}

#[derive(Clone, Debug)]
pub struct JobExecution {
    pub remux_engine: RemuxEngine,
    pub existing: ExistingPolicy,
    pub dry_run: bool,
    pub output_target: crate::inputs::OutputTarget,
}

#[derive(Clone)]
/// Ordered jobs and execution policies shared by one queue run.
pub struct QueueSpec {
    pub jobs: Vec<JobSpec>,
    pub tools: Toolchain,
    pub encoders: EncoderSet,
    pub remux_engine: RemuxEngine,
    pub existing: ExistingPolicy,
    pub dry_run: bool,
}

enum ProcessOutcome {
    Success,
    Warnings(WarningSummary),
}

fn send(sender: &mpsc::Sender<WorkerEvent>, ctx: &EventNotifier, event: WorkerEvent) {
    if sender.send(event).is_ok() {
        ctx();
    }
}

pub fn scan_directory(
    directory: PathBuf,
    tools: Toolchain,
    sender: mpsc::Sender<WorkerEvent>,
    ctx: EventNotifier,
) {
    std::thread::spawn(move || {
        let mut files = match mkv::scan_directory(&directory) {
            Ok(files) => files,
            Err(error) => {
                send(&sender, &ctx, WorkerEvent::ScanFailed { directory, error });
                return;
            }
        };
        let stats = mkv::batch_stats(&files);
        send(
            &sender,
            &ctx,
            WorkerEvent::ScanFiles {
                directory: directory.clone(),
                files: files.clone(),
                stats,
            },
        );

        let mut metadata = Vec::new();
        let mut scan_errors = Vec::new();
        for file in &mut files {
            match mkv::read_metadata(
                tools.mkvmerge.as_ref().map(|tool| tool.path.as_path()),
                tools.ffprobe.as_ref().map(|tool| tool.path.as_path()),
                &file.path,
            ) {
                Ok(info) => {
                    file.metadata = Some(info.clone());
                    metadata.push((file.path.clone(), info));
                }
                Err(error) => scan_errors.push(format!("{}: {error}", file.name)),
            }
        }
        let languages = mkv::available_languages(
            &metadata
                .iter()
                .map(|(_, metadata)| metadata.clone())
                .collect::<Vec<_>>(),
        );
        send(
            &sender,
            &ctx,
            WorkerEvent::ScanLanguages {
                directory,
                languages,
                metadata,
                error: (!scan_errors.is_empty()).then(|| scan_errors.join("\n")),
            },
        );
    });
}

pub fn start_batch(
    spec: BatchSpec,
    control: WorkerControl,
    sender: mpsc::Sender<WorkerEvent>,
    ctx: EventNotifier,
) {
    control.reset();
    std::thread::spawn(move || {
        let started = Instant::now();
        send(
            &sender,
            &ctx,
            WorkerEvent::Log(format!(
                "{} batch started for {} file(s)",
                if spec.dry_run { "Dry-run" } else { "Remux" },
                spec.files.len()
            )),
        );
        if !spec.dry_run
            && let Err(error) = std::fs::create_dir_all(&spec.output_directory)
        {
            send(
                &sender,
                &ctx,
                WorkerEvent::Log(format!("Could not create output folder: {error}")),
            );
            send(
                &sender,
                &ctx,
                WorkerEvent::BatchFinished {
                    cancelled: false,
                    elapsed_seconds: started.elapsed().as_secs_f64(),
                },
            );
            return;
        }

        let mut cancelled = false;
        for (index, file) in spec.files.iter().enumerate() {
            while control.paused.load(Ordering::SeqCst) && !control.is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            if control.is_cancelled() {
                cancelled = true;
                mark_cancelled(index, spec.files.len(), &sender, &ctx);
                break;
            }

            let output = mkv::output_path(&spec.output_directory, &file.path);
            if !spec.dry_run
                && let Err(error) = validate_output_paths(&file.path, &output)
            {
                send(
                    &sender,
                    &ctx,
                    WorkerEvent::FileUpdated {
                        index,
                        status: FileStatus::Error,
                        percent: 0,
                        detail: Some(error.clone()),
                    },
                );
                send(
                    &sender,
                    &ctx,
                    WorkerEvent::Log(format!("{} — {error}", file.name)),
                );
                continue;
            }
            let output_exists = std::fs::metadata(&output)
                .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0);
            if should_skip_existing(spec.skip_existing, spec.dry_run, output_exists) {
                send(
                    &sender,
                    &ctx,
                    WorkerEvent::FileUpdated {
                        index,
                        status: FileStatus::Skipped,
                        percent: 100,
                        detail: Some("Output already exists".to_owned()),
                    },
                );
                send(
                    &sender,
                    &ctx,
                    WorkerEvent::Log(format!("Skipped {} (already exists)", file.name)),
                );
                continue;
            }

            send(
                &sender,
                &ctx,
                WorkerEvent::FileUpdated {
                    index,
                    status: FileStatus::Processing,
                    percent: 0,
                    detail: None,
                },
            );

            let result = process_file(index, file, &output, &spec, &control, &sender, &ctx);
            match result {
                Ok(outcome) => {
                    let (detail, warnings) = match outcome {
                        ProcessOutcome::Success => (None, None),
                        ProcessOutcome::Warnings(warnings) => {
                            (Some("Warnings".to_owned()), Some(warnings))
                        }
                    };
                    send(
                        &sender,
                        &ctx,
                        WorkerEvent::FileUpdated {
                            index,
                            status: FileStatus::Success,
                            percent: 100,
                            detail,
                        },
                    );
                    if let Some(warnings) = warnings {
                        send(
                            &sender,
                            &ctx,
                            WorkerEvent::Log(format!("{} finished with warnings", file.name)),
                        );
                        for warning in warnings.lines {
                            send(
                                &sender,
                                &ctx,
                                WorkerEvent::Log(format!("{} — {warning}", file.name)),
                            );
                        }
                        if warnings.omitted > 0 {
                            send(
                                &sender,
                                &ctx,
                                WorkerEvent::Log(format!(
                                    "{} — ...and {} more warnings",
                                    file.name, warnings.omitted
                                )),
                            );
                        }
                    } else {
                        send(
                            &sender,
                            &ctx,
                            WorkerEvent::Log(format!(
                                "{} {}",
                                if spec.dry_run {
                                    "Previewed"
                                } else {
                                    "Finished"
                                },
                                file.name
                            )),
                        );
                    }
                }
                Err(_error) if control.is_cancelled() => {
                    cancelled = true;
                    send(
                        &sender,
                        &ctx,
                        WorkerEvent::FileUpdated {
                            index,
                            status: FileStatus::Cancelled,
                            percent: 0,
                            detail: Some("Stopped by user".to_owned()),
                        },
                    );
                    send(
                        &sender,
                        &ctx,
                        WorkerEvent::Log(format!(
                            "Stopped {} and removed partial output",
                            file.name
                        )),
                    );
                    mark_cancelled(index + 1, spec.files.len(), &sender, &ctx);
                    break;
                }
                Err(error) => {
                    send(
                        &sender,
                        &ctx,
                        WorkerEvent::FileUpdated {
                            index,
                            status: FileStatus::Error,
                            percent: 0,
                            detail: Some(error.clone()),
                        },
                    );
                    send(
                        &sender,
                        &ctx,
                        WorkerEvent::Log(format!("{} — {error}", file.name)),
                    );
                }
            }
        }

        send(
            &sender,
            &ctx,
            WorkerEvent::BatchFinished {
                cancelled,
                elapsed_seconds: started.elapsed().as_secs_f64(),
            },
        );
    });
}

fn mark_cancelled(
    start: usize,
    file_count: usize,
    sender: &mpsc::Sender<WorkerEvent>,
    ctx: &EventNotifier,
) {
    for index in start..file_count {
        send(
            sender,
            ctx,
            WorkerEvent::FileUpdated {
                index,
                status: FileStatus::Cancelled,
                percent: 0,
                detail: Some("Not started".to_owned()),
            },
        );
    }
}

/// Starts a queue on a worker thread and notifies the host after each progress event.
pub fn start_queue(
    spec: QueueSpec,
    control: WorkerControl,
    sender: mpsc::Sender<WorkerEvent>,
    ctx: EventNotifier,
) {
    control.reset();
    std::thread::spawn(move || run_queue(spec, control, sender, ctx));
}

/// Runs a queue on the calling thread and collects all emitted events.
pub fn run_queue_blocking(spec: QueueSpec) -> Vec<WorkerEvent> {
    let (sender, receiver) = mpsc::channel();
    let context: EventNotifier = std::sync::Arc::new(|| {});
    run_queue(spec, WorkerControl::default(), sender, context);
    receiver.into_iter().collect()
}

fn run_queue(
    spec: QueueSpec,
    control: WorkerControl,
    sender: mpsc::Sender<WorkerEvent>,
    ctx: EventNotifier,
) {
    let started = Instant::now();
    send(
        &sender,
        &ctx,
        WorkerEvent::Log(format!(
            "{} batch started for {} job(s)",
            if spec.dry_run {
                "Dry-run"
            } else if spec.jobs.iter().all(|job| job.settings.is_remux_only()) {
                "Remux"
            } else {
                "Transcode"
            },
            spec.jobs.len()
        )),
    );
    let mut cancelled = false;
    for (index, job) in spec.jobs.iter().enumerate() {
        if control.is_cancelled() {
            cancelled = true;
            for not_started in &spec.jobs[index..] {
                send(
                    &sender,
                    &ctx,
                    WorkerEvent::JobUpdated {
                        job_id: not_started.id,
                        status: FileStatus::Cancelled,
                        percent: 0,
                        detail: Some("Not started".to_owned()),
                    },
                );
            }
            break;
        }
        while control.paused.load(Ordering::SeqCst) && !control.is_cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
        if control.job_stopped(job.id) {
            queue_update(
                &sender,
                &ctx,
                job.id,
                FileStatus::Cancelled,
                0,
                Some("Stopped by user".into()),
            );
            continue;
        }
        let policy = execution_spec(&spec, job);
        let mut resolved_job = job.clone();
        resolved_job.settings =
            match crate::transcode::resolve_job_settings(&job.settings, &spec.encoders) {
                Ok(settings) => settings,
                Err(error) => {
                    queue_update(&sender, &ctx, job.id, FileStatus::Error, 0, Some(error));
                    continue;
                }
            };
        if let Some(ffmpeg) = &spec.tools.ffmpeg {
            match crate::analysis::resolve_automatic_settings(
                &ffmpeg.path,
                &job.input,
                &resolved_job.metadata,
                &resolved_job.settings,
            ) {
                Ok((metadata, settings)) => {
                    resolved_job.metadata = metadata;
                    resolved_job.settings = settings;
                }
                Err(error) => {
                    queue_update(&sender, &ctx, job.id, FileStatus::Error, 0, Some(error));
                    continue;
                }
            }
        }
        let job = &resolved_job;
        if !policy.dry_run
            && let Some(parent) = job.output.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            queue_update(
                &sender,
                &ctx,
                job.id,
                FileStatus::Error,
                0,
                Some(error.to_string()),
            );
            continue;
        }
        let output = match queue_output(&job.output, policy.existing, policy.dry_run) {
            Ok(Some(output)) => output,
            Ok(None) => {
                queue_update(
                    &sender,
                    &ctx,
                    job.id,
                    FileStatus::Skipped,
                    100,
                    Some("Output already exists".to_owned()),
                );
                continue;
            }
            Err(error) => {
                queue_update(&sender, &ctx, job.id, FileStatus::Error, 0, Some(error));
                continue;
            }
        };
        if !policy.dry_run
            && let Err(error) = validate_output_paths(&job.input, &output)
        {
            queue_update(&sender, &ctx, job.id, FileStatus::Error, 0, Some(error));
            continue;
        }
        queue_update(&sender, &ctx, job.id, FileStatus::Processing, 0, None);
        for warning in crate::compat::job_warnings(&job.metadata, &job.settings) {
            send(
                &sender,
                &ctx,
                WorkerEvent::Log(format!("{} — {warning}", job.input.display())),
            );
        }
        if let Ok(mut jobs) = control.jobs.lock() {
            jobs.active = Some(job.id);
        }
        let result = process_queue_job(job, &output, &policy, &control, &sender, &ctx);
        if let Ok(mut jobs) = control.jobs.lock() {
            jobs.active = None;
        }
        match result {
            Ok(outcome) => {
                let detail = match &outcome {
                    ProcessOutcome::Success => None,
                    ProcessOutcome::Warnings(_) => Some("Warnings".to_owned()),
                };
                queue_update(&sender, &ctx, job.id, FileStatus::Success, 100, detail);
                if let ProcessOutcome::Warnings(warnings) = outcome {
                    send(
                        &sender,
                        &ctx,
                        WorkerEvent::Log(format!("{} finished with warnings", job.input.display())),
                    );
                    for warning in warnings.lines {
                        send(
                            &sender,
                            &ctx,
                            WorkerEvent::Log(format!("{} — {warning}", job.input.display())),
                        );
                    }
                    if warnings.omitted > 0 {
                        send(
                            &sender,
                            &ctx,
                            WorkerEvent::Log(format!(
                                "{} — ...and {} more warnings",
                                job.input.display(),
                                warnings.omitted
                            )),
                        );
                    }
                }
            }
            Err(error) if control.is_cancelled() => {
                cancelled = true;
                queue_update(
                    &sender,
                    &ctx,
                    job.id,
                    FileStatus::Cancelled,
                    0,
                    Some("Stopped by user".to_owned()),
                );
                send(
                    &sender,
                    &ctx,
                    WorkerEvent::Log(format!("{} — {error}", job.input.display())),
                );
                for not_started in &spec.jobs[index + 1..] {
                    send(
                        &sender,
                        &ctx,
                        WorkerEvent::JobUpdated {
                            job_id: not_started.id,
                            status: FileStatus::Cancelled,
                            percent: 0,
                            detail: Some("Not started".to_owned()),
                        },
                    );
                }
                break;
            }
            Err(_) if control.job_stopped(job.id) => {
                queue_update(
                    &sender,
                    &ctx,
                    job.id,
                    FileStatus::Cancelled,
                    0,
                    Some("Stopped by user".into()),
                );
            }
            Err(error) => {
                queue_update(
                    &sender,
                    &ctx,
                    job.id,
                    FileStatus::Error,
                    0,
                    Some(error.clone()),
                );
                send(
                    &sender,
                    &ctx,
                    WorkerEvent::Log(format!("{} — {error}", job.input.display())),
                );
            }
        }
    }
    send(
        &sender,
        &ctx,
        WorkerEvent::BatchFinished {
            cancelled,
            elapsed_seconds: started.elapsed().as_secs_f64(),
        },
    );
}

fn queue_update(
    sender: &mpsc::Sender<WorkerEvent>,
    ctx: &EventNotifier,
    job_id: u64,
    status: FileStatus,
    percent: u32,
    detail: Option<String>,
) {
    send(
        sender,
        ctx,
        WorkerEvent::JobUpdated {
            job_id,
            status,
            percent,
            detail,
        },
    );
}

fn queue_output(
    output: &Path,
    existing: ExistingPolicy,
    dry_run: bool,
) -> Result<Option<PathBuf>, String> {
    if dry_run || existing != ExistingPolicy::Skip || !output.exists() {
        if existing == ExistingPolicy::Rename && output.exists() {
            let stem = output.file_stem().unwrap_or_default().to_string_lossy();
            let extension = output.extension().unwrap_or_default().to_string_lossy();
            for suffix in 2..1000 {
                let candidate = output.with_file_name(format!("{stem} ({suffix}).{extension}"));
                if !candidate.exists() {
                    return Ok(Some(candidate));
                }
            }
            return Err("Could not find an unused output name".to_owned());
        }
        return Ok(Some(output.to_path_buf()));
    }
    Ok(None)
}

fn process_queue_job(
    job: &JobSpec,
    output: &Path,
    spec: &QueueSpec,
    control: &WorkerControl,
    sender: &mpsc::Sender<WorkerEvent>,
    ctx: &EventNotifier,
) -> Result<ProcessOutcome, String> {
    let prepared_text_subtitle =
        crate::transcode::selected_burn_in_subtitle(&job.metadata, &job.settings)
            .filter(|(_, track)| crate::transcode::is_text_subtitle(&track.codec_id))
            .map(|(_, track)| {
                let ffmpeg = spec
                    .tools
                    .ffmpeg
                    .as_ref()
                    .ok_or_else(|| "FFmpeg is unavailable for subtitle burn-in".to_owned())?;
                let index = track
                    .ffprobe_index
                    .ok_or_else(|| "Subtitle stream index is unavailable".to_owned())?;
                prepare_text_subtitle(&ffmpeg.path, &job.input, index)
            })
            .transpose()?;
    let text_burn_in_file = prepared_text_subtitle
        .as_ref()
        .map(|subtitle| subtitle.filename());
    let prepared_chapters = prepare_chapter_names(job, spec)?;
    let pass_logs = PassLogs::create_if_needed(job)?;
    let resources = JobResources {
        text_burn_in_file: text_burn_in_file.map(str::to_owned),
        chapters_file: prepared_chapters
            .as_ref()
            .map(|chapters| chapters.path().to_path_buf()),
        pass_log_prefix: pass_logs.as_ref().map(PassLogs::prefix),
    };
    if job.settings.is_remux_only() {
        job.settings.track_edits.check_external_files()?;
    }
    let uses_mkvmerge = mkvmerge_engine_eligible(
        &job.input,
        &job.metadata,
        &job.settings,
        spec.remux_engine,
        spec.tools.mkvmerge.is_some(),
    );
    let temporary = if spec.dry_run {
        None
    } else {
        Some(TemporaryOutput::create(output)?)
    };
    let engine_output = temporary
        .as_ref()
        .map(TemporaryOutput::path)
        .unwrap_or(output);

    let mut edit_warnings = Vec::new();
    let (executable, passes, engine) = if uses_mkvmerge {
        let mkvmerge = spec.tools.mkvmerge.as_ref().unwrap();
        let audio_languages = &job.settings.audio.languages;
        let subtitle_languages = &job.settings.subtitles.languages;
        match mkv::build_arguments(
            &job.input,
            engine_output,
            &job.metadata,
            audio_languages,
            subtitle_languages,
            false,
            mkv::MkvRemuxSettings {
                include_subtitles: job.settings.subtitles.include,
                keep_chapters: job.settings.keep_chapters,
                keep_metadata: job.settings.keep_metadata,
                title: job.settings.title.as_deref(),
                track_edits: Some(&job.settings.track_edits),
            },
        ) {
            Ok(Some(arguments)) => (
                mkvmerge.path.clone(),
                vec![arguments],
                RemuxEngine::MkvToolNix,
            ),
            Ok(None) => {
                queue_ffmpeg_arguments(job, engine_output, spec, &resources, &mut edit_warnings)?
            }
            Err(error) if error.contains("track IDs are unavailable") => {
                queue_ffmpeg_arguments(job, engine_output, spec, &resources, &mut edit_warnings)?
            }
            Err(error) => return Err(error),
        }
    } else {
        queue_ffmpeg_arguments(job, engine_output, spec, &resources, &mut edit_warnings)?
    };
    for warning in &edit_warnings {
        send(
            sender,
            ctx,
            WorkerEvent::Log(format!("{}: warning: {warning}", job.input.display())),
        );
    }
    if control.is_cancelled() || control.job_stopped(job.id) {
        return Err("Stopped by user".to_owned());
    }
    for arguments in &passes {
        let command_text = tools::display_command(&executable, arguments);
        send(
            sender,
            ctx,
            WorkerEvent::Command {
                job_id: job.id,
                executable: executable.clone(),
                arguments: arguments.clone(),
                display: command_text.clone(),
            },
        );
        send(
            sender,
            ctx,
            WorkerEvent::Log(format!("{}: {command_text}", job.input.display())),
        );
    }
    if spec.dry_run {
        return Ok(ProcessOutcome::Success);
    }
    let partial = temporary.as_ref().unwrap().path();
    let pass_count = passes.len() as u32;
    let mut run = None;
    for (index, arguments) in passes.iter().enumerate() {
        let index = index as u32;
        let span = 100 / pass_count;
        let result = run_child_process(
            job,
            &executable,
            arguments,
            engine,
            control,
            sender,
            ctx,
            prepared_text_subtitle.is_some(),
            (index * span, span),
        )?;
        if index + 1 < pass_count {
            if !result.0.success() {
                return Err(format!(
                    "{} analysis pass failed: {}",
                    engine.label(),
                    trim_diagnostic(&result.1)
                ));
            }
        } else {
            run = Some(result);
        }
    }
    let (status, diagnostic, warnings) = run.expect("at least one pass runs");
    if control.is_cancelled() || control.job_stopped(job.id) {
        return Err("Stopped by user".to_owned());
    }
    let output_exists =
        std::fs::metadata(partial).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0);
    let outcome = mkv::classify_remux_exit(
        engine == RemuxEngine::MkvToolNix,
        status.code(),
        output_exists,
    );
    if outcome == mkv::RemuxExitOutcome::Failure {
        let diagnostic = trim_diagnostic(&diagnostic);
        return Err(if diagnostic.is_empty() {
            format!(
                "{} failed with exit code {:?}",
                engine.label(),
                status.code()
            )
        } else {
            format!("{} failed: {diagnostic}", engine.label())
        });
    }
    finalize_output(partial, output)?;
    match outcome {
        mkv::RemuxExitOutcome::Success => Ok(ProcessOutcome::Success),
        mkv::RemuxExitOutcome::SuccessWithWarnings => Ok(ProcessOutcome::Warnings(
            warnings
                .lock()
                .map(|warnings| warnings.clone())
                .unwrap_or_default(),
        )),
        mkv::RemuxExitOutcome::Failure => unreachable!(),
    }
}

type ChildRun = (std::process::ExitStatus, String, Arc<Mutex<WarningSummary>>);

/// Runs one FFmpeg or mkvmerge process to completion, reporting progress scaled into
/// `progress.0..progress.0 + progress.1` percent.
#[allow(clippy::too_many_arguments)]
fn run_child_process(
    job: &JobSpec,
    executable: &Path,
    arguments: &[OsString],
    engine: RemuxEngine,
    control: &WorkerControl,
    sender: &mpsc::Sender<WorkerEvent>,
    ctx: &EventNotifier,
    temp_cwd: bool,
    progress: (u32, u32),
) -> Result<ChildRun, String> {
    let mut command = tools::command(executable);
    command.args(arguments);
    if temp_cwd {
        command.current_dir(std::env::temp_dir());
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start {}: {error}", engine.label()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    register_child(control, child)?;

    let warnings = Arc::new(Mutex::new(WarningSummary::default()));
    let output_reader = stdout.map(|stdout| {
        let sender = sender.clone();
        let ctx = ctx.clone();
        let duration = job.metadata.duration;
        let job_id = job.id;
        std::thread::spawn(move || {
            let mut current = QueueProgress::default();
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                current.read(&line, duration);
                if line.starts_with("progress=") || mkv::parse_progress_line(&line).is_some() {
                    send(
                        &sender,
                        &ctx,
                        WorkerEvent::Progress {
                            job_id,
                            percent: progress.0 + current.percent.min(100) * progress.1 / 100,
                            fps: current.fps,
                            speed: current.speed,
                            eta_seconds: current.eta,
                        },
                    );
                }
            }
        })
    });
    let error_reader = stderr.map(|stderr| {
        let warnings = warnings.clone();
        std::thread::spawn(move || {
            let mut diagnostic = Vec::new();
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                if line.to_ascii_lowercase().contains("warning")
                    && let Ok(mut summary) = warnings.lock()
                {
                    summary.record(&line);
                }
                diagnostic.push(line);
                if diagnostic.len() > 20 {
                    diagnostic.remove(0);
                }
            }
            diagnostic.join("\n")
        })
    });
    if let Some(reader) = output_reader {
        let _ = reader.join();
    }
    let diagnostic = error_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let mut child = control
        .active_child
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
        .ok_or_else(|| "Child process handle lost".to_owned())?;
    let status = child
        .wait()
        .map_err(|error| format!("Could not wait for {}: {error}", engine.label()))?;
    Ok((status, diagnostic, warnings))
}

fn mkvmerge_engine_eligible(
    input: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    selected_engine: RemuxEngine,
    mkvmerge_available: bool,
) -> bool {
    if !settings.is_remux_only()
        || settings.needs_ffmpeg_muxing()
        || settings.container != Container::Mkv
        || selected_engine != RemuxEngine::MkvToolNix
        || !mkvmerge_available
        || !is_matroska_input(input, metadata)
    {
        return false;
    }

    if !settings.subtitles.include {
        return true;
    }
    mkv::matching_tracks(&metadata.subtitle_tracks, &settings.subtitles.languages)
        .into_iter()
        .all(|track| {
            matches!(
                crate::compat::subtitle_plan(&track.codec_id, Container::Mkv),
                crate::compat::StreamPlan::Copy
            )
        })
}

fn is_matroska_input(input: &Path, metadata: &MediaMetadata) -> bool {
    input
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["mkv", "mk3d", "webm"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
        || metadata
            .container
            .as_deref()
            .is_some_and(|format| format.to_ascii_lowercase().contains("matroska"))
}

fn queue_ffmpeg_arguments(
    job: &JobSpec,
    output: &Path,
    spec: &QueueSpec,
    resources: &JobResources,
    edit_warnings: &mut Vec<String>,
) -> Result<(PathBuf, Vec<Vec<OsString>>, RemuxEngine), String> {
    let ffmpeg = spec.tools.ffmpeg.as_ref().ok_or_else(|| {
        if spec.remux_engine == RemuxEngine::MkvToolNix {
            "FFmpeg is unavailable and this job cannot use MKVToolNix".to_owned()
        } else {
            "FFmpeg is unavailable".to_owned()
        }
    })?;
    // FFmpeg may run with the temp directory as its working directory (text burn-in), so
    // relative imported-file paths are resolved here, against this process's directory.
    let settings = with_absolute_external_paths(&job.settings)?;
    if settings.is_remux_only()
        && !settings.track_edits.is_empty()
        && resources.text_burn_in_file.is_none()
    {
        let (arguments, warnings) =
            mkv::build_ffmpeg_edit_arguments(&job.input, output, &job.metadata, &settings)?;
        edit_warnings.extend(warnings);
        return Ok((ffmpeg.path.clone(), vec![arguments], RemuxEngine::Ffmpeg));
    }
    let absolute = resources.text_burn_in_file.is_some();
    let passes = build_ffmpeg_job_passes(
        &if absolute {
            absolute_path(&job.input)?
        } else {
            job.input.clone()
        },
        &if absolute {
            absolute_path(output)?
        } else {
            output.to_path_buf()
        },
        &job.metadata,
        &settings,
        &spec.encoders,
        resources,
    )?;
    Ok((ffmpeg.path.clone(), passes, RemuxEngine::Ffmpeg))
}

/// Copy of `settings` with every imported subtitle/audio file path made absolute.
fn with_absolute_external_paths(settings: &JobSettings) -> Result<JobSettings, String> {
    let mut settings = settings.clone();
    for track in settings
        .subtitles
        .external
        .iter_mut()
        .chain(settings.track_edits.external.iter_mut())
    {
        track.path = absolute_path(&track.path)?;
    }
    Ok(settings)
}

/// Keeps the useful end of a tool's stderr: drops the FFmpeg version/build banner and keeps the
/// last few lines, where the actual error is.
fn trim_diagnostic(text: &str) -> String {
    const KEEP: usize = 6;
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty() && !is_ffmpeg_banner_line(line))
        .collect();
    lines[lines.len().saturating_sub(KEEP)..].join("\n")
}

fn is_ffmpeg_banner_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("ffmpeg version")
        || trimmed.starts_with("built with")
        || trimmed.starts_with("configuration:")
        || (trimmed.starts_with("lib") && trimmed.contains(" / "))
}

/// Writes the renamed-chapter metadata file when the job overrides chapter names.
fn prepare_chapter_names(
    job: &JobSpec,
    spec: &QueueSpec,
) -> Result<Option<chapters::PreparedChapters>, String> {
    if job.settings.chapter_names.is_empty() || !job.settings.keep_chapters {
        return Ok(None);
    }
    let ffprobe = spec
        .tools
        .ffprobe
        .as_ref()
        .ok_or_else(|| "FFprobe is unavailable for chapter renaming".to_owned())?;
    chapters::prepare_chapter_file(&ffprobe.path, &job.input, &job.settings.chapter_names).map(Some)
}

/// A private temporary directory for two-pass statistics, removed when dropped.
struct PassLogs {
    directory: PathBuf,
}

impl PassLogs {
    fn create_if_needed(job: &JobSpec) -> Result<Option<Self>, String> {
        let two_pass = matches!(
            &job.settings.video,
            crate::transcode::VideoSettings::Encode(encode)
                if encode.passes >= 2
                    && matches!(encode.rate, crate::transcode::RateControl::Bitrate(_))
        );
        if !two_pass {
            return Ok(None);
        }
        let directory =
            std::env::temp_dir().join(format!("kestrel-pass-{}-{}", std::process::id(), job.id));
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("Could not create the pass log folder: {error}"))?;
        Ok(Some(Self { directory }))
    }

    fn prefix(&self) -> PathBuf {
        self.directory.join("pass")
    }
}

impl Drop for PassLogs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[derive(Default)]
struct QueueProgress {
    percent: u32,
    fps: Option<f64>,
    speed: Option<f64>,
    eta: Option<f64>,
}

impl QueueProgress {
    fn read(&mut self, line: &str, duration: Option<f64>) {
        if let Some(percent) = mkv::parse_progress_line(line) {
            self.percent = percent;
        } else if let Some(value) = line.strip_prefix("out_time_us=")
            && let (Ok(microseconds), Some(duration)) = (
                value.trim().parse::<u64>(),
                duration.filter(|duration| *duration > 0.0),
            )
        {
            let elapsed = microseconds as f64 / 1_000_000.0;
            self.percent = (elapsed / duration * 100.0).clamp(0.0, 100.0) as u32;
            self.eta = self
                .speed
                .filter(|speed| *speed > 0.0)
                .map(|speed| (duration - elapsed).max(0.0) / speed);
        } else if let Some(value) = line.strip_prefix("fps=") {
            self.fps = value.parse().ok();
        } else if let Some(value) = line.strip_prefix("speed=") {
            self.speed = value.trim_end_matches('x').parse().ok();
        }
    }
}

fn register_child(control: &WorkerControl, mut child: Child) -> Result<(), String> {
    let jobs = control.jobs.lock().ok();
    let mut guard = match control.active_child.lock() {
        Ok(guard) => guard,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "Could not register child process: process-control lock is poisoned".to_owned(),
            );
        }
    };
    if control.is_cancelled()
        || jobs
            .as_ref()
            .is_some_and(|jobs| jobs.active.is_some_and(|id| jobs.cancelled.contains(&id)))
    {
        let _ = child.kill();
    }
    *guard = Some(child);
    Ok(())
}

fn validate_output_paths(input: &Path, output: &Path) -> Result<(), String> {
    if mkv::paths_resolve_to_same_file(input, output)? {
        return Err("Output path is the same as the input".to_owned());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_file(
    index: usize,
    file: &FileInfo,
    output: &Path,
    spec: &BatchSpec,
    control: &WorkerControl,
    sender: &mpsc::Sender<WorkerEvent>,
    ctx: &EventNotifier,
) -> Result<ProcessOutcome, String> {
    if control.is_cancelled() {
        return Err("Stopped by user".to_owned());
    }
    if !spec.dry_run {
        validate_output_paths(&file.path, output)?;
    }
    let temporary_output = if spec.dry_run {
        None
    } else {
        Some(TemporaryOutput::create(output)?)
    };
    let engine_output = temporary_output
        .as_ref()
        .map(TemporaryOutput::path)
        .unwrap_or(output);
    let metadata = match &file.metadata {
        Some(metadata) => metadata.clone(),
        None => mkv::read_metadata(
            spec.tools.mkvmerge.as_ref().map(|tool| tool.path.as_path()),
            spec.tools.ffprobe.as_ref().map(|tool| tool.path.as_path()),
            &file.path,
        )?,
    };

    let (executable, arguments, engine) = match spec.engine {
        RemuxEngine::MkvToolNix => {
            let mkvmerge = spec
                .tools
                .mkvmerge
                .as_ref()
                .ok_or_else(|| "MKVToolNix is unavailable".to_owned())?;
            match mkv::build_arguments(
                &file.path,
                engine_output,
                &metadata,
                &spec.audio_languages,
                &spec.subtitle_languages,
                spec.dry_run,
                mkv::MkvRemuxSettings {
                    include_subtitles: true,
                    keep_chapters: true,
                    keep_metadata: true,
                    title: None,
                    track_edits: None,
                },
            ) {
                Ok(Some(arguments)) => (mkvmerge.path.clone(), arguments, RemuxEngine::MkvToolNix),
                Ok(None) => return Ok(ProcessOutcome::Success),
                Err(error) if error.contains("track IDs are unavailable") => {
                    let ffmpeg = spec.tools.ffmpeg.as_ref().ok_or_else(|| error.clone())?;
                    send(
                        sender,
                        ctx,
                        WorkerEvent::Log(format!(
                            "{}: using FFmpeg because MKVToolNix track IDs are unavailable",
                            file.name
                        )),
                    );
                    let Some(arguments) = mkv::build_ffmpeg_arguments(
                        &file.path,
                        engine_output,
                        &metadata,
                        &spec.audio_languages,
                        &spec.subtitle_languages,
                        spec.dry_run,
                    )?
                    else {
                        return Ok(ProcessOutcome::Success);
                    };
                    (ffmpeg.path.clone(), arguments, RemuxEngine::Ffmpeg)
                }
                Err(error) => return Err(error),
            }
        }
        RemuxEngine::Ffmpeg => {
            let ffmpeg = spec
                .tools
                .ffmpeg
                .as_ref()
                .ok_or_else(|| "FFmpeg is unavailable".to_owned())?;
            let Some(arguments) = mkv::build_ffmpeg_arguments(
                &file.path,
                engine_output,
                &metadata,
                &spec.audio_languages,
                &spec.subtitle_languages,
                spec.dry_run,
            )?
            else {
                return Ok(ProcessOutcome::Success);
            };
            (ffmpeg.path.clone(), arguments, RemuxEngine::Ffmpeg)
        }
    };
    if control.is_cancelled() {
        return Err("Stopped by user".to_owned());
    }

    let partial_output = temporary_output
        .as_ref()
        .map(TemporaryOutput::path)
        .ok_or_else(|| "Temporary output was not reserved".to_owned())?;
    let progress_mode = match engine {
        RemuxEngine::MkvToolNix => ProgressMode::MkvToolNix,
        RemuxEngine::Ffmpeg => ProgressMode::Ffmpeg(metadata.duration),
    };
    let mut child = tools::command(&executable)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start {}: {error}", engine.label()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    register_child(control, child)?;

    let warning_summary = Arc::new(Mutex::new(WarningSummary::default()));
    let stdout_reader = stdout.map(|stdout| {
        let sender = sender.clone();
        let ctx = ctx.clone();
        let warning_summary = warning_summary.clone();
        std::thread::spawn(move || {
            drain_progress(
                stdout,
                index,
                &sender,
                &ctx,
                false,
                progress_mode,
                warning_summary,
            )
        })
    });
    let stderr_reader = stderr.map(|stderr| {
        let sender = sender.clone();
        let ctx = ctx.clone();
        let warning_summary = warning_summary.clone();
        std::thread::spawn(move || {
            drain_progress(
                stderr,
                index,
                &sender,
                &ctx,
                true,
                progress_mode,
                warning_summary,
            )
        })
    });
    let _ = stdout_reader.and_then(|reader| reader.join().ok());
    let stderr_output = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let warnings = warning_summary
        .lock()
        .map(|summary| summary.clone())
        .unwrap_or_default();
    let Some(mut child) = control
        .active_child
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
    else {
        return Err(format!("{} process handle lost", engine.label()));
    };
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            return Err(format!("Could not wait for {}: {error}", engine.label()));
        }
    };

    if control.is_cancelled() {
        return Err("Stopped by user".to_owned());
    }
    let has_nonempty_output = std::fs::metadata(partial_output)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0);
    let exit_outcome = mkv::classify_remux_exit(
        engine == RemuxEngine::MkvToolNix,
        status.code(),
        has_nonempty_output,
    );
    if exit_outcome == mkv::RemuxExitOutcome::Failure {
        if !stderr_output.diagnostic_tail.trim().is_empty() {
            return Err(format!(
                "{} failed: {}",
                engine.label(),
                trim_diagnostic(&stderr_output.diagnostic_tail)
            ));
        }
        return Err(format!(
            "{} failed with exit code {:?}",
            engine.label(),
            status.code()
        ));
    }

    finalize_output(partial_output, output)?;

    match exit_outcome {
        mkv::RemuxExitOutcome::Success => Ok(ProcessOutcome::Success),
        mkv::RemuxExitOutcome::SuccessWithWarnings => Ok(ProcessOutcome::Warnings(warnings)),
        mkv::RemuxExitOutcome::Failure => {
            if !stderr_output.diagnostic_tail.trim().is_empty() {
                Err(format!(
                    "{} failed: {}",
                    engine.label(),
                    trim_diagnostic(&stderr_output.diagnostic_tail)
                ))
            } else {
                Err(format!(
                    "{} failed with exit code {:?}",
                    engine.label(),
                    status.code()
                ))
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ProgressMode {
    MkvToolNix,
    Ffmpeg(Option<f64>),
}

#[derive(Default)]
struct ChildOutput {
    diagnostic_tail: String,
}

#[derive(Clone, Default)]
struct WarningSummary {
    lines: Vec<String>,
    omitted: usize,
}

impl WarningSummary {
    fn record(&mut self, warning: &str) {
        if self.lines.len() < 50 {
            self.lines.push(warning.to_owned());
        } else {
            self.omitted += 1;
        }
    }
}

fn drain_progress<R: std::io::Read>(
    reader: R,
    index: usize,
    sender: &mpsc::Sender<WorkerEvent>,
    ctx: &EventNotifier,
    collect_errors: bool,
    progress_mode: ProgressMode,
    warnings: Arc<Mutex<WarningSummary>>,
) -> ChildOutput {
    let mut diagnostic_tail = Vec::new();
    for line in std::io::BufReader::new(reader)
        .lines()
        .map_while(Result::ok)
    {
        let progress = match progress_mode {
            ProgressMode::MkvToolNix => mkv::parse_progress_line(&line),
            ProgressMode::Ffmpeg(duration) => mkv::parse_ffmpeg_progress_line(&line, duration),
        };
        if let Some(percent) = progress {
            send(
                sender,
                ctx,
                WorkerEvent::FileUpdated {
                    index,
                    status: FileStatus::Processing,
                    percent,
                    detail: None,
                },
            );
        } else if !line.trim().is_empty() {
            if line.to_ascii_lowercase().contains("warning")
                && let Ok(mut summary) = warnings.lock()
            {
                summary.record(&line);
            }
            if collect_errors {
                diagnostic_tail.push(line);
                if diagnostic_tail.len() > 20 {
                    diagnostic_tail.remove(0);
                }
            }
        }
    }
    ChildOutput {
        diagnostic_tail: diagnostic_tail.join("\n"),
    }
}

struct TemporaryOutput {
    path: PathBuf,
}

impl TemporaryOutput {
    fn create(output: &Path) -> Result<Self, String> {
        for attempt in 0..1000 {
            let path = mkv::partial_output_path(output, std::process::id(), attempt);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    drop(file);
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!(
                        "Could not reserve temporary output {}: {error}",
                        path.display()
                    ));
                }
            }
        }
        Err(format!(
            "Could not reserve a unique temporary output for {} after 1000 attempts",
            output.display()
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn finalize_output(partial_output: &Path, output: &Path) -> Result<(), String> {
    #[cfg(windows)]
    if output.exists() {
        let backup = backup_output_path(output)?;
        std::fs::rename(output, &backup)
            .map_err(|error| format!("Could not preserve existing output file: {error}"))?;
        if let Err(error) = std::fs::rename(partial_output, output) {
            if let Err(restore_error) = std::fs::rename(&backup, output) {
                return Err(format!(
                    "Could not finalize output file: {error}; original output remains at {} ({restore_error})",
                    backup.display()
                ));
            }
            return Err(format!("Could not finalize output file: {error}"));
        }
        let _ = std::fs::remove_file(backup);
        return Ok(());
    }
    std::fs::rename(partial_output, output)
        .map_err(|error| format!("Could not finalize output file: {error}"))
}

#[cfg(windows)]
fn backup_output_path(output: &Path) -> Result<PathBuf, String> {
    let file_name = output
        .file_name()
        .ok_or_else(|| "Output path has no file name".to_owned())?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for index in 0..1000 {
        let mut candidate_name = file_name.to_os_string();
        candidate_name.push(format!(".kestrel-backup-{index}"));
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Could not create a temporary backup name for the output".to_owned())
}

fn should_skip_existing(skip_existing: bool, dry_run: bool, output_exists: bool) -> bool {
    !dry_run && skip_existing && output_exists
}

fn execution_spec(batch: &QueueSpec, job: &JobSpec) -> QueueSpec {
    QueueSpec {
        jobs: Vec::new(),
        tools: batch.tools.clone(),
        encoders: batch.encoders.clone(),
        remux_engine: job
            .execution
            .as_ref()
            .map_or(batch.remux_engine, |s| s.remux_engine),
        existing: job
            .execution
            .as_ref()
            .map_or(batch.existing, |s| s.existing),
        dry_run: job.execution.as_ref().map_or(batch.dry_run, |s| s.dry_run),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn external_paths_are_made_absolute_and_banner_is_trimmed_from_errors() {
        use super::*;
        use crate::external::ExternalTrack;
        let mut settings = JobSettings::default();
        let relative = ExternalTrack {
            path: "subs/en.srt".into(),
            ..ExternalTrack::default()
        };
        settings.subtitles.external = vec![relative.clone()];
        settings.track_edits.external = vec![relative];
        let absolute = with_absolute_external_paths(&settings).unwrap();
        assert!(absolute.subtitles.external[0].path.is_absolute());
        assert!(absolute.track_edits.external[0].path.is_absolute());
        assert!(absolute.subtitles.external[0].path.ends_with("subs/en.srt"));

        let stderr = "ffmpeg version 9.0.1 Copyright (c) 2000-2025
  built with gcc 14
  configuration: --enable-gpl
  libavutil      60.  8.100 / 60.  8.100
Input #0, matroska
subs/en.srt: No such file or directory
Error opening input files";
        let trimmed = trim_diagnostic(stderr);
        assert!(!trimmed.contains("ffmpeg version") && !trimmed.contains("libavutil"));
        assert!(!trimmed.contains("configuration"));
        assert!(trimmed.ends_with("Error opening input files"));
        assert!(trimmed.contains("No such file or directory"));
    }

    #[test]
    fn queue_start_logs_distinguish_remux_transcode_and_dry_run() {
        use super::*;
        let remux = JobSpec {
            id: 1,
            input: "input.mkv".into(),
            output: "output.mkv".into(),
            metadata: Default::default(),
            settings: Default::default(),
            execution: None,
        };
        let mut transcode = remux.clone();
        transcode.id = 2;
        transcode.settings.video = VideoSettings::Encode(Default::default());
        for (jobs, dry_run, expected) in [
            (vec![], false, "Remux batch started for 0 job(s)"),
            (
                vec![remux.clone()],
                false,
                "Remux batch started for 1 job(s)",
            ),
            (
                vec![transcode.clone()],
                false,
                "Transcode batch started for 1 job(s)",
            ),
            (
                vec![remux.clone(), transcode],
                false,
                "Transcode batch started for 2 job(s)",
            ),
            (vec![remux], true, "Dry-run batch started for 1 job(s)"),
        ] {
            let events = run_queue_blocking(QueueSpec {
                jobs,
                tools: Default::default(),
                encoders: Default::default(),
                remux_engine: RemuxEngine::Ffmpeg,
                existing: Default::default(),
                dry_run,
            });
            assert!(matches!(events.first(),Some(WorkerEvent::Log(text)) if text==expected));
        }
    }
    #[test]
    fn stop_requested_before_child_registration_kills_only_that_job() {
        use super::*;
        if std::env::var_os("KESTREL_WORKER_CONTROL_CHILD").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(60));
            return;
        }
        let control = WorkerControl::default();
        control.jobs.lock().unwrap().active = Some(42);
        control.stop_job(42);
        let child = crate::tools::command(&std::env::current_exe().unwrap())
            .args([
                "--exact",
                "worker::tests::stop_requested_before_child_registration_kills_only_that_job",
            ])
            .env("KESTREL_WORKER_CONTROL_CHILD", "1")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        register_child(&control, child).unwrap();
        let mut child = control.active_child.lock().unwrap().take().unwrap();
        assert!(!child.wait().unwrap().success());
        assert!(control.job_stopped(42));
        assert!(!control.is_cancelled());
        assert!(!control.job_stopped(43));
    }
    #[test]
    fn stopping_one_job_does_not_cancel_its_neighbors() {
        use super::*;
        let control = WorkerControl::default();
        control.stop_job(2);
        assert!(!control.is_cancelled());
        assert!(!control.job_stopped(1));
        assert!(control.job_stopped(2));
        let jobs = (1..=3)
            .map(|id| JobSpec {
                id,
                input: "input.mkv".into(),
                output: "output.mkv".into(),
                metadata: MediaMetadata {
                    video_codec: Some("h264".into()),
                    ..Default::default()
                },
                settings: Default::default(),
                execution: None,
            })
            .collect();
        let spec = QueueSpec {
            jobs,
            tools: Toolchain {
                ffmpeg: Some(crate::tools::Tool {
                    path: "unused tool".into(),
                    version: None,
                }),
                ..Default::default()
            },
            encoders: Default::default(),
            remux_engine: RemuxEngine::Ffmpeg,
            existing: ExistingPolicy::Overwrite,
            dry_run: true,
        };
        let (sender, receiver) = mpsc::channel();
        run_queue(spec, control.clone(), sender, Arc::new(|| {}));
        let events = receiver.into_iter().collect::<Vec<_>>();
        for (id, expected) in [
            (1, FileStatus::Success),
            (2, FileStatus::Cancelled),
            (3, FileStatus::Success),
        ] {
            assert!(events.iter().any(|event|matches!(event,WorkerEvent::JobUpdated{job_id,status,..} if *job_id==id&&*status==expected)));
        }
        control.reset();
        assert!(!control.job_stopped(2));
    }
    #[test]
    fn mixed_queue_honors_job_execution_snapshots() {
        use super::*;
        let directory = std::env::temp_dir().join(format!(
            "kestrel-snapshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let output = directory.join("movie.mkv");
        std::fs::write(&output, b"existing output").unwrap();
        let base = JobSpec {
            id: 1,
            input: "source with 'quotes'.mkv".into(),
            output: output.clone(),
            metadata: MediaMetadata {
                video_codec: Some("h264".into()),
                ..Default::default()
            },
            settings: JobSettings::default(),
            execution: Some(JobExecution {
                remux_engine: RemuxEngine::Ffmpeg,
                existing: ExistingPolicy::Overwrite,
                dry_run: true,
                output_target: crate::inputs::OutputTarget::File(output.clone()),
            }),
        };
        let mut skip = base.clone();
        skip.id = 2;
        let execution = skip.execution.as_mut().unwrap();
        execution.dry_run = false;
        execution.existing = ExistingPolicy::Skip;
        let mut rename = base.clone();
        rename.id = 3;
        rename.execution.as_mut().unwrap().existing = ExistingPolicy::Rename;
        let batch = QueueSpec {
            jobs: vec![base, skip, rename],
            tools: Toolchain {
                ffmpeg: Some(crate::tools::Tool {
                    path: "nonexistent ffmpeg".into(),
                    version: None,
                }),
                ..Default::default()
            },
            encoders: Default::default(),
            remux_engine: RemuxEngine::MkvToolNix,
            existing: ExistingPolicy::Overwrite,
            dry_run: false,
        };
        let events = run_queue_blocking(batch);
        for (id, expected) in [
            (1, FileStatus::Success),
            (2, FileStatus::Skipped),
            (3, FileStatus::Success),
        ] {
            assert!(events.iter().any(|event|matches!(event,WorkerEvent::JobUpdated {job_id,status,..} if *job_id==id && *status==expected)));
        }
        assert!(events.iter().any(|event|matches!(event,WorkerEvent::Command {job_id:3,arguments,display,..} if arguments.contains(&directory.join("movie (2).mkv").into_os_string()) && display.contains("source with"))));
        assert_eq!(std::fs::read(&output).unwrap(), b"existing output");
        assert!(!directory.join("movie (2).mkv").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_jobs_fall_back_to_batch_policies_and_missing_tools_fail() {
        use super::*;
        let job = JobSpec {
            id: 7,
            input: "input.mkv".into(),
            output: "output.mkv".into(),
            metadata: Default::default(),
            settings: Default::default(),
            execution: None,
        };
        let batch = QueueSpec {
            jobs: vec![job.clone()],
            tools: Default::default(),
            encoders: Default::default(),
            remux_engine: RemuxEngine::Ffmpeg,
            existing: ExistingPolicy::Rename,
            dry_run: true,
        };
        let effective = execution_spec(&batch, &job);
        assert_eq!(effective.remux_engine, batch.remux_engine);
        assert_eq!(effective.existing, batch.existing);
        assert!(effective.dry_run);
        let events = run_queue_blocking(batch);
        assert!(events.iter().any(|event|matches!(event,WorkerEvent::JobUpdated{job_id:7,status:FileStatus::Error,detail:Some(detail),..} if detail.contains("FFmpeg is unavailable"))));
    }
    use super::{
        RemuxEngine, TemporaryOutput, WarningSummary, mkvmerge_engine_eligible,
        should_skip_existing, validate_output_paths,
    };
    use crate::mkv::{self, MediaMetadata, TrackInfo};
    use crate::transcode::{JobSettings, SubtitleSettings, VideoEncode, VideoSettings};
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn poisoned_process_control_cleans_up_the_spawned_child() {
        let control = super::WorkerControl::default();
        let poisoned = control.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.active_child.lock().unwrap();
            panic!("poison fixture");
        })
        .join();
        let child = crate::tools::command(&std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let error = super::register_child(&control, child).unwrap_err();
        assert_eq!(
            error,
            "Could not register child process: process-control lock is poisoned"
        );
    }

    #[test]
    fn unavailable_encoder_fails_before_processing_or_starting_a_tool() {
        let job = super::JobSpec {
            execution: None,
            id: 42,
            input: "missing-source.mkv".into(),
            output: "missing-output.mkv".into(),
            metadata: MediaMetadata::default(),
            settings: JobSettings {
                video: VideoSettings::Encode(VideoEncode {
                    encoder: crate::transcode::VideoEncoder::SvtAv1,
                    ..Default::default()
                }),
                ..Default::default()
            },
        };
        let events = super::run_queue_blocking(super::QueueSpec {
            jobs: vec![job],
            tools: Default::default(),
            encoders: Default::default(),
            remux_engine: RemuxEngine::Ffmpeg,
            existing: Default::default(),
            dry_run: false,
        });
        assert!(
            events
                .iter()
                .any(|event| matches!(event, super::WorkerEvent::JobUpdated {
            job_id: 42, status: super::FileStatus::Error, detail: Some(message), ..
        } if message.contains("libsvtav1")))
        );
        assert!(!events.iter().any(|event| matches!(
            event,
            super::WorkerEvent::JobUpdated {
                status: super::FileStatus::Processing,
                ..
            }
        )));
    }

    #[test]
    fn dry_run_does_not_skip_existing_outputs() {
        assert!(!should_skip_existing(true, true, true));
        assert!(should_skip_existing(true, false, true));
        assert!(!should_skip_existing(false, false, true));
        assert!(!should_skip_existing(true, false, false));
    }

    #[test]
    fn selects_mkvmerge_only_for_eligible_matroska_remux_jobs() {
        let settings = JobSettings::default();
        let metadata = MediaMetadata {
            container: Some("mov,mp4,m4a,3gp,3g2,mj2".to_owned()),
            ..MediaMetadata::default()
        };
        assert!(!mkvmerge_engine_eligible(
            Path::new("film.mp4"),
            &metadata,
            &settings,
            RemuxEngine::MkvToolNix,
            true
        ));
        assert!(mkvmerge_engine_eligible(
            Path::new("film.MKV"),
            &metadata,
            &settings,
            RemuxEngine::MkvToolNix,
            true
        ));
        assert!(mkvmerge_engine_eligible(
            Path::new("film.mk3d"),
            &metadata,
            &settings,
            RemuxEngine::MkvToolNix,
            true
        ));
        assert!(mkvmerge_engine_eligible(
            Path::new("film.webm"),
            &metadata,
            &settings,
            RemuxEngine::MkvToolNix,
            true
        ));
        assert!(mkvmerge_engine_eligible(
            Path::new("film.bin"),
            &MediaMetadata {
                container: Some("matroska,webm".to_owned()),
                ..MediaMetadata::default()
            },
            &settings,
            RemuxEngine::MkvToolNix,
            true
        ));
        assert!(!mkvmerge_engine_eligible(
            Path::new("film.mkv"),
            &metadata,
            &settings,
            RemuxEngine::Ffmpeg,
            true
        ));
        assert!(!mkvmerge_engine_eligible(
            Path::new("film.mkv"),
            &metadata,
            &settings,
            RemuxEngine::MkvToolNix,
            false
        ));
        assert!(!mkvmerge_engine_eligible(
            Path::new("film.mkv"),
            &MediaMetadata {
                subtitle_tracks: vec![TrackInfo {
                    codec_id: "mov_text".to_owned(),
                    language: "eng".to_owned(),
                    ..TrackInfo::default()
                }],
                ..MediaMetadata::default()
            },
            &settings,
            RemuxEngine::MkvToolNix,
            true
        ));
        assert!(!mkvmerge_engine_eligible(
            Path::new("film.mkv"),
            &metadata,
            &JobSettings {
                video: VideoSettings::Encode(VideoEncode::default()),
                subtitles: SubtitleSettings::default(),
                ..JobSettings::default()
            },
            RemuxEngine::MkvToolNix,
            true
        ));
    }

    #[test]
    fn retains_only_the_first_fifty_warnings_and_counts_the_rest() {
        let mut summary = WarningSummary::default();
        for index in 0..52 {
            summary.record(&format!("warning {index}"));
        }

        assert_eq!(summary.lines.len(), 50);
        assert_eq!(summary.lines.first().map(String::as_str), Some("warning 0"));
        assert_eq!(summary.lines.last().map(String::as_str), Some("warning 49"));
        assert_eq!(summary.omitted, 2);
    }

    #[test]
    fn rejects_output_that_resolves_to_the_input() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after UNIX epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "kestrel-worker-path-test-{}-{timestamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("test directory should be created");
        let input = directory.join("film.mkv");
        std::fs::write(&input, b"input").expect("input fixture should be written");

        assert_eq!(
            validate_output_paths(&input, &input).expect_err("same-path output should be rejected"),
            "Output path is the same as the input"
        );

        std::fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn reserves_a_unique_temp_output_without_overwriting_existing_files() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after UNIX epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "kestrel-worker-temp-output-test-{}-{timestamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("test directory should be created");
        let output = directory.join("film.mkv");
        let legacy_partial = directory.join("film.langmux-partial.mkv");
        let occupied_candidate = mkv::partial_output_path(&output, std::process::id(), 0);
        std::fs::write(&legacy_partial, b"legacy partial")
            .expect("legacy partial fixture should be written");
        std::fs::write(&occupied_candidate, b"occupied candidate")
            .expect("occupied candidate fixture should be written");

        let temporary_output =
            TemporaryOutput::create(&output).expect("next free output should be reserved");
        let expected = mkv::partial_output_path(&output, std::process::id(), 1);
        assert_eq!(temporary_output.path(), expected);
        assert_eq!(
            std::fs::read(&legacy_partial).expect("legacy partial should survive"),
            b"legacy partial"
        );
        assert_eq!(
            std::fs::read(&occupied_candidate).expect("occupied candidate should survive"),
            b"occupied candidate"
        );
        assert_eq!(
            std::fs::metadata(temporary_output.path())
                .expect("reserved path should exist")
                .len(),
            0
        );

        drop(temporary_output);
        assert!(!expected.exists());
        std::fs::remove_dir_all(directory).expect("test directory should be removed");
    }
}
