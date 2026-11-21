//! Quality metrics of one or more encodes against a reference: VMAF, PSNR, SSIM and XPSNR from
//! FFmpeg's `libvmaf`, `psnr`, `ssim` and `xpsnr` filters.
//!
//! [`build_arguments`] writes one FFmpeg command per distorted file; [`run_metrics`] runs them
//! one after another and parses the logs they leave behind into per-frame [`FrameRow`] values and
//! a [`FileSummary`]. Frame numbers are 0-based and count from the start of the selected range.
//! Everything FFmpeg writes goes to plain relative file names in a per-run scratch directory that
//! is also the process's working directory, so no Windows path ever enters a filtergraph.

use crate::mkv;
use crate::preview::progress_seconds;
use crate::subtitles::absolute_path;
use crate::tools::{self, Toolchain};
use regex::Regex;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

/// VMAF model used for sources up to 1080p.
pub const MODEL_HD: &str = "vmaf_v0.6.1";
/// VMAF model used for sources of 2160p and more.
pub const MODEL_4K: &str = "vmaf_4k_v0.6.1";
/// PSNR and XPSNR of identical frames are infinite; they are held at this value so means stay
/// finite.
pub const PSNR_CAP_DB: f64 = 100.0;

const VMAF_LOG: &str = "vmaf.json";
const PSNR_LOG: &str = "psnr.log";
const SSIM_LOG: &str = "ssim.log";
const XPSNR_LOG: &str = "xpsnr.log";
const SHOT_PATTERN: &str = "shot%03d.png";

static NEXT_RUN: AtomicUsize = AtomicUsize::new(0);

/// Which VMAF model scores the run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum VmafModel {
    /// The 4K model for a reference of 2160p or more, the standard one below that.
    #[default]
    Auto,
    Named(String),
}

impl VmafModel {
    /// The model name FFmpeg is given for a reference of `width` x `height`.
    pub fn resolve(&self, width: u32, height: u32) -> String {
        match self {
            Self::Auto if height >= 2160 || width >= 3840 => MODEL_4K.to_owned(),
            Self::Auto => MODEL_HD.to_owned(),
            Self::Named(name) => name.clone(),
        }
    }
}

/// Part of the clips to measure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetricRange {
    pub start_seconds: f64,
    /// `None` measures to the end of the shorter clip.
    pub duration_seconds: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct MetricsSettings {
    pub reference: PathBuf,
    pub distorted: Vec<PathBuf>,
    pub vmaf: bool,
    pub psnr: bool,
    pub ssim: bool,
    pub xpsnr: bool,
    pub range: Option<MetricRange>,
    pub model: VmafModel,
    /// Threads libvmaf may use.
    pub threads: usize,
}

impl MetricsSettings {
    /// Every metric on, the whole clips, the automatic model and one thread per core.
    pub fn new(reference: PathBuf, distorted: Vec<PathBuf>) -> Self {
        Self {
            reference,
            distorted,
            vmaf: true,
            psnr: true,
            ssim: true,
            xpsnr: true,
            range: None,
            model: VmafModel::Auto,
            threads: default_threads(),
        }
    }

    fn enabled_filters(&self) -> Vec<&'static str> {
        [
            (self.vmaf, "libvmaf"),
            (self.psnr, "psnr"),
            (self.ssim, "ssim"),
            (self.xpsnr, "xpsnr"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }

    /// Settings that cannot produce a run, in words.
    pub fn validate(&self) -> Result<(), String> {
        if self.distorted.is_empty() {
            return Err("Add at least one file to measure".to_owned());
        }
        if self.enabled_filters().is_empty() {
            return Err("Turn on at least one metric".to_owned());
        }
        if let Some(range) = self.range {
            if !range.start_seconds.is_finite() || range.start_seconds < 0.0 {
                return Err("The range start must be 0 or more seconds".to_owned());
            }
            if range
                .duration_seconds
                .is_some_and(|duration| !duration.is_finite() || duration <= 0.0)
            {
                return Err("The range duration must be more than 0 seconds".to_owned());
            }
        }
        if let VmafModel::Named(name) = &self.model
            && (name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)))
        {
            return Err(format!("\"{name}\" is not a valid VMAF model name"));
        }
        Ok(())
    }
}

/// Threads available to this process, at least one.
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, usize::from)
}

/// Size and pixel format both clips are brought to before measuring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry {
    pub width: u32,
    pub height: u32,
    pub pixel_format: &'static str,
}

impl Geometry {
    /// Reference size, and a 10-bit format when the reference is deeper than 8 bits.
    pub fn of_reference(width: u32, height: u32, bit_depth: Option<u8>) -> Self {
        Self {
            width,
            height,
            pixel_format: if bit_depth.is_some_and(|depth| depth > 8) {
                "yuv420p10le"
            } else {
                "yuv420p"
            },
        }
    }
}

fn seconds(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn push_range_input(arguments: &mut Vec<OsString>, range: Option<MetricRange>, input: &Path) {
    if let Some(range) = range {
        if range.start_seconds > 0.0 {
            arguments.push("-ss".into());
            arguments.push(seconds(range.start_seconds).into());
        }
        if let Some(duration) = range.duration_seconds {
            arguments.push("-t".into());
            arguments.push(seconds(duration).into());
        }
    }
    arguments.push("-i".into());
    arguments.push(input.as_os_str().to_owned());
}

/// The FFmpeg arguments that measure `distorted` against the reference. The distorted clip is
/// the main input (input 0) and the reference the second, as libvmaf expects. Log files are
/// plain relative names: run the command with the scratch directory as its working directory.
pub fn build_arguments(
    settings: &MetricsSettings,
    distorted: &Path,
    geometry: Geometry,
) -> Result<Vec<OsString>, String> {
    let filters = settings.enabled_filters();
    let Some(last) = filters.len().checked_sub(1) else {
        return Err("Turn on at least one metric".to_owned());
    };
    let model = settings.model.resolve(geometry.width, geometry.height);
    let mut arguments: Vec<OsString> = [
        "-hide_banner",
        "-nostdin",
        "-nostats",
        "-v",
        "warning",
        "-progress",
        "pipe:1",
    ]
    .map(OsString::from)
    .into();
    push_range_input(&mut arguments, settings.range, distorted);
    push_range_input(&mut arguments, settings.range, &settings.reference);

    let pixel_format = geometry.pixel_format;
    let mut graph = vec![
        format!(
            "[0:v:0]scale={}:{}:flags=bicubic,format={pixel_format},setpts=PTS-STARTPTS[dis]",
            geometry.width, geometry.height
        ),
        format!("[1:v:0]format={pixel_format},setpts=PTS-STARTPTS[ref]"),
    ];
    let count = filters.len();
    if count > 1 {
        let outputs = |prefix: &str| {
            (0..count)
                .map(|index| format!("[{prefix}{index}]"))
                .collect::<String>()
        };
        graph.push(format!("[dis]split={count}{}", outputs("d")));
        graph.push(format!("[ref]split={count}{}", outputs("r")));
    }
    for (index, name) in filters.iter().enumerate() {
        let (dis, reference) = if count == 1 {
            ("dis".to_owned(), "ref".to_owned())
        } else {
            (format!("d{index}"), format!("r{index}"))
        };
        let options = match *name {
            "libvmaf" => format!(
                "libvmaf=model=version={model}:log_fmt=json:log_path={VMAF_LOG}:n_threads={}",
                settings.threads.max(1)
            ),
            "psnr" => format!("psnr=stats_file={PSNR_LOG}"),
            "ssim" => format!("ssim=stats_file={SSIM_LOG}"),
            _ => format!("xpsnr=stats_file={XPSNR_LOG}"),
        };
        graph.push(format!("[{dis}][{reference}]{options}[o{index}]"));
    }
    for index in 0..last {
        graph.push(format!("[o{index}]nullsink"));
    }
    arguments.push("-filter_complex".into());
    arguments.push(graph.join(";").into());
    arguments.push("-map".into());
    arguments.push(format!("[o{last}]").into());
    arguments.push("-f".into());
    arguments.push("null".into());
    arguments.push("-".into());
    Ok(arguments)
}

// ----- parsers -----

static XPSNR_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^n:\s*(\d+)\s+XPSNR y:\s*(\S+)\s+XPSNR u:\s*(\S+)\s+XPSNR v:\s*(\S+)")
        .expect("valid XPSNR pattern")
});

/// A number from a stats file; with `cap` set, `inf` becomes [`PSNR_CAP_DB`].
fn number(text: &str, cap: bool) -> Result<f64, String> {
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("unreadable number \"{text}\""))?;
    if value.is_nan() {
        return Err(format!("unreadable number \"{text}\""));
    }
    Ok(if cap { value.min(PSNR_CAP_DB) } else { value })
}

/// The `key:value` pairs of one stats line (psnr and ssim format).
fn key_values(line: &str) -> BTreeMap<&str, &str> {
    line.split_whitespace()
        .filter_map(|token| token.split_once(':'))
        .collect()
}

/// The value of `key` on a stats line.
fn field<'a>(values: &BTreeMap<&str, &'a str>, key: &str, line: &str) -> Result<&'a str, String> {
    values
        .get(key)
        .copied()
        .ok_or_else(|| format!("no {key} in \"{line}\""))
}

/// 0-based frame number from a 1-based `n:` value.
fn frame_of(text: &str, line: &str) -> Result<usize, String> {
    text.parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .ok_or_else(|| format!("bad frame number in \"{line}\""))
}

/// Pooled values libvmaf writes at the end of its log.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pooled {
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    pub harmonic_mean: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VmafLog {
    /// (0-based frame, VMAF score).
    pub frames: Vec<(usize, f64)>,
    pub pooled: Option<Pooled>,
}

/// Reads libvmaf's JSON log (`log_fmt=json`).
pub fn parse_vmaf_json(text: &str) -> Result<VmafLog, String> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("The VMAF log is not JSON: {error}"))?;
    let entries = root["frames"]
        .as_array()
        .ok_or_else(|| "The VMAF log has no frames".to_owned())?;
    let frames = entries
        .iter()
        .map(|entry| {
            let frame = entry["frameNum"]
                .as_u64()
                .ok_or_else(|| "A VMAF frame has no frameNum".to_owned())?;
            let score = entry["metrics"]["vmaf"]
                .as_f64()
                .ok_or_else(|| format!("VMAF frame {frame} has no vmaf score"))?;
            Ok((frame as usize, score))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let pooled = root["pooled_metrics"]["vmaf"].as_object().and_then(|pool| {
        Some(Pooled {
            mean: pool.get("mean")?.as_f64()?,
            min: pool.get("min")?.as_f64()?,
            max: pool.get("max")?.as_f64()?,
            harmonic_mean: pool.get("harmonic_mean")?.as_f64()?,
        })
    });
    Ok(VmafLog { frames, pooled })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PsnrFrame {
    pub frame: usize,
    /// `psnr_avg`, in dB.
    pub psnr: f64,
    pub mse: f64,
}

/// Reads the `psnr` filter's stats file.
pub fn parse_psnr_log(text: &str) -> Result<Vec<PsnrFrame>, String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let values = key_values(line);
            Ok(PsnrFrame {
                frame: frame_of(field(&values, "n", line)?, line)?,
                psnr: number(field(&values, "psnr_avg", line)?, true)?,
                mse: number(field(&values, "mse_avg", line)?, false)?,
            })
        })
        .collect()
}

/// Reads the `ssim` filter's stats file: the `All` value of each frame.
pub fn parse_ssim_log(text: &str) -> Result<Vec<(usize, f64)>, String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let values = key_values(line);
            Ok((
                frame_of(field(&values, "n", line)?, line)?,
                number(field(&values, "All", line)?, false)?,
            ))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XpsnrFrame {
    pub frame: usize,
    pub y: f64,
    pub u: f64,
    pub v: f64,
}

impl XpsnrFrame {
    /// One value per frame. FFmpeg logs the three planes only; they are pooled here as
    /// (4Y + U + V) / 6 in the squared-error domain, the luma-heavy weighting of 4:2:0 video.
    pub fn combined(&self) -> f64 {
        let noise = |db: f64| 10f64.powf(-db / 10.0);
        let pooled = (4.0 * noise(self.y) + noise(self.u) + noise(self.v)) / 6.0;
        (-10.0 * pooled.log10()).min(PSNR_CAP_DB)
    }
}

/// Reads the `xpsnr` filter's stats file. The closing average line is ignored.
pub fn parse_xpsnr_log(text: &str) -> Result<Vec<XpsnrFrame>, String> {
    text.lines()
        .filter(|line| line.trim_start().starts_with("n:"))
        .map(|line| {
            let captures = XPSNR_LINE
                .captures(line.trim())
                .ok_or_else(|| format!("unreadable XPSNR line \"{line}\""))?;
            Ok(XpsnrFrame {
                frame: frame_of(&captures[1], line)?,
                y: number(&captures[2], true)?,
                u: number(&captures[3], true)?,
                v: number(&captures[4], true)?,
            })
        })
        .collect()
}

// ----- rows and summaries -----

/// One frame's scores; a metric that was off or has no value for the frame is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameRow {
    pub frame: usize,
    pub vmaf: Option<f64>,
    pub psnr: Option<f64>,
    pub ssim: Option<f64>,
    pub xpsnr: Option<f64>,
}

/// Joins the per-metric lists on the frame number.
pub fn merge_rows(
    vmaf: &[(usize, f64)],
    psnr: &[PsnrFrame],
    ssim: &[(usize, f64)],
    xpsnr: &[XpsnrFrame],
) -> Vec<FrameRow> {
    fn at(rows: &mut BTreeMap<usize, FrameRow>, frame: usize) -> &mut FrameRow {
        rows.entry(frame).or_insert(FrameRow {
            frame,
            ..FrameRow::default()
        })
    }
    let mut rows: BTreeMap<usize, FrameRow> = BTreeMap::new();
    for &(frame, value) in vmaf {
        at(&mut rows, frame).vmaf = Some(value);
    }
    for entry in psnr {
        at(&mut rows, entry.frame).psnr = Some(entry.psnr);
    }
    for &(frame, value) in ssim {
        at(&mut rows, frame).ssim = Some(value);
    }
    for entry in xpsnr {
        at(&mut rows, entry.frame).xpsnr = Some(entry.combined());
    }
    rows.into_values().collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub mean: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VmafStats {
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    /// `1 / mean(1 / (1 + score)) - 1`, the pooling libvmaf uses.
    pub harmonic_mean: f64,
    /// 1st percentile (linear interpolation between ranks).
    pub p1: f64,
    /// 5th percentile.
    pub p5: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileSummary {
    pub frames: usize,
    /// VMAF model that scored the file; `None` when VMAF was off.
    pub model: Option<String>,
    pub vmaf: Option<VmafStats>,
    pub psnr: Option<Stats>,
    pub ssim: Option<Stats>,
    pub xpsnr: Option<Stats>,
}

/// Percentile `p` (0..=100) of a non-empty list sorted ascending.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = p / 100.0 * (sorted.len() - 1) as f64;
    let (below, above) = (rank.floor() as usize, rank.ceil() as usize);
    sorted[below] + (sorted[above] - sorted[below]) * (rank - below as f64)
}

fn stats(values: &[f64]) -> Option<Stats> {
    if values.is_empty() {
        return None;
    }
    Some(Stats {
        mean: values.iter().sum::<f64>() / values.len() as f64,
        min: values.iter().copied().fold(f64::INFINITY, f64::min),
        max: values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    })
}

fn vmaf_stats(values: &[f64]) -> Option<VmafStats> {
    let base = stats(values)?;
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let inverse_sum: f64 = values.iter().map(|score| 1.0 / (1.0 + score)).sum();
    Some(VmafStats {
        mean: base.mean,
        min: base.min,
        max: base.max,
        harmonic_mean: values.len() as f64 / inverse_sum - 1.0,
        p1: percentile(&sorted, 1.0),
        p5: percentile(&sorted, 5.0),
    })
}

/// Pools the per-frame rows of one file.
pub fn summarize(rows: &[FrameRow], model: Option<String>) -> FileSummary {
    let column =
        |pick: fn(&FrameRow) -> Option<f64>| -> Vec<f64> { rows.iter().filter_map(pick).collect() };
    FileSummary {
        frames: rows.len(),
        model,
        vmaf: vmaf_stats(&column(|row| row.vmaf)),
        psnr: stats(&column(|row| row.psnr)),
        ssim: stats(&column(|row| row.ssim)),
        xpsnr: stats(&column(|row| row.xpsnr)),
    }
}

// ----- CSV -----

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

fn csv_number(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.6}"))
}

/// Per-frame CSV of one file: `frame,vmaf,psnr,ssim,xpsnr`, 0-based frames, empty cells for a
/// metric that was off.
pub fn frames_csv(rows: &[FrameRow]) -> String {
    let mut text = String::from("frame,vmaf,psnr,ssim,xpsnr\n");
    for row in rows {
        text.push_str(&format!(
            "{},{},{},{},{}\n",
            row.frame,
            csv_number(row.vmaf),
            csv_number(row.psnr),
            csv_number(row.ssim),
            csv_number(row.xpsnr)
        ));
    }
    text
}

/// One summary row per file, named by the first element of each pair.
pub fn summary_csv(files: &[(String, FileSummary)]) -> String {
    let mut text = String::from(
        "file,frames,vmaf_model,vmaf_mean,vmaf_min,vmaf_max,vmaf_harmonic_mean,vmaf_p1,vmaf_p5,\
         psnr_mean,psnr_min,psnr_max,ssim_mean,ssim_min,ssim_max,xpsnr_mean,xpsnr_min,xpsnr_max\n",
    );
    for (name, summary) in files {
        let vmaf = summary.vmaf.as_ref();
        let triple = |stats: Option<&Stats>| {
            [
                csv_number(stats.map(|s| s.mean)),
                csv_number(stats.map(|s| s.min)),
                csv_number(stats.map(|s| s.max)),
            ]
        };
        let mut cells = vec![
            csv_field(name),
            summary.frames.to_string(),
            csv_field(summary.model.as_deref().unwrap_or("")),
            csv_number(vmaf.map(|v| v.mean)),
            csv_number(vmaf.map(|v| v.min)),
            csv_number(vmaf.map(|v| v.max)),
            csv_number(vmaf.map(|v| v.harmonic_mean)),
            csv_number(vmaf.map(|v| v.p1)),
            csv_number(vmaf.map(|v| v.p5)),
        ];
        cells.extend(triple(summary.psnr.as_ref()));
        cells.extend(triple(summary.ssim.as_ref()));
        cells.extend(triple(summary.xpsnr.as_ref()));
        text.push_str(&cells.join(","));
        text.push('\n');
    }
    text
}

// ----- running -----

/// Lets the UI cancel a run, or a worst-frame export, from another thread.
#[derive(Debug, Default)]
pub struct MetricsControl {
    cancelled: AtomicBool,
    child: Mutex<Option<Child>>,
}

impl MetricsControl {
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
pub enum MetricsError {
    Cancelled,
    Failed(String),
}

impl From<String> for MetricsError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl std::fmt::Display for MetricsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

/// Where a run stands: file `file_index` (0-based) of `file_count` is `file_fraction` done.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetricsProgress {
    pub file_index: usize,
    pub file_count: usize,
    pub file_fraction: f64,
}

impl MetricsProgress {
    /// Progress of the whole run, 0..=1.
    pub fn overall(&self) -> f64 {
        ((self.file_index as f64 + self.file_fraction) / self.file_count.max(1) as f64)
            .clamp(0.0, 1.0)
    }
}

/// Everything measured for one distorted file.
#[derive(Clone, Debug)]
pub struct FileMetrics {
    pub rows: Vec<FrameRow>,
    pub summary: FileSummary,
}

#[derive(Clone, Debug)]
pub struct FileReport {
    pub distorted: PathBuf,
    /// The measurements, or why this file could not be measured.
    pub outcome: Result<FileMetrics, String>,
}

impl FileReport {
    /// File name shown in tables and CSV.
    pub fn name(&self) -> String {
        self.distorted.file_name().map_or_else(
            || self.distorted.to_string_lossy().into_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

#[derive(Clone, Debug)]
pub struct MetricsOutcome {
    pub reference: PathBuf,
    pub range: Option<MetricRange>,
    pub files: Vec<FileReport>,
    /// Every FFmpeg command line that ran, as typed in a shell.
    pub commands: Vec<String>,
}

impl MetricsOutcome {
    /// Summary CSV over every file that was measured.
    pub fn summary_csv(&self) -> String {
        let files: Vec<(String, FileSummary)> = self
            .files
            .iter()
            .filter_map(|file| {
                let metrics = file.outcome.as_ref().ok()?;
                Some((file.name(), metrics.summary.clone()))
            })
            .collect();
        summary_csv(&files)
    }
}

/// Removes the scratch directory when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn create() -> Result<Self, String> {
        let id = NEXT_RUN.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("kestrel-metrics-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("Could not create a scratch folder: {error}"))?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs FFmpeg in `directory`, reading `-progress` output. `total_seconds` scales the progress
/// callback (0..=1). Blocking: call from a worker thread.
fn run_ffmpeg(
    ffmpeg: &Path,
    arguments: &[OsString],
    directory: &Path,
    total_seconds: Option<f64>,
    control: &MetricsControl,
    progress: &mut dyn FnMut(f64),
) -> Result<(), MetricsError> {
    if control.is_cancelled() {
        return Err(MetricsError::Cancelled);
    }
    let mut child = tools::command(ffmpeg)
        .args(arguments)
        .current_dir(directory)
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
            .map_err(|_| "The metrics process lock is poisoned".to_owned())?;
        if control.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(MetricsError::Cancelled);
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
            if let (Some(seconds), Some(total)) = (progress_seconds(&line), total_seconds) {
                progress((seconds / total).clamp(0.0, 0.99));
            }
        }
    }
    let diagnostic = diagnostic.join().unwrap_or_default();
    let mut child = control
        .child
        .lock()
        .map_err(|_| "The metrics process lock is poisoned".to_owned())?
        .take()
        .ok_or_else(|| "The FFmpeg process handle was lost".to_owned())?;
    let status = child
        .wait()
        .map_err(|error| format!("Could not wait for FFmpeg: {error}"))?;
    if control.is_cancelled() {
        return Err(MetricsError::Cancelled);
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
        return Err(MetricsError::Failed(format!("FFmpeg failed: {tail}")));
    }
    progress(1.0);
    Ok(())
}

/// True when FFmpeg's `-filters` listing offers the filter `name`.
fn lists_filter(listing: &str, name: &str) -> bool {
    listing
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(name))
}

fn check_filters(ffmpeg: &Path, wanted: &[&str]) -> Result<(), String> {
    let output = tools::command(ffmpeg)
        .args(["-hide_banner", "-filters"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Failed to start FFmpeg: {error}"))?;
    let listing = String::from_utf8_lossy(&output.stdout);
    for name in wanted {
        if !lists_filter(&listing, name) {
            return Err(format!(
                "This FFmpeg build has no {name} filter. Use a full FFmpeg build (the one \
                 Kestrel bundles has it)."
            ));
        }
    }
    Ok(())
}

fn read_log(directory: &Path, name: &str) -> Result<String, String> {
    std::fs::read_to_string(directory.join(name))
        .map_err(|error| format!("FFmpeg wrote no {name}: {error}"))
}

/// What one FFmpeg run needs besides the settings.
struct FileJob<'a> {
    ffmpeg: &'a Path,
    settings: &'a MetricsSettings,
    distorted: &'a Path,
    geometry: Geometry,
    total_seconds: Option<f64>,
}

fn measure_file(
    job: &FileJob,
    control: &MetricsControl,
    commands: &mut Vec<String>,
    progress: &mut dyn FnMut(f64),
) -> Result<FileMetrics, MetricsError> {
    let settings = job.settings;
    let scratch = Scratch::create()?;
    let arguments = build_arguments(settings, job.distorted, job.geometry)?;
    commands.push(tools::display_command(job.ffmpeg, &arguments));
    run_ffmpeg(
        job.ffmpeg,
        &arguments,
        &scratch.0,
        job.total_seconds,
        control,
        progress,
    )?;
    let vmaf = if settings.vmaf {
        parse_vmaf_json(&read_log(&scratch.0, VMAF_LOG)?)?.frames
    } else {
        Vec::new()
    };
    let psnr = if settings.psnr {
        parse_psnr_log(&read_log(&scratch.0, PSNR_LOG)?)?
    } else {
        Vec::new()
    };
    let ssim = if settings.ssim {
        parse_ssim_log(&read_log(&scratch.0, SSIM_LOG)?)?
    } else {
        Vec::new()
    };
    let xpsnr = if settings.xpsnr {
        parse_xpsnr_log(&read_log(&scratch.0, XPSNR_LOG)?)?
    } else {
        Vec::new()
    };
    let rows = merge_rows(&vmaf, &psnr, &ssim, &xpsnr);
    if rows.is_empty() {
        return Err(MetricsError::Failed(
            "FFmpeg measured no frames; check that both files contain video".to_owned(),
        ));
    }
    let model = settings.vmaf.then(|| {
        settings
            .model
            .resolve(job.geometry.width, job.geometry.height)
    });
    let summary = summarize(&rows, model);
    Ok(FileMetrics { rows, summary })
}

/// Measures every distorted file against the reference, one FFmpeg run per file. `progress`
/// receives updates while a file runs. A file that fails is reported in its [`FileReport`] and
/// the run goes on; only invalid settings, a missing tool or filter, an unreadable reference or a
/// cancel end the whole run. Blocking: call from a worker thread.
pub fn run_metrics(
    toolchain: &Toolchain,
    settings: &MetricsSettings,
    control: &MetricsControl,
    progress: &mut dyn FnMut(MetricsProgress),
) -> Result<MetricsOutcome, MetricsError> {
    settings.validate()?;
    let ffmpeg = toolchain
        .ffmpeg
        .as_ref()
        .map(|tool| tool.path.clone())
        .ok_or_else(|| "FFmpeg was not found".to_owned())?;
    let ffprobe = toolchain
        .ffprobe
        .as_ref()
        .map(|tool| tool.path.clone())
        .ok_or_else(|| "FFprobe was not found".to_owned())?;
    if !settings.reference.is_file() {
        return Err(MetricsError::Failed(format!(
            "The reference file does not exist: {}",
            settings.reference.display()
        )));
    }
    check_filters(&ffmpeg, &settings.enabled_filters())?;

    let mut absolute = settings.clone();
    absolute.reference = absolute_path(&settings.reference)?;
    let metadata = mkv::read_metadata(None, Some(&ffprobe), &absolute.reference)
        .map_err(|error| format!("Could not read the reference: {error}"))?;
    let video = metadata
        .video
        .as_ref()
        .ok_or_else(|| "The reference has no video stream".to_owned())?;
    let (Some(width), Some(height)) = (video.width, video.height) else {
        return Err(MetricsError::Failed(
            "The reference size could not be read".to_owned(),
        ));
    };
    let geometry = Geometry::of_reference(width, height, video.bit_depth);
    let range = settings.range;
    let total_seconds = metadata
        .duration
        .map(|duration| {
            let available = (duration - range.map_or(0.0, |range| range.start_seconds)).max(0.0);
            range
                .and_then(|range| range.duration_seconds)
                .map_or(available, |wanted| wanted.min(available))
        })
        .filter(|seconds| *seconds > 0.0);

    let count = settings.distorted.len();
    let mut commands = Vec::new();
    let mut files = Vec::with_capacity(count);
    for (index, path) in settings.distorted.iter().enumerate() {
        if control.is_cancelled() {
            return Err(MetricsError::Cancelled);
        }
        let mut report = |fraction: f64| {
            progress(MetricsProgress {
                file_index: index,
                file_count: count,
                file_fraction: fraction,
            });
        };
        report(0.0);
        let outcome = absolute_path(path)
            .map_err(MetricsError::Failed)
            .and_then(|input| {
                let job = FileJob {
                    ffmpeg: &ffmpeg,
                    settings: &absolute,
                    distorted: &input,
                    geometry,
                    total_seconds,
                };
                measure_file(&job, control, &mut commands, &mut report)
            });
        match outcome {
            Ok(metrics) => files.push(FileReport {
                distorted: path.clone(),
                outcome: Ok(metrics),
            }),
            Err(MetricsError::Cancelled) => return Err(MetricsError::Cancelled),
            Err(MetricsError::Failed(message)) => files.push(FileReport {
                distorted: path.clone(),
                outcome: Err(message),
            }),
        }
    }
    Ok(MetricsOutcome {
        reference: settings.reference.clone(),
        range,
        files,
        commands,
    })
}

// ----- worst frames -----

/// The `count` lowest-scoring frames, worst first, scored by VMAF or, when no frame has a VMAF
/// score, by PSNR. Each entry is (frame, score).
pub fn worst_frames(rows: &[FrameRow], count: usize) -> Result<Vec<(usize, f64)>, String> {
    let by_vmaf: Vec<(usize, f64)> = rows
        .iter()
        .filter_map(|row| Some((row.frame, row.vmaf?)))
        .collect();
    let mut scored = if by_vmaf.is_empty() {
        rows.iter()
            .filter_map(|row| Some((row.frame, row.psnr?)))
            .collect()
    } else {
        by_vmaf
    };
    if scored.is_empty() {
        return Err("Worst frames need VMAF or PSNR in the results".to_owned());
    }
    scored.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    scored.truncate(count.max(1));
    Ok(scored)
}

/// Arguments that write the given frames of `distorted` as PNGs named `shot001.png`,
/// `shot002.png` and so on, in frame order. `frames` are 0-based and counted from the range
/// start, as in the measurements.
pub fn worst_frame_arguments(
    distorted: &Path,
    range: Option<MetricRange>,
    frames: &[usize],
) -> Vec<OsString> {
    let mut sorted = frames.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let selection = sorted
        .iter()
        .map(|frame| format!("eq(n,{frame})"))
        .collect::<Vec<_>>()
        .join("+");
    let mut arguments: Vec<OsString> = ["-hide_banner", "-nostdin", "-v", "error", "-y"]
        .map(OsString::from)
        .into();
    push_range_input(&mut arguments, range, distorted);
    arguments.extend(
        [
            "-vf".to_owned(),
            format!("select='{selection}'"),
            "-fps_mode".to_owned(),
            "passthrough".to_owned(),
            "-frames:v".to_owned(),
            sorted.len().to_string(),
            SHOT_PATTERN.to_owned(),
        ]
        .map(OsString::from),
    );
    arguments
}

/// Writes the PNG of each frame as `<stem>_frame<N>.png` into `output_directory` (created if
/// needed) and returns the files. The command line is appended to `commands`.
pub fn export_worst_frames(
    ffmpeg: &Path,
    distorted: &Path,
    range: Option<MetricRange>,
    frames: &[usize],
    output_directory: &Path,
    control: &MetricsControl,
    commands: &mut Vec<String>,
) -> Result<Vec<PathBuf>, MetricsError> {
    let mut sorted = frames.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.is_empty() {
        return Err(MetricsError::Failed("No frames to export".to_owned()));
    }
    let input = absolute_path(distorted)?;
    let stem = distorted
        .file_stem()
        .map_or_else(|| "frame".into(), |stem| stem.to_string_lossy())
        .into_owned();
    std::fs::create_dir_all(output_directory)
        .map_err(|error| format!("Could not create {}: {error}", output_directory.display()))?;
    let scratch = Scratch::create()?;
    let arguments = worst_frame_arguments(&input, range, &sorted);
    commands.push(tools::display_command(ffmpeg, &arguments));
    run_ffmpeg(ffmpeg, &arguments, &scratch.0, None, control, &mut |_| {})?;
    let mut shots: Vec<PathBuf> = std::fs::read_dir(&scratch.0)
        .map_err(|error| format!("Could not read the scratch folder: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with("shot") && name.ends_with(".png")
            })
        })
        .collect();
    shots.sort();
    if shots.len() != sorted.len() {
        return Err(MetricsError::Failed(format!(
            "FFmpeg wrote {} of {} frames; the rest are past the end of the clip",
            shots.len(),
            sorted.len()
        )));
    }
    let mut written = Vec::with_capacity(shots.len());
    for (shot, frame) in shots.iter().zip(&sorted) {
        let target = output_directory.join(format!("{stem}_frame{frame}.png"));
        if std::fs::rename(shot, &target).is_err() {
            std::fs::copy(shot, &target)
                .map_err(|error| format!("Could not write {}: {error}", target.display()))?;
        }
        written.push(target);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from FFmpeg 9.1 (gyan full build): a 320x240 testsrc clip at crf 10 measured
    // against its crf 40 re-encode. Kept to the first frames, and the VMAF log to the keys the
    // parser reads.
    const VMAF_JSON: &str = r#"{
  "version": "db23fdd",
  "fps": 205.13,
  "frames": [
    { "frameNum": 0, "metrics": { "integer_adm2": 0.973678, "integer_motion2": 0.000000, "vmaf": 83.228637 } },
    { "frameNum": 1, "metrics": { "integer_adm2": 0.975317, "integer_motion2": 0.100000, "vmaf": 84.609826 } },
    { "frameNum": 2, "metrics": { "integer_adm2": 0.976000, "integer_motion2": 0.200000, "vmaf": 85.101860 } }
  ],
  "pooled_metrics": {
    "integer_adm2": { "min": 0.973678, "max": 0.982405, "mean": 0.9785, "harmonic_mean": 0.978498 },
    "vmaf": { "min": 83.228637, "max": 86.964539, "mean": 85.910613, "harmonic_mean": 85.904218 }
  },
  "aggregate_metrics": {
  }
}"#;
    const PSNR_LOG_TEXT: &str = "\
n:1 mse_avg:20.41 mse_y:26.21 mse_u:9.16 mse_v:8.43 psnr_avg:35.03 psnr_y:33.95 psnr_u:38.51 psnr_v:38.87 \n\
n:2 mse_avg:19.14 mse_y:24.58 mse_u:8.70 mse_v:7.85 psnr_avg:35.31 psnr_y:34.23 psnr_u:38.74 psnr_v:39.18 \n\
n:3 mse_avg:18.73 mse_y:23.91 mse_u:8.70 mse_v:8.02 psnr_avg:35.41 psnr_y:34.34 psnr_u:38.74 psnr_v:39.09 \n";
    const SSIM_LOG_TEXT: &str = "\
n:1 Y:0.957362 U:0.985653 V:0.989934 All:0.967506 (14.881937)\n\
n:2 Y:0.960232 U:0.987044 V:0.990888 All:0.969810 (15.201378)\n\
n:3 Y:0.960907 U:0.987333 V:0.991077 All:0.970340 (15.278245)\n";
    const XPSNR_LOG_TEXT: &str = "\
n:    1  XPSNR y: 36.5517  XPSNR u: 41.0175  XPSNR v: 41.2525\n\
n:    2  XPSNR y: 28.2749  XPSNR u: 30.7703  XPSNR v: 30.9120\n\
n:    3  XPSNR y: 28.3675  XPSNR u: 30.8361  XPSNR v: 30.7882\n\
\n\
XPSNR average, 48 frames  y: 29.3112  u: 30.3065  v: 29.9415  (minimum: 29.3112)\n";
    // The same clip measured against itself.
    const PSNR_IDENTICAL: &str = "n:1 mse_avg:0.00 mse_y:0.00 mse_u:0.00 mse_v:0.00 psnr_avg:inf psnr_y:inf psnr_u:inf psnr_v:inf \n";
    const XPSNR_IDENTICAL: &str = "n:    1  XPSNR y: inf  XPSNR u: inf  XPSNR v: inf\n\nXPSNR average, 1 frames  y: inf  u: inf  v: inf  (minimum: inf)\n";
    const FILTER_LISTING: &str = "\
 ... psnr              VV->V      Calculate the PSNR between two video streams.
 .. xpsnr             VV->V      Calculate the XPSNR between two video streams.
 T. ssim              VV->V      Calculate the SSIM between two video streams.
 .. libvmaf           VV->V      Calculate the VMAF between two video streams.
";

    fn arguments(settings: &MetricsSettings, geometry: Geometry) -> Vec<String> {
        build_arguments(settings, Path::new("dis.mp4"), geometry)
            .unwrap()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    fn settings() -> MetricsSettings {
        let mut settings =
            MetricsSettings::new(PathBuf::from("ref.mp4"), vec![PathBuf::from("dis.mp4")]);
        settings.threads = 6;
        settings
    }

    fn hd() -> Geometry {
        Geometry::of_reference(1920, 1080, Some(8))
    }

    #[test]
    fn auto_model_follows_the_reference_size() {
        let auto = VmafModel::Auto;
        assert_eq!(auto.resolve(1920, 1080), MODEL_HD);
        assert_eq!(auto.resolve(3840, 1608), MODEL_4K, "wide scope 4K");
        assert_eq!(auto.resolve(2560, 2160), MODEL_4K);
        assert_eq!(auto.resolve(3840, 2160), MODEL_4K);
        assert_eq!(auto.resolve(2560, 1440), MODEL_HD);
        let named = VmafModel::Named("vmaf_v0.6.1neg".to_owned());
        assert_eq!(named.resolve(3840, 2160), "vmaf_v0.6.1neg");
    }

    #[test]
    fn four_metrics_build_one_graph_with_the_distorted_clip_first() {
        let args = arguments(&settings(), hd());
        let graph = "[0:v:0]scale=1920:1080:flags=bicubic,format=yuv420p,setpts=PTS-STARTPTS[dis];\
             [1:v:0]format=yuv420p,setpts=PTS-STARTPTS[ref];\
             [dis]split=4[d0][d1][d2][d3];[ref]split=4[r0][r1][r2][r3];\
             [d0][r0]libvmaf=model=version=vmaf_v0.6.1:log_fmt=json:log_path=vmaf.json:n_threads=6[o0];\
             [d1][r1]psnr=stats_file=psnr.log[o1];\
             [d2][r2]ssim=stats_file=ssim.log[o2];\
             [d3][r3]xpsnr=stats_file=xpsnr.log[o3];\
             [o0]nullsink;[o1]nullsink;[o2]nullsink";
        let expected = [
            "-hide_banner",
            "-nostdin",
            "-nostats",
            "-v",
            "warning",
            "-progress",
            "pipe:1",
            "-i",
            "dis.mp4",
            "-i",
            "ref.mp4",
            "-filter_complex",
            graph,
            "-map",
            "[o3]",
            "-f",
            "null",
            "-",
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn filters_and_logs_are_plain_relative_names() {
        let mut settings = settings();
        settings.reference = PathBuf::from(r"C:\Users\Some One\ref clip.mp4");
        let args = arguments(&settings, hd());
        let graph = args.iter().find(|a| a.contains("libvmaf")).unwrap();
        assert!(!graph.contains("C:") && !graph.contains('\\'), "{graph}");
        assert!(args.contains(&r"C:\Users\Some One\ref clip.mp4".to_owned()));
    }

    #[test]
    fn a_single_metric_needs_no_split_and_no_sink() {
        let mut settings = settings();
        (settings.vmaf, settings.ssim, settings.xpsnr) = (false, false, false);
        let args = arguments(&settings, hd());
        let graph = &args[args.iter().position(|a| a == "-filter_complex").unwrap() + 1];
        assert!(
            !graph.contains("split") && !graph.contains("nullsink"),
            "{graph}"
        );
        assert!(
            graph.ends_with("[dis][ref]psnr=stats_file=psnr.log[o0]"),
            "{graph}"
        );
        assert!(args.contains(&"[o0]".to_owned()));
    }

    #[test]
    fn range_is_applied_to_both_inputs_and_the_model_follows_the_reference() {
        let mut settings = settings();
        settings.range = Some(MetricRange {
            start_seconds: 12.5,
            duration_seconds: Some(30.0),
        });
        let args = arguments(&settings, Geometry::of_reference(3840, 2160, Some(10)));
        let first = args.iter().position(|a| a == "-i").unwrap();
        assert_eq!(
            args[first - 4..=first + 1],
            ["-ss", "12.5", "-t", "30", "-i", "dis.mp4"]
        );
        let second = args.iter().rposition(|a| a == "-i").unwrap();
        assert_eq!(
            args[second - 4..=second + 1],
            ["-ss", "12.5", "-t", "30", "-i", "ref.mp4"]
        );
        let graph = args.iter().find(|a| a.contains("libvmaf")).unwrap();
        assert!(graph.contains("version=vmaf_4k_v0.6.1:"), "{graph}");
        assert!(graph.contains("scale=3840:2160"), "{graph}");
        assert!(graph.contains("format=yuv420p10le"), "{graph}");

        settings.range = Some(MetricRange {
            start_seconds: 0.0,
            duration_seconds: None,
        });
        let args = arguments(&settings, hd());
        assert!(!args.contains(&"-ss".to_owned()) && !args.contains(&"-t".to_owned()));
        settings.model = VmafModel::Named(MODEL_4K.to_owned());
        let args = arguments(&settings, hd());
        assert!(args.iter().any(|a| a.contains("version=vmaf_4k_v0.6.1:")));
    }

    #[test]
    fn settings_that_cannot_run_are_refused() {
        let mut bad = settings();
        bad.distorted.clear();
        assert!(bad.validate().unwrap_err().contains("at least one file"));
        let mut bad = settings();
        (bad.vmaf, bad.psnr, bad.ssim, bad.xpsnr) = (false, false, false, false);
        assert!(bad.validate().unwrap_err().contains("at least one metric"));
        assert!(build_arguments(&bad, Path::new("d.mp4"), hd()).is_err());
        let mut bad = settings();
        bad.range = Some(MetricRange {
            start_seconds: -1.0,
            duration_seconds: None,
        });
        assert!(bad.validate().is_err());
        bad.range = Some(MetricRange {
            start_seconds: 0.0,
            duration_seconds: Some(0.0),
        });
        assert!(bad.validate().is_err());
        let mut bad = settings();
        bad.model = VmafModel::Named("x:y".to_owned());
        assert!(bad.validate().is_err());
        assert!(settings().validate().is_ok());
    }

    #[test]
    fn vmaf_json_gives_frames_and_pooled_values() {
        let log = parse_vmaf_json(VMAF_JSON).unwrap();
        assert_eq!(log.frames, [(0, 83.228637), (1, 84.609826), (2, 85.10186)]);
        let pooled = log.pooled.unwrap();
        assert_eq!((pooled.min, pooled.max), (83.228637, 86.964539));
        assert_eq!((pooled.mean, pooled.harmonic_mean), (85.910613, 85.904218));
        assert!(parse_vmaf_json("not json").is_err());
        assert!(parse_vmaf_json("{}").is_err());
        let no_score = r#"{"frames":[{"frameNum":0,"metrics":{}}]}"#;
        assert!(
            parse_vmaf_json(no_score)
                .unwrap_err()
                .contains("no vmaf score")
        );
    }

    #[test]
    fn psnr_log_gives_zero_based_frames_and_caps_infinity() {
        let frames = parse_psnr_log(PSNR_LOG_TEXT).unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(
            frames[0],
            PsnrFrame {
                frame: 0,
                psnr: 35.03,
                mse: 20.41
            }
        );
        assert_eq!(frames[2].frame, 2);
        let identical = parse_psnr_log(PSNR_IDENTICAL).unwrap();
        assert_eq!(identical[0].psnr, PSNR_CAP_DB);
        assert_eq!(identical[0].mse, 0.0);
        assert!(parse_psnr_log("n:1 mse_avg:1.0\n").is_err());
        assert!(parse_psnr_log("n:0 mse_avg:1 psnr_avg:30\n").is_err());
    }

    #[test]
    fn ssim_log_gives_the_all_value() {
        let frames = parse_ssim_log(SSIM_LOG_TEXT).unwrap();
        assert_eq!(frames, [(0, 0.967506), (1, 0.96981), (2, 0.97034)]);
        assert!(parse_ssim_log("n:1 Y:0.9\n").is_err());
    }

    #[test]
    fn xpsnr_log_skips_the_average_line_and_pools_planes() {
        let frames = parse_xpsnr_log(XPSNR_LOG_TEXT).unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(
            frames[1],
            XpsnrFrame {
                frame: 1,
                y: 28.2749,
                u: 30.7703,
                v: 30.912
            }
        );
        // Luma weighs most: the pooled value stays close to the luma value.
        let combined = frames[1].combined();
        assert!(combined > 28.2749 && combined < 29.0, "{combined}");
        let flat = XpsnrFrame {
            frame: 0,
            y: 40.0,
            u: 40.0,
            v: 40.0,
        };
        assert!((flat.combined() - 40.0).abs() < 1e-9);
        let identical = parse_xpsnr_log(XPSNR_IDENTICAL).unwrap();
        assert_eq!(identical[0].combined(), PSNR_CAP_DB);
        assert!(parse_xpsnr_log("n: x\n").is_err());
    }

    #[test]
    fn rows_join_the_metrics_by_frame() {
        let rows = merge_rows(
            &parse_vmaf_json(VMAF_JSON).unwrap().frames,
            &parse_psnr_log(PSNR_LOG_TEXT).unwrap()[..2],
            &parse_ssim_log(SSIM_LOG_TEXT).unwrap(),
            &[],
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].frame, 0);
        assert_eq!(rows[0].vmaf, Some(83.228637));
        assert_eq!(rows[1].psnr, Some(35.31));
        assert_eq!(rows[2].psnr, None, "the PSNR list ended one frame early");
        assert_eq!(rows[2].ssim, Some(0.97034));
        assert!(rows.iter().all(|row| row.xpsnr.is_none()));
    }

    fn row(frame: usize, vmaf: Option<f64>, psnr: Option<f64>) -> FrameRow {
        FrameRow {
            frame,
            vmaf,
            psnr,
            ..FrameRow::default()
        }
    }

    #[test]
    fn summary_math_matches_hand_values() {
        let rows: Vec<FrameRow> = [10.0, 20.0, 30.0, 40.0, 50.0]
            .iter()
            .enumerate()
            .map(|(frame, score)| row(frame, Some(*score), Some(score + 20.0)))
            .collect();
        let summary = summarize(&rows, Some(MODEL_HD.to_owned()));
        let vmaf = summary.vmaf.unwrap();
        assert_eq!(summary.frames, 5);
        assert_eq!((vmaf.mean, vmaf.min, vmaf.max), (30.0, 10.0, 50.0));
        // Linear rank interpolation: rank = p / 100 * (n - 1).
        assert!((vmaf.p1 - 10.4).abs() < 1e-9, "{}", vmaf.p1);
        assert!((vmaf.p5 - 12.0).abs() < 1e-9, "{}", vmaf.p5);
        let inverse: f64 = [11.0, 21.0, 31.0, 41.0, 51.0].iter().map(|x| 1.0 / x).sum();
        assert!((vmaf.harmonic_mean - (5.0 / inverse - 1.0)).abs() < 1e-9);
        assert!(vmaf.harmonic_mean < vmaf.mean);
        let psnr = summary.psnr.unwrap();
        assert_eq!((psnr.mean, psnr.min, psnr.max), (50.0, 30.0, 70.0));
        assert!(summary.ssim.is_none() && summary.xpsnr.is_none());
        // A constant score has every statistic equal to it.
        let flat = summarize(&[row(0, Some(50.0), None), row(1, Some(50.0), None)], None);
        let flat = flat.vmaf.unwrap();
        assert!((flat.harmonic_mean - 50.0).abs() < 1e-9 && flat.p1 == 50.0);
        // The fixture frames average to 84.313441.
        let log = parse_vmaf_json(VMAF_JSON).unwrap();
        let rows = merge_rows(&log.frames, &[], &[], &[]);
        let vmaf = summarize(&rows, None).vmaf.unwrap();
        assert!((vmaf.mean - 84.313441).abs() < 1e-5, "{}", vmaf.mean);
    }

    #[test]
    fn per_frame_csv_has_a_header_dot_decimals_and_empty_off_cells() {
        let rows = vec![
            FrameRow {
                frame: 0,
                vmaf: Some(83.228637),
                psnr: Some(35.03),
                ssim: None,
                xpsnr: None,
            },
            FrameRow {
                frame: 1,
                vmaf: Some(84.5),
                psnr: None,
                ssim: Some(0.97),
                xpsnr: Some(30.25),
            },
        ];
        assert_eq!(
            frames_csv(&rows),
            "frame,vmaf,psnr,ssim,xpsnr\n\
             0,83.228637,35.030000,,\n\
             1,84.500000,,0.970000,30.250000\n"
        );
    }

    #[test]
    fn summary_csv_has_one_row_per_file_and_quotes_awkward_names() {
        let rows = vec![
            row(0, Some(80.0), Some(30.0)),
            row(1, Some(90.0), Some(40.0)),
        ];
        let summary = summarize(&rows, Some(MODEL_HD.to_owned()));
        let text = summary_csv(&[
            ("a.mkv".to_owned(), summary.clone()),
            ("b, \"final\".mkv".to_owned(), summary),
        ]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("file,frames,vmaf_model,vmaf_mean,"));
        assert_eq!(
            lines[1],
            "a.mkv,2,vmaf_v0.6.1,85.000000,80.000000,90.000000,84.709302,80.100000,80.500000,\
             35.000000,30.000000,40.000000,,,,,,"
        );
        assert!(lines[2].starts_with("\"b, \"\"final\"\".mkv\",2,"));
        assert_eq!(lines[0].split(',').count(), lines[1].split(',').count());
    }

    #[test]
    fn worst_frames_prefer_vmaf_then_psnr() {
        let rows = vec![
            row(0, Some(90.0), Some(40.0)),
            row(1, Some(60.0), Some(41.0)),
            row(2, Some(70.0), Some(30.0)),
            row(3, Some(60.0), Some(42.0)),
        ];
        assert_eq!(
            worst_frames(&rows, 3).unwrap(),
            [(1, 60.0), (3, 60.0), (2, 70.0)]
        );
        let psnr_only: Vec<FrameRow> = rows.iter().map(|r| row(r.frame, None, r.psnr)).collect();
        assert_eq!(worst_frames(&psnr_only, 1).unwrap(), [(2, 30.0)]);
        assert!(worst_frames(&[row(0, None, None)], 5).is_err());
        assert_eq!(worst_frames(&rows, 99).unwrap().len(), 4);
    }

    #[test]
    fn worst_frame_command_selects_frames_inside_the_range() {
        let range = Some(MetricRange {
            start_seconds: 5.0,
            duration_seconds: Some(10.0),
        });
        let args: Vec<String> = worst_frame_arguments(Path::new("d.mp4"), range, &[9, 3, 3, 40])
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-hide_banner",
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-ss",
                "5",
                "-t",
                "10",
                "-i",
                "d.mp4",
                "-vf",
                "select='eq(n,3)+eq(n,9)+eq(n,40)'",
                "-fps_mode",
                "passthrough",
                "-frames:v",
                "3",
                "shot%03d.png",
            ]
        );
    }

    #[test]
    fn filter_listing_is_read_by_name() {
        assert!(lists_filter(FILTER_LISTING, "libvmaf"));
        assert!(lists_filter(FILTER_LISTING, "xpsnr"));
        assert!(
            !lists_filter(FILTER_LISTING, "vmaf"),
            "only whole names match"
        );
        assert!(!lists_filter(FILTER_LISTING, "scale"));
    }

    #[test]
    fn overall_progress_spans_the_files() {
        let at = |file_index, file_fraction| {
            MetricsProgress {
                file_index,
                file_count: 4,
                file_fraction,
            }
            .overall()
        };
        assert_eq!(at(0, 0.0), 0.0);
        assert_eq!(at(1, 0.5), 0.375);
        assert_eq!(at(3, 1.0), 1.0);
    }

    #[test]
    fn a_missing_ffmpeg_is_an_error_not_a_panic() {
        let result = run_metrics(
            &Toolchain::default(),
            &settings(),
            &MetricsControl::default(),
            &mut |_| {},
        );
        assert_eq!(
            result.unwrap_err(),
            MetricsError::Failed("FFmpeg was not found".to_owned())
        );
    }
}
