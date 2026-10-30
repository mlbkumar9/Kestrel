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
            if spec.skip_existing
                && std::fs::metadata(&output)
                    .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
            {
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
                Ok(()) => {
                    send(
                        &sender,
                        &ctx,
                        WorkerEvent::FileUpdated {
                            index,
                            status: FileStatus::Success,
                            percent: 100,
                            detail: None,
                        },
                    );
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

#[allow(clippy::too_many_arguments)]
fn process_file(
    index: usize,
    file: &FileInfo,
    output: &Path,
    spec: &BatchSpec,
    control: &WorkerControl,
    sender: &mpsc::Sender<WorkerEvent>,
    ctx: &egui::Context,
) -> Result<(), String> {
    if control.is_cancelled() {
        return Err("Stopped by user".to_owned());
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
                output,
                &metadata,
                &spec.audio_languages,
                &spec.subtitle_languages,
                spec.dry_run,
            ) {
                Ok(Some(arguments)) => (mkvmerge.path.clone(), arguments, RemuxEngine::MkvToolNix),
                Ok(None) => return Ok(()),
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
                        output,
                        &metadata,
                        &spec.audio_languages,
                        &spec.subtitle_languages,
                        spec.dry_run,
                    )?
                    else {
                        return Ok(());
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
                output,
                &metadata,
                &spec.audio_languages,
                &spec.subtitle_languages,
                spec.dry_run,
            )?
            else {
                return Ok(());
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
    if let Some(reader) = stdout_reader {
        let _ = reader.join();
    }
    let stderr_output = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let status = control
        .active_child
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
        .ok_or_else(|| format!("{} process handle lost", engine.label()))?
        .wait()
        .map_err(|error| format!("Could not wait for {}: {error}", engine.label()))?;

    if control.is_cancelled() {
        remove_partial_output(output);
        return Err("Stopped by user".to_owned());
    }
    if status.success()
        && std::fs::metadata(output).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    {
        return Ok(());
    }

    remove_partial_output(output);
    if !stderr_output.trim().is_empty() {
        Err(format!(
            "{} failed: {}",
            engine.label(),
            stderr_output.trim()
        ))
    } else {
        Err(format!(
            "{} failed with exit code {:?}",
            engine.label(),
            status.code()
        ))
    }
}

#[derive(Clone, Copy)]
enum ProgressMode {
    MkvToolNix,
    Ffmpeg(Option<f64>),
}

fn drain_progress<R: std::io::Read>(
    reader: R,
    index: usize,
    sender: &mpsc::Sender<WorkerEvent>,
    ctx: &egui::Context,
    collect_errors: bool,
    progress_mode: ProgressMode,
) -> String {
    let mut tail = Vec::new();
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
        } else if collect_errors && !line.trim().is_empty() {
            tail.push(line);
            if tail.len() > 20 {
                tail.remove(0);
            }
        }
    }
    tail.join("\n")
}

fn remove_partial_output(output: &Path) {
    if output.is_file() {
        let _ = std::fs::remove_file(output);
    }
}
