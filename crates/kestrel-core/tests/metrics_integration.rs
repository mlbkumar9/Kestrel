//! Real-FFmpeg checks for the quality metrics. Every test skips (and says so) when ffmpeg or
//! ffprobe is missing, or the build lacks one of the four metric filters.

use kestrel_core::metrics::{
    self, MetricRange, MetricsControl, MetricsError, MetricsOutcome, MetricsProgress,
    MetricsSettings, PSNR_CAP_DB,
};
use kestrel_core::tools::{self, Toolchain};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// The scratch-folder check looks at this process's temp folders, so the tests run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

struct Env {
    _serial: MutexGuard<'static, ()>,
    toolchain: Toolchain,
    ffmpeg: PathBuf,
    directory: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn env(name: &str) -> Option<Env> {
    let serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let toolchain = Toolchain::discover();
    let (Some(ffmpeg), Some(_)) = (&toolchain.ffmpeg, &toolchain.ffprobe) else {
        println!("SKIP: needs ffmpeg and ffprobe on PATH");
        return None;
    };
    let listing = tools::command(&ffmpeg.path)
        .args(["-hide_banner", "-filters"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let encoders = tools::command(&ffmpeg.path)
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    for filter in ["libvmaf", "psnr", "ssim", "xpsnr", "testsrc"] {
        if !listing
            .lines()
            .any(|line| line.split_whitespace().nth(1) == Some(filter))
        {
            println!("SKIP: this FFmpeg has no {filter} filter");
            return None;
        }
    }
    if !encoders.contains("libx264") {
        println!("SKIP: libx264 is not available");
        return None;
    }
    let directory =
        std::env::temp_dir().join(format!("kestrel-metrics-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("scratch directory");
    Some(Env {
        _serial: serial,
        ffmpeg: ffmpeg.path.clone(),
        toolchain,
        directory,
    })
}

fn ffmpeg(env: &Env, arguments: &[&str]) {
    let output = tools::command(&env.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(arguments)
        .current_dir(&env.directory)
        .output()
        .expect("ffmpeg runs");
    assert!(
        output.status.success(),
        "ffmpeg {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A 2 s, 24 fps, 320x240 reference and an x264 crf 40 copy of it (48 frames each).
fn clips(env: &Env) -> (PathBuf, PathBuf) {
    ffmpeg(
        env,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=24:duration=2",
            "-c:v",
            "libx264",
            "-crf",
            "10",
            "-pix_fmt",
            "yuv420p",
            "ref.mp4",
        ],
    );
    ffmpeg(
        env,
        &[
            "-i", "ref.mp4", "-c:v", "libx264", "-crf", "40", "-pix_fmt", "yuv420p", "dis.mp4",
        ],
    );
    (env.directory.join("ref.mp4"), env.directory.join("dis.mp4"))
}

fn run(env: &Env, settings: &MetricsSettings) -> MetricsOutcome {
    metrics::run_metrics(
        &env.toolchain,
        settings,
        &MetricsControl::default(),
        &mut |_| {},
    )
    .expect("the run finishes")
}

#[test]
fn a_crf_40_encode_scores_below_its_reference_on_every_metric() {
    let Some(env) = env("scores") else { return };
    let (reference, distorted) = clips(&env);
    let mut updates: Vec<MetricsProgress> = Vec::new();
    let settings = MetricsSettings::new(reference, vec![distorted]);
    let outcome = metrics::run_metrics(
        &env.toolchain,
        &settings,
        &MetricsControl::default(),
        &mut |progress| updates.push(progress),
    )
    .expect("the run finishes");

    assert_eq!(outcome.files.len(), 1);
    let file = outcome.files[0].outcome.as_ref().expect("file measured");
    let summary = &file.summary;
    assert_eq!(summary.frames, 48);
    assert_eq!(file.rows.len(), 48);
    let vmaf = summary.vmaf.expect("VMAF");
    assert!(vmaf.mean > 0.0 && vmaf.mean < 100.0, "VMAF {}", vmaf.mean);
    assert!(vmaf.min <= vmaf.p1 && vmaf.p1 <= vmaf.p5 && vmaf.p5 <= vmaf.mean);
    assert_eq!(summary.model.as_deref(), Some(metrics::MODEL_HD));
    let psnr = summary.psnr.expect("PSNR");
    assert!(psnr.mean.is_finite() && psnr.mean > 20.0 && psnr.mean < PSNR_CAP_DB);
    let ssim = summary.ssim.expect("SSIM");
    assert!(ssim.mean > 0.0 && ssim.mean <= 1.0, "SSIM {}", ssim.mean);
    let xpsnr = summary.xpsnr.expect("XPSNR");
    assert!(xpsnr.mean.is_finite() && xpsnr.mean > 10.0 && xpsnr.mean < PSNR_CAP_DB);
    assert!(
        file.rows
            .iter()
            .all(|row| row.vmaf.is_some() && row.psnr.is_some() && row.ssim.is_some())
    );
    println!(
        "frames {} VMAF mean {:.3} min {:.3} p1 {:.3} harmonic {:.3} | PSNR {:.3} | SSIM {:.5} | XPSNR {:.3}",
        summary.frames,
        vmaf.mean,
        vmaf.min,
        vmaf.p1,
        vmaf.harmonic_mean,
        psnr.mean,
        ssim.mean,
        xpsnr.mean
    );

    assert_eq!(outcome.commands.len(), 1);
    assert!(outcome.commands[0].contains("libvmaf"), "command logged");
    assert!(updates.first().is_some_and(|p| p.overall() == 0.0));
    assert!(updates.last().is_some_and(|p| p.overall() >= 0.99));
    assert!(updates.windows(2).all(|w| w[0].overall() <= w[1].overall()));
    assert!(
        fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("kestrel-metrics-{}-", std::process::id()))),
        "scratch folders are removed"
    );

    let csv = metrics::frames_csv(&file.rows);
    assert_eq!(csv.lines().count(), 49);
    let summary_csv = outcome.summary_csv();
    assert_eq!(summary_csv.lines().count(), 2);
    assert!(summary_csv.contains("dis.mp4,48,vmaf_v0.6.1,"));
}

#[test]
fn identical_clips_score_near_perfect() {
    let Some(env) = env("identical") else { return };
    let (reference, _) = clips(&env);
    let settings = MetricsSettings::new(reference.clone(), vec![reference]);
    let outcome = run(&env, &settings);
    let summary = &outcome.files[0].outcome.as_ref().unwrap().summary;
    let vmaf = summary.vmaf.unwrap();
    assert!(vmaf.mean > 95.0, "identical VMAF {}", vmaf.mean);
    assert!((summary.ssim.unwrap().mean - 1.0).abs() < 1e-6);
    assert_eq!(summary.psnr.unwrap().mean, PSNR_CAP_DB);
    println!("identical VMAF mean {:.3}", vmaf.mean);
}

#[test]
fn a_range_measures_only_that_part_and_two_files_run_in_turn() {
    let Some(env) = env("range") else { return };
    let (reference, distorted) = clips(&env);
    let mut settings = MetricsSettings::new(reference.clone(), vec![distorted, reference]);
    settings.range = Some(MetricRange {
        start_seconds: 0.5,
        duration_seconds: Some(1.0),
    });
    settings.xpsnr = false;
    let outcome = run(&env, &settings);
    assert_eq!(outcome.files.len(), 2);
    for report in &outcome.files {
        let file = report.outcome.as_ref().expect("measured");
        assert_eq!(file.summary.frames, 24, "1 s at 24 fps");
        assert!(file.summary.xpsnr.is_none());
    }
    assert_eq!(outcome.commands.len(), 2);
    assert!(outcome.commands[0].contains("-ss 0.5 -t 1"));
}

#[test]
fn a_file_that_is_not_video_fails_alone() {
    let Some(env) = env("broken") else { return };
    let (reference, distorted) = clips(&env);
    let junk = env.directory.join("junk.mp4");
    fs::write(&junk, b"not a video").unwrap();
    let mut settings = MetricsSettings::new(reference, vec![junk, distorted]);
    (settings.psnr, settings.ssim, settings.xpsnr) = (false, false, false);
    let outcome = run(&env, &settings);
    let message = outcome.files[0].outcome.as_ref().unwrap_err();
    assert!(message.contains("FFmpeg failed"), "{message}");
    assert!(outcome.files[1].outcome.is_ok());
}

#[test]
fn cancelling_before_the_run_returns_cancelled() {
    let Some(env) = env("cancel") else { return };
    let (reference, distorted) = clips(&env);
    let control = MetricsControl::default();
    control.cancel();
    let result = metrics::run_metrics(
        &env.toolchain,
        &MetricsSettings::new(reference, vec![distorted]),
        &control,
        &mut |_| {},
    );
    assert_eq!(result.unwrap_err(), MetricsError::Cancelled);
}

#[test]
fn worst_frames_are_written_as_named_pngs() {
    let Some(env) = env("worst") else { return };
    let (reference, distorted) = clips(&env);
    let settings = MetricsSettings::new(reference, vec![distorted.clone()]);
    let outcome = run(&env, &settings);
    let rows = &outcome.files[0].outcome.as_ref().unwrap().rows;
    let worst = metrics::worst_frames(rows, 5).unwrap();
    assert_eq!(worst.len(), 5);
    let frames: Vec<usize> = worst.iter().map(|(frame, _)| *frame).collect();
    let out = env.directory.join("worst");
    let mut commands = Vec::new();
    let files = metrics::export_worst_frames(
        &env.ffmpeg,
        &distorted,
        None,
        &frames,
        &out,
        &MetricsControl::default(),
        &mut commands,
    )
    .expect("export");
    assert_eq!(files.len(), 5);
    let mut sorted = frames.clone();
    sorted.sort_unstable();
    for (path, frame) in files.iter().zip(sorted) {
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("dis_frame{frame}.png")
        );
        assert!(path.metadata().unwrap().len() > 100, "{}", path.display());
        assert_png(path);
    }
    assert_eq!(commands.len(), 1);
}

fn assert_png(path: &Path) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{}", path.display());
}
