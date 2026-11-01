use crate::mkv::{self, AvailableLanguages, BatchStats, FileInfo, MediaMetadata};
use crate::tools::{self, Toolchain};
use eframe::egui;
use serde::{Deserialize, Serialize};
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

enum ProcessOutcome {
    Success,
    Warnings(Vec<String>),
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
            let partial_output = mkv::partial_output_path(&output);
            if !spec.dry_run
                && let Err(error) = validate_output_paths(&file.path, &output, &partial_output)
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
                        ProcessOutcome::Warnings(lines) => {
                            (Some("Warnings".to_owned()), Some(lines))
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
                        for warning in warnings {
                            send(
                                &sender,
                                &ctx,
                                WorkerEvent::Log(format!("{} — {warning}", file.name)),
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

fn validate_output_paths(input: &Path, output: &Path, partial_output: &Path) -> Result<(), String> {
    if mkv::paths_resolve_to_same_file(input, output)? {
        return Err("Output path is the same as the input".to_owned());
    }
    if mkv::paths_resolve_to_same_file(input, partial_output)? {
        return Err("Temporary output path is the same as the input".to_owned());
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
    let partial_output = mkv::partial_output_path(output);
    if !spec.dry_run {
        validate_output_paths(&file.path, output, &partial_output)?;
    }
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
                &partial_output,
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
                        &partial_output,
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
                &partial_output,
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

    let progress_mode = match engine {
        RemuxEngine::MkvToolNix => ProgressMode::MkvToolNix,
        RemuxEngine::Ffmpeg => ProgressMode::Ffmpeg(metadata.duration),
    };
    remove_partial_output(&partial_output);
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
        remove_partial_output(&partial_output);
        return Err("Could not register mkvmerge process".to_owned());
    }

    let stdout_reader = stdout.map(|stdout| {
        let sender = sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            drain_progress(stdout, index, &sender, &ctx, false, progress_mode)
        })
    });
    let stderr_reader = stderr.map(|stderr| {
        let sender = sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            drain_progress(stderr, index, &sender, &ctx, true, progress_mode)
        })
    });
    let stdout_output = stdout_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let stderr_output = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let Some(mut child) = control
        .active_child
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
    else {
        remove_partial_output(&partial_output);
        return Err(format!("{} process handle lost", engine.label()));
    };
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            remove_partial_output(&partial_output);
            return Err(format!("Could not wait for {}: {error}", engine.label()));
        }
    };

    if control.is_cancelled() {
        remove_partial_output(&partial_output);
        return Err("Stopped by user".to_owned());
    }
    let has_nonempty_output = std::fs::metadata(&partial_output)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0);
    let exit_outcome = mkv::classify_remux_exit(
        engine == RemuxEngine::MkvToolNix,
        status.code(),
        has_nonempty_output,
    );
    if exit_outcome == mkv::RemuxExitOutcome::Failure {
        remove_partial_output(&partial_output);
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

    if let Err(error) = finalize_output(&partial_output, output) {
        remove_partial_output(&partial_output);
        return Err(error);
    }

    match exit_outcome {
        mkv::RemuxExitOutcome::Success => Ok(ProcessOutcome::Success),
        mkv::RemuxExitOutcome::SuccessWithWarnings => {
            let warnings = stderr_output
                .warnings
                .into_iter()
                .chain(stdout_output.warnings)
                .collect();
            Ok(ProcessOutcome::Warnings(warnings))
        }
        mkv::RemuxExitOutcome::Failure => {
            remove_partial_output(&partial_output);
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
    warnings: Vec<String>,
}

fn drain_progress<R: std::io::Read>(
    reader: R,
    index: usize,
    sender: &mpsc::Sender<WorkerEvent>,
    ctx: &egui::Context,
    collect_errors: bool,
    progress_mode: ProgressMode,
) -> ChildOutput {
    let mut diagnostic_tail = Vec::new();
    let mut warnings = Vec::new();
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
            if line.to_ascii_lowercase().contains("warning") {
                warnings.push(line.clone());
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
        warnings,
    }
}

fn remove_partial_output(output: &Path) {
    let _ = std::fs::remove_file(output);
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
    use super::{should_skip_existing, validate_output_paths};
    use crate::mkv;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dry_run_does_not_skip_existing_outputs() {
        assert!(!should_skip_existing(true, true, true));
        assert!(should_skip_existing(true, false, true));
        assert!(!should_skip_existing(false, false, true));
        assert!(!should_skip_existing(true, false, false));
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
            validate_output_paths(&input, &input, &mkv::partial_output_path(&input))
                .expect_err("same-path output should be rejected"),
            "Output path is the same as the input"
        );

        std::fs::remove_dir_all(directory).expect("test directory should be removed");
    }
}
