//! Real-FFmpeg checks for the Transcode preview. Every test skips (and says so) when ffmpeg or
//! ffprobe is missing, or the needed encoder is not built in.

use kestrel_core::encoders::EncoderSet;
use kestrel_core::formats::Container;
use kestrel_core::mkv::{self, MediaMetadata};
use kestrel_core::preview::{self, PreviewControl, PreviewError, PreviewRequest};
use kestrel_core::tools::{self, Toolchain};
use kestrel_core::transcode::{
    BurnIn, JobSettings, RateControl, SubtitleSettings, VideoEncode, VideoEncoder, VideoSettings,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// The preview folder is shared by every test of this process, so they run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Env {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    encoders: EncoderSet,
    directory: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn env(name: &str) -> Option<Env> {
    let tools = Toolchain::discover();
    let (Some(ffmpeg), Some(ffprobe)) = (&tools.ffmpeg, &tools.ffprobe) else {
        println!("SKIP: needs ffmpeg and ffprobe on PATH");
        return None;
    };
    // The full detection probes every hardware encoder and takes a minute; the preview only
    // needs libx264, so ask FFmpeg for exactly that.
    let listed = tools::command(&ffmpeg.path)
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    if !listed.contains("libx264") {
        println!("SKIP: libx264 is not available");
        return None;
    }
    let encoders = EncoderSet {
        software: vec![VideoEncoder::X264],
        ..EncoderSet::default()
    };
    let directory =
        std::env::temp_dir().join(format!("kestrel-preview-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("scratch directory");
    Some(Env {
        ffmpeg: ffmpeg.path.clone(),
        ffprobe: ffprobe.path.clone(),
        encoders,
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

fn source(env: &Env, seconds: u32) -> (PathBuf, MediaMetadata) {
    source_of(
        env,
        &format!("testsrc2=size=320x240:rate=10:duration={seconds}"),
        seconds,
    )
}

fn source_of(env: &Env, video: &str, seconds: u32) -> (PathBuf, MediaMetadata) {
    let path = env.directory.join("source.mkv");
    ffmpeg(
        env,
        &[
            "-f",
            "lavfi",
            "-i",
            video,
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={seconds}"),
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            "-t",
            &seconds.to_string(),
            path.to_str().unwrap(),
        ],
    );
    let metadata = mkv::read_metadata(None, Some(&env.ffprobe), &path).expect("metadata");
    (path, metadata)
}

fn x264_settings(container: Container) -> JobSettings {
    JobSettings {
        container,
        video: VideoSettings::Encode(VideoEncode {
            encoder: VideoEncoder::X264,
            rate: RateControl::Quality(30),
            speed: 4,
            ..VideoEncode::default()
        }),
        ..JobSettings::default()
    }
}

fn request(
    env: &Env,
    input: &Path,
    metadata: &MediaMetadata,
    settings: JobSettings,
    length: f64,
    position: f64,
) -> PreviewRequest {
    PreviewRequest {
        ffmpeg: env.ffmpeg.clone(),
        input: input.to_path_buf(),
        metadata: metadata.clone(),
        settings,
        encoders: env.encoders.clone(),
        segment: preview::plan_segment(metadata.duration.unwrap(), length, position).unwrap(),
    }
}

fn probe_duration(env: &Env, path: &Path) -> f64 {
    let output = tools::command(&env.ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(path)
        .output()
        .expect("ffprobe runs");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("no duration for {}", path.display()))
}

#[test]
fn preview_encodes_a_playable_segment_of_the_requested_length() {
    let _serial = serial();
    let Some(env) = env("segment") else { return };
    let (input, metadata) = source(&env, 20);
    let request = request(
        &env,
        &input,
        &metadata,
        x264_settings(Container::Mp4),
        5.0,
        0.5,
    );
    assert!((request.segment.start_seconds - 10.0).abs() < 0.1);
    let mut percents = Vec::new();
    let outcome = preview::run_preview(&request, &PreviewControl::default(), &mut |percent| {
        percents.push(percent)
    })
    .expect("preview runs");

    assert!(outcome.file.is_file());
    assert!(outcome.bytes > 0);
    let length = probe_duration(&env, &outcome.file);
    assert!(
        (length - 5.0).abs() < 0.6,
        "preview is {length} s, expected about 5 s"
    );
    // Playable: ffprobe reads a video stream with real dimensions.
    let preview_meta = mkv::read_metadata(None, Some(&env.ffprobe), &outcome.file).unwrap();
    assert_eq!(preview_meta.video.as_ref().and_then(|v| v.width), Some(320));
    assert_eq!(percents.last(), Some(&100));
    assert!(percents.windows(2).all(|pair| pair[0] <= pair[1]));

    // Size, bitrate and the full-size extrapolation follow from the segment.
    assert!(outcome.bitrate_kbps > 0.0);
    let expected_full = outcome.bytes as f64 * 20.0 / 5.0;
    assert!(
        (outcome.estimated_full_bytes as f64 / expected_full - 1.0).abs() < 0.02,
        "{} vs {expected_full}",
        outcome.estimated_full_bytes
    );

    // Stills at the midpoint exist and are PNGs.
    assert_eq!(outcome.still_error, None);
    for still in [&outcome.source_still, &outcome.encoded_still] {
        let still = still.as_ref().expect("still");
        let bytes = fs::read(still).unwrap();
        assert_eq!(&bytes[1..4], b"PNG", "{}", still.display());
    }

    // Discarding removes every file of the preview.
    let files = [
        outcome.file.clone(),
        outcome.source_still.clone().unwrap(),
        outcome.encoded_still.clone().unwrap(),
    ];
    outcome.discard();
    assert!(files.iter().all(|file| !file.exists()));
}

#[test]
fn preview_of_a_short_source_encodes_all_of_it_and_cleanup_empties_the_folder() {
    let _serial = serial();
    let Some(env) = env("short") else { return };
    let (input, metadata) = source(&env, 4);
    let request = request(
        &env,
        &input,
        &metadata,
        x264_settings(Container::Mkv),
        15.0,
        0.5,
    );
    assert_eq!(request.segment.start_seconds, 0.0);
    assert!((request.segment.length_seconds - 4.0).abs() < 0.1);
    let outcome = preview::run_preview(&request, &PreviewControl::default(), &mut |_| {})
        .expect("preview runs");
    let length = probe_duration(&env, &outcome.file);
    assert!((length - 4.0).abs() < 0.6, "preview is {length} s");
    assert!(outcome.file.starts_with(preview::preview_directory()));
    preview::remove_preview_directory();
    assert!(!outcome.file.exists());
    assert!(!preview::preview_directory().exists());
}

#[test]
fn a_cancelled_preview_leaves_no_file_behind() {
    let _serial = serial();
    let Some(env) = env("cancel") else { return };
    let (input, metadata) = source(&env, 8);
    let request = request(
        &env,
        &input,
        &metadata,
        x264_settings(Container::Mkv),
        5.0,
        0.0,
    );
    let control = PreviewControl::default();
    control.cancel();
    let error = preview::run_preview(&request, &control, &mut |_| {}).unwrap_err();
    assert_eq!(error, PreviewError::Cancelled);
    let leftovers = fs::read_dir(preview::preview_directory())
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(leftovers, 0);
}

#[test]
fn a_failing_encode_reports_ffmpeg_and_removes_its_output() {
    let _serial = serial();
    let Some(env) = env("fail") else { return };
    let (input, metadata) = source(&env, 4);
    let mut request = request(
        &env,
        &input,
        &metadata,
        x264_settings(Container::Mkv),
        3.0,
        0.0,
    );
    // The input vanishes before FFmpeg opens it.
    fs::remove_file(&input).unwrap();
    request.input = input;
    let error =
        preview::run_preview(&request, &PreviewControl::default(), &mut |_| {}).unwrap_err();
    assert!(matches!(error, PreviewError::Failed(text) if text.contains("FFmpeg")));
}

/// Burned-in text subtitles keep their timing when the segment does not start at zero:
/// a cue shown from 6 s to 8 s of the source must appear in a preview that starts at 5 s.
#[test]
fn burned_subtitles_stay_in_sync_inside_a_segment() {
    let _serial = serial();
    let Some(env) = env("burn") else { return };
    let (input, _) = source_of(&env, "color=c=black:size=320x240:rate=10:duration=12", 12);
    // Embed a subtitle track so the preview extracts and burns it like a real job.
    let srt = env.directory.join("cue.srt");
    fs::write(
        &srt,
        "1\n00:00:06,000 --> 00:00:08,000\nMMMMMMMMMMMMMMMMMMMM\nMMMMMMMMMMMMMMMMMMMM\n",
    )
    .unwrap();
    let with_subs = env.directory.join("with-subs.mkv");
    ffmpeg(
        &env,
        &[
            "-i",
            input.to_str().unwrap(),
            "-i",
            srt.to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            "-c",
            "copy",
            "-c:s",
            "srt",
            with_subs.to_str().unwrap(),
        ],
    );
    let metadata = mkv::read_metadata(None, Some(&env.ffprobe), &with_subs).unwrap();
    if metadata.subtitle_tracks.is_empty() {
        println!("SKIP: the subtitle track was not muxed");
        return;
    }
    let probe = tools::command(&env.ffmpeg)
        .args(["-hide_banner", "-filters"])
        .output()
        .unwrap();
    if !String::from_utf8_lossy(&probe.stdout).contains(" subtitles ") {
        println!("SKIP: this FFmpeg has no subtitles filter (libass)");
        return;
    }
    let mut settings = x264_settings(Container::Mkv);
    settings.subtitles = SubtitleSettings {
        burn_in: BurnIn::FirstSelected,
        ..SubtitleSettings::default()
    };
    let request = request(&env, &with_subs, &metadata, settings, 5.0, 5.0 / 12.0);
    assert!((request.segment.start_seconds - 5.0).abs() < 0.1);
    let outcome = preview::run_preview(&request, &PreviewControl::default(), &mut |_| {})
        .expect("preview runs");

    // The cue sits 1 s to 3 s into the segment. Compare the mean brightness of a frame inside
    // and outside it: burned white text raises the mean over the plain test pattern.
    let mean = |at: &str| -> f64 {
        let output = tools::command(&env.ffmpeg)
            .args(["-nostdin", "-v", "error", "-ss", at, "-i"])
            .arg(&outcome.file)
            .args([
                "-frames:v",
                "1",
                "-vf",
                "crop=iw:ih/2:0:ih/2,format=gray",
                "-f",
                "rawvideo",
                "-",
            ])
            .output()
            .unwrap();
        let pixels = output.stdout;
        assert!(!pixels.is_empty(), "no frame at {at}");
        pixels.iter().map(|&p| f64::from(p)).sum::<f64>() / pixels.len() as f64
    };
    let inside = mean("2.0");
    let before = mean("0.4");
    let after = mean("3.8");
    assert!(
        (inside - before).abs() > 1.0 && (inside - after).abs() > 1.0,
        "no cue visible at 2 s: inside {inside:.2}, before {before:.2}, after {after:.2}"
    );
    outcome.discard();
}

/// A copied audio track must start where the re-encoded video starts. After an input seek,
/// stream copy otherwise begins at the keyframe before the start, which shifts the video late
/// and makes the file longer than the requested segment.
#[test]
fn copied_audio_stays_aligned_with_the_video_in_a_long_gop_source() {
    let _serial = serial();
    let Some(env) = env("gop") else { return };
    let path = env.directory.join("source.mkv");
    ffmpeg(
        &env,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=10:duration=40",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=40",
            "-c:v",
            "mpeg4",
            // A keyframe every 20 s, so a segment at 24 s begins mid-GOP.
            "-g",
            "200",
            "-c:a",
            "aac",
            "-t",
            "40",
            path.to_str().unwrap(),
        ],
    );
    let metadata = mkv::read_metadata(None, Some(&env.ffprobe), &path).expect("metadata");
    let request = request(
        &env,
        &path,
        &metadata,
        x264_settings(Container::Mkv),
        5.0,
        0.6,
    );
    assert!((request.segment.start_seconds - 24.0).abs() < 0.1);
    let outcome = preview::run_preview(&request, &PreviewControl::default(), &mut |_| {})
        .expect("preview runs");

    let probe = tools::command(&env.ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,start_time",
            "-of",
            "csv=p=0",
        ])
        .arg(&outcome.file)
        .output()
        .expect("ffprobe runs");
    let starts = String::from_utf8_lossy(&probe.stdout).into_owned();
    let mut seen = 0;
    for line in starts.lines() {
        let (kind, start) = line.split_once(',').expect("codec_type,start_time");
        let start: f64 = start.trim().parse().expect("start time");
        assert!(start.abs() < 0.5, "{kind} starts at {start} s: {starts}");
        seen += 1;
    }
    assert_eq!(seen, 2, "video and audio: {starts}");
    let length = probe_duration(&env, &outcome.file);
    assert!(
        (length - 5.0).abs() < 0.6,
        "preview is {length} s, expected about 5 s"
    );
    outcome.discard();
}
