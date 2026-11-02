use crate::encoders::EncoderSet;
use crate::formats::Container;
use crate::inputs::ExistingPolicy;
use crate::mkv::{self, AvailableLanguages, BatchStats, FileInfo, MediaMetadata};
use crate::tools::{self, Toolchain};
use crate::transcode::{JobSettings, build_ffmpeg_job_arguments};
use eframe::egui;
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
    Log(String),
    BatchFinished {
        cancelled: bool,
        elapsed_seconds: f64,
    },
}

#[derive(Clone)]
pub struct WorkerControl {
    paused: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    active_child: Arc<Mutex<Option<Child>>>,
}

impl Default for WorkerControl {
    fn default() -> Self {
        Self {
            paused: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            active_child: Arc::new(Mutex::new(None)),
        }
    }
}

impl WorkerControl {
    pub fn reset(&self) {
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
pub struct JobSpec {
    pub id: u64,
    pub input: PathBuf,
    pub output: PathBuf,
    pub metadata: MediaMetadata,
    pub settings: JobSettings,
}

#[derive(Clone)]
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

fn send(sender: &mpsc::Sender<WorkerEvent>, ctx: &egui::Context, event: WorkerEvent) {
    if sender.send(event).is_ok() {
        ctx.request_repaint();
    }
}

pub fn scan_directory(
    directory: PathBuf,
    tools: Toolchain,
    sender: mpsc::Sender<WorkerEvent>,
    ctx: egui::Context,
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
    ctx: egui::Context,
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
    ctx: &egui::Context,
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

pub fn start_queue(
    spec: QueueSpec,
    control: WorkerControl,
    sender: mpsc::Sender<WorkerEvent>,
    ctx: egui::Context,
) {
    control.reset();
    std::thread::spawn(move || run_queue(spec, control, sender, ctx));
}

pub fn run_queue_blocking(spec: QueueSpec) -> Vec<WorkerEvent> {
    let (sender, receiver) = mpsc::channel();
    let context = egui::Context::default();
    run_queue(spec, WorkerControl::default(), sender, context);
    receiver.into_iter().collect()
}

fn run_queue(
    spec: QueueSpec,
    control: WorkerControl,
    sender: mpsc::Sender<WorkerEvent>,
    ctx: egui::Context,
) {
    let started = Instant::now();
    send(
        &sender,
        &ctx,
        WorkerEvent::Log(format!(
            "{} batch started for {} job(s)",
            if spec.dry_run { "Dry-run" } else { "Transcode" },
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
        if !spec.dry_run
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
        let output = match queue_output(&job.output, spec.existing, spec.dry_run) {
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
        if !spec.dry_run
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
        let result = process_queue_job(job, &output, &spec, &control, &sender, &ctx);
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
    ctx: &egui::Context,
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
    ctx: &egui::Context,
) -> Result<ProcessOutcome, String> {
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

    let (executable, arguments, engine) = if uses_mkvmerge {
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
        ) {
            Ok(Some(arguments)) => (mkvmerge.path.clone(), arguments, RemuxEngine::MkvToolNix),
            Ok(None) => queue_ffmpeg_arguments(job, engine_output, spec)?,
            Err(error) if error.contains("track IDs are unavailable") => {
                queue_ffmpeg_arguments(job, engine_output, spec)?
            }
            Err(error) => return Err(error),
        }
    } else {
        queue_ffmpeg_arguments(job, engine_output, spec)?
    };
    if control.is_cancelled() {
        return Err("Stopped by user".to_owned());
    }
    let command_text = std::iter::once(executable.to_string_lossy().into_owned())
        .chain(
            arguments
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join(" ");
    send(
        sender,
        ctx,
        WorkerEvent::Log(format!("{}: {command_text}", job.input.display())),
    );
    if spec.dry_run {
        return Ok(ProcessOutcome::Success);
    }
    let partial = temporary.as_ref().unwrap().path();
    let mut child = tools::command(&executable)
        .args(&arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start {}: {error}", engine.label()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let mut guard = control
        .active_child
        .lock()
        .map_err(|_| "Could not register child process".to_owned())?;
    *guard = Some(child);
    if control.is_cancelled()
        && let Some(child) = guard.as_mut()
    {
        let _ = child.kill();
    }
    drop(guard);

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
                            percent: current.percent,
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
    if control.is_cancelled() {
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
        return Err(if diagnostic.trim().is_empty() {
            format!(
                "{} failed with exit code {:?}",
                engine.label(),
                status.code()
            )
        } else {
            format!("{} failed: {}", engine.label(), diagnostic.trim())
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

fn mkvmerge_engine_eligible(
    input: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    selected_engine: RemuxEngine,
    mkvmerge_available: bool,
) -> bool {
    if !settings.is_remux_only()
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
    let languages = mkv::normalize_languages(&settings.subtitles.languages);
    metadata
        .subtitle_tracks
        .iter()
        .filter(|track| {
            languages.is_empty() || languages.contains(&track.language.to_ascii_lowercase())
        })
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
) -> Result<(PathBuf, Vec<OsString>, RemuxEngine), String> {
    let ffmpeg = spec.tools.ffmpeg.as_ref().ok_or_else(|| {
        if spec.remux_engine == RemuxEngine::MkvToolNix {
            "FFmpeg is unavailable and this job cannot use MKVToolNix".to_owned()
        } else {
            "FFmpeg is unavailable".to_owned()
        }
    })?;
    let arguments = build_ffmpeg_job_arguments(
        &job.input,
        output,
        &job.metadata,
        &job.settings,
        &spec.encoders,
    )?;
    Ok((ffmpeg.path.clone(), arguments, RemuxEngine::Ffmpeg))
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
    ctx: &egui::Context,
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
    if let Ok(mut guard) = control.active_child.lock() {
        *guard = Some(child);
        if control.is_cancelled()
            && let Some(child) = guard.as_mut()
        {
            let _ = child.kill();
        }
    } else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Could not register mkvmerge process".to_owned());
    }

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
                stderr_output.diagnostic_tail.trim()
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
                    stderr_output.diagnostic_tail.trim()
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
    ctx: &egui::Context,
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
        candidate_name.push(format!(".langmux-backup-{index}"));
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

#[cfg(test)]
mod tests {
    use super::{
        RemuxEngine, TemporaryOutput, WarningSummary, mkvmerge_engine_eligible,
        should_skip_existing, validate_output_paths,
    };
    use crate::mkv::{self, MediaMetadata, TrackInfo};
    use crate::transcode::{JobSettings, SubtitleSettings, VideoEncode, VideoSettings};
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

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
            "langmux-worker-path-test-{}-{timestamp}",
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
            "langmux-worker-temp-output-test-{}-{timestamp}",
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
