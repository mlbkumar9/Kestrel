//! Transcode preview: encodes a short segment of the source with the job's exact settings into
//! a temporary file, measures it, and cuts a still from the source and from the result.

use crate::analysis;
use crate::encoders::EncoderSet;
use crate::mkv::MediaMetadata;
use crate::subtitles::{absolute_path, prepare_text_subtitle};
use crate::tools;
use crate::transcode::{
    JobSettings, Segment, build_ffmpeg_segment_arguments, is_text_subtitle,
    selected_burn_in_subtitle,
};
use std::ffi::OsString;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Segment lengths offered by the page, in seconds.
pub const LENGTHS_SECONDS: [f64; 3] = [15.0, 30.0, 60.0];

/// Width of the comparison stills.
const STILL_WIDTH: u32 = 640;

static NEXT_PREVIEW: AtomicUsize = AtomicUsize::new(0);

/// Where the segment lies: starts at `position` (0..=1) of the source, moved earlier when the
/// segment would run past the end, and shortened when the source is shorter than the segment.
pub fn plan_segment(duration: f64, length_seconds: f64, position: f64) -> Option<Segment> {
    if !duration.is_finite() || duration <= 0.0 || !length_seconds.is_finite() {
        return None;
    }
    let length = length_seconds.clamp(0.5, duration);
    let start = (duration * position.clamp(0.0, 1.0)).min(duration - length);
    Some(Segment {
        start_seconds: start.max(0.0),
        length_seconds: length,
    })
}

/// Directory that holds every preview file of this process.
pub fn preview_directory() -> PathBuf {
    std::env::temp_dir().join(format!("kestrel-preview-{}", std::process::id()))
}

/// Removes every preview file of this process (used when the app exits).
pub fn remove_preview_directory() {
    let _ = std::fs::remove_dir_all(preview_directory());
}

/// Lets the UI cancel a running preview from another thread.
#[derive(Debug, Default)]
pub struct PreviewControl {
    cancelled: AtomicBool,
    child: Mutex<Option<Child>>,
}

impl PreviewControl {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.kill();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PreviewError {
    Cancelled,
    Failed(String),
}

impl From<String> for PreviewError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// A finished preview. The files live in [`preview_directory`] until [`discard`](Self::discard).
#[derive(Clone, Debug)]
pub struct PreviewOutcome {
    pub file: PathBuf,
    pub segment: Segment,
    pub bytes: u64,
    /// Average bitrate of the segment, audio included.
    pub bitrate_kbps: f64,
    /// The segment's size scaled to the whole source.
    pub estimated_full_bytes: u64,
    /// PNG of the source at the segment midpoint, and of the encoded file at its midpoint.
    pub source_still: Option<PathBuf>,
    pub encoded_still: Option<PathBuf>,
    /// Why a still is missing; the preview itself is still usable.
    pub still_error: Option<String>,
}

impl PreviewOutcome {
    /// Deletes the preview file and its stills.
    pub fn discard(&self) {
        for path in [
            Some(&self.file),
            self.source_still.as_ref(),
            self.encoded_still.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Everything a preview run needs; owned so it can move to a worker thread.
#[derive(Clone, Debug)]
pub struct PreviewRequest {
    pub ffmpeg: PathBuf,
    pub input: PathBuf,
    pub metadata: MediaMetadata,
    pub settings: JobSettings,
    pub encoders: EncoderSet,
    pub segment: Segment,
}

/// Encodes the segment, then cuts the stills. `progress` receives 0..=100 while FFmpeg runs.
/// Blocking: call from a worker thread.
pub fn run_preview(
    request: &PreviewRequest,
    control: &PreviewControl,
    progress: &mut dyn FnMut(u32),
) -> Result<PreviewOutcome, PreviewError> {
    let duration = request
        .metadata
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or_else(|| "The source duration is unavailable".to_owned())?;
    let segment = request.segment;

    // The same resolution the worker applies before it builds a job's arguments.
    let (metadata, settings) = analysis::resolve_automatic_settings(
        &request.ffmpeg,
        &request.input,
        &request.metadata,
        &request.settings,
    )?;
    if control.is_cancelled() {
        return Err(PreviewError::Cancelled);
    }
    let mut settings = settings;
    for track in settings
        .subtitles
        .external
        .iter_mut()
        .chain(settings.track_edits.external.iter_mut())
    {
        track.path = absolute_path(&track.path)?;
    }

    let prepared = selected_burn_in_subtitle(&metadata, &settings)
        .filter(|(_, track)| is_text_subtitle(&track.codec_id))
        .map(|(_, track)| {
            let index = track
                .ffprobe_index
                .ok_or_else(|| "Subtitle stream index is unavailable".to_owned())?;
            prepare_text_subtitle(&request.ffmpeg, &request.input, index)
        })
        .transpose()?;

    let directory = preview_directory();
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create the preview folder: {error}"))?;
    let id = NEXT_PREVIEW.fetch_add(1, Ordering::Relaxed);
    let file = directory.join(format!("preview-{id}.{}", settings.container.extension()));
    let arguments = build_ffmpeg_segment_arguments(
        &absolute_path(&request.input)?,
        &file,
        &metadata,
        &settings,
        &request.encoders,
        prepared.as_ref().map(|subtitle| subtitle.filename()),
        segment,
    )?;
    let result = encode(
        &request.ffmpeg,
        &arguments,
        segment.length_seconds,
        control,
        progress,
    );
    drop(prepared);
    if let Err(error) = result {
        let _ = std::fs::remove_file(&file);
        return Err(error);
    }
    let bytes = std::fs::metadata(&file)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("The preview file is missing: {error}"))?;
    if bytes == 0 {
        let _ = std::fs::remove_file(&file);
        return Err("The preview file is empty".to_owned().into());
    }

    let midpoint = segment.length_seconds / 2.0;
    let source_path = directory.join(format!("source-{id}.png"));
    let encoded_path = directory.join(format!("encoded-{id}.png"));
    let mut still_errors = Vec::new();
    let source_still = frame_png(
        &request.ffmpeg,
        &request.input,
        segment.start_seconds + midpoint,
        &source_path,
    )
    .map(|()| source_path)
    .map_err(|error| still_errors.push(format!("source: {error}")))
    .ok();
    let encoded_still = frame_png(&request.ffmpeg, &file, midpoint, &encoded_path)
        .map(|()| encoded_path)
        .map_err(|error| still_errors.push(format!("encoded: {error}")))
        .ok();

    Ok(PreviewOutcome {
        file,
        segment,
        bytes,
        bitrate_kbps: bytes as f64 * 8.0 / segment.length_seconds / 1000.0,
        estimated_full_bytes: (bytes as f64 * duration / segment.length_seconds).round() as u64,
        source_still,
        encoded_still,
        still_error: (!still_errors.is_empty()).then(|| still_errors.join("; ")),
    })
}

fn encode(
    ffmpeg: &Path,
    arguments: &[OsString],
    length_seconds: f64,
    control: &PreviewControl,
    progress: &mut dyn FnMut(u32),
) -> Result<(), PreviewError> {
    if control.is_cancelled() {
        return Err(PreviewError::Cancelled);
    }
    let mut child = tools::command(ffmpeg)
        .args(arguments)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start FFmpeg: {error}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    {
        let mut slot = control
            .child
            .lock()
            .map_err(|_| "The preview process lock is poisoned".to_owned())?;
        if control.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PreviewError::Cancelled);
        }
        *slot = Some(child);
    }
    let diagnostic = std::thread::spawn(move || {
        let mut lines = Vec::new();
        if let Some(stderr) = stderr {
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                lines.push(line);
                if lines.len() > 12 {
                    lines.remove(0);
                }
            }
        }
        lines.join("\n")
    });
    if let Some(stdout) = stdout {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if let Some(seconds) = progress_seconds(&line) {
                progress(((seconds / length_seconds) * 100.0).clamp(0.0, 99.0) as u32);
            }
        }
    }
    let diagnostic = diagnostic.join().unwrap_or_default();
    let mut child = control
        .child
        .lock()
        .map_err(|_| "The preview process lock is poisoned".to_owned())?
        .take()
        .ok_or_else(|| "The preview process handle was lost".to_owned())?;
    let status = child
        .wait()
        .map_err(|error| format!("Could not wait for FFmpeg: {error}"))?;
    if control.is_cancelled() {
        return Err(PreviewError::Cancelled);
    }
    if !status.success() {
        let tail = diagnostic
            .lines()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(PreviewError::Failed(format!("FFmpeg failed: {tail}")));
    }
    progress(100);
    Ok(())
}

/// Seconds encoded so far from an FFmpeg `-progress` line (`out_time_us`/`out_time_ms` are both
/// microseconds).
pub(crate) fn progress_seconds(line: &str) -> Option<f64> {
    let value = line
        .strip_prefix("out_time_us=")
        .or_else(|| line.strip_prefix("out_time_ms="))?;
    let micros: i64 = value.trim().parse().ok()?;
    Some(micros.max(0) as f64 / 1_000_000.0)
}

/// Writes one frame of `input` at `at_seconds` as a PNG at most [`STILL_WIDTH`] wide.
fn frame_png(ffmpeg: &Path, input: &Path, at_seconds: f64, output: &Path) -> Result<(), String> {
    let result = tools::command(ffmpeg)
        .args([
            "-nostdin",
            "-y",
            "-ss",
            &format!("{:.3}", at_seconds.max(0.0)),
            "-i",
        ])
        .arg(absolute_path(input)?)
        .args([
            "-frames:v",
            "1",
            "-vf",
            &format!("scale='min(iw,{STILL_WIDTH})':-2"),
        ])
        .arg(output)
        .stdin(Stdio::null())
        .current_dir(std::env::temp_dir())
        .output()
        .map_err(|error| format!("could not start FFmpeg: {error}"))?;
    if !result.status.success() || !output.is_file() {
        return Err(format!(
            "FFmpeg could not cut a frame: {}",
            String::from_utf8_lossy(&result.stderr)
                .lines()
                .last()
                .unwrap_or("")
                .trim()
        ));
    }
    Ok(())
}

/// Opens a file with the system's default player or viewer.
pub fn open_with_system(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = tools::command(Path::new("explorer"));
    #[cfg(target_os = "macos")]
    let mut command = tools::command(Path::new("open"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = tools::command(Path::new("xdg-open"));
    command
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_start_at_the_position_and_stay_inside_the_source() {
        let at = |duration, length, position| plan_segment(duration, length, position).unwrap();
        assert_eq!(at(600.0, 30.0, 0.25).start_seconds, 150.0);
        assert_eq!(at(600.0, 30.0, 0.75).start_seconds, 450.0);
        // Near the end the segment moves earlier so it keeps its length.
        let late = at(600.0, 60.0, 1.0);
        assert_eq!(late.start_seconds, 540.0);
        assert_eq!(late.length_seconds, 60.0);
        // A source shorter than the segment is encoded whole.
        let short = at(10.0, 30.0, 0.5);
        assert_eq!((short.start_seconds, short.length_seconds), (0.0, 10.0));
        assert_eq!(at(600.0, 15.0, -3.0).start_seconds, 0.0);
        assert!(plan_segment(0.0, 15.0, 0.5).is_none());
        assert!(plan_segment(f64::NAN, 15.0, 0.5).is_none());
    }

    #[test]
    fn progress_lines_give_encoded_seconds() {
        assert_eq!(progress_seconds("out_time_us=2500000"), Some(2.5));
        assert_eq!(progress_seconds("out_time_ms=1000000"), Some(1.0));
        assert_eq!(progress_seconds("out_time_us=-9"), Some(0.0));
        assert_eq!(progress_seconds("out_time_us=N/A"), None);
        assert_eq!(progress_seconds("speed=2x"), None);
    }

    #[test]
    fn cancel_before_the_run_is_remembered() {
        let control = PreviewControl::default();
        assert!(!control.is_cancelled());
        control.cancel();
        assert!(control.is_cancelled());
    }
}
