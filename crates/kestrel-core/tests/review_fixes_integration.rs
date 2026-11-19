//! Real FFmpeg regressions for the Kestrel 4 adversarial-review findings. Scratch media lives
//! under the system temp directory and is removed when each test finishes.

use kestrel_core::compat;
use kestrel_core::encoders::{self, EncoderSet};
use kestrel_core::external::{ExternalKind, ExternalTrack};
use kestrel_core::formats::Container;
use kestrel_core::inputs::ExistingPolicy;
use kestrel_core::mkv::{self, MediaMetadata};
use kestrel_core::remux_edit::{TrackEdit, TrackEdits};
use kestrel_core::tools::{self, Toolchain};
use kestrel_core::transcode::{
    BurnIn, Deinterlace, JobSettings, TenBit, VideoEncode, VideoEncoder, VideoSettings,
};
use kestrel_core::worker::{self, FileStatus, JobSpec, QueueSpec, RemuxEngine, WorkerEvent};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Env {
    tools: Toolchain,
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    encoders: EncoderSet,
    dir: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn env(name: &str) -> Option<Env> {
    let tools = Toolchain::discover();
    let (Some(ffmpeg), Some(ffprobe)) = (&tools.ffmpeg, &tools.ffprobe) else {
        println!("SKIP: review-fix integration tests need ffmpeg and ffprobe on PATH");
        return None;
    };
    let (ffmpeg, ffprobe) = (ffmpeg.path.clone(), ffprobe.path.clone());
    let encoders = encoders::detect(&ffmpeg).expect("encoders should be detected");
    let dir = std::env::temp_dir().join(format!(
        "kestrel-review-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    Some(Env {
        tools,
        ffmpeg,
        ffprobe,
        encoders,
        dir,
    })
}

fn ffmpeg(env: &Env, args: &[&str], output: &Path) {
    let result = tools::command(&env.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .arg(output)
        .output()
        .expect("ffmpeg runs");
    assert!(
        result.status.success(),
        "ffmpeg failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// MKV with mpeg4 video, a DTS (eng) track and an AAC (jpn) track; `dts_only` drops the AAC one.
fn dts_source(env: &Env, dts_only: bool) -> Option<PathBuf> {
    let path = env.dir.join("dts.mkv");
    let mut args = vec![
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x240:rate=10:duration=2",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=2",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=660:duration=2",
        "-map",
        "0:v",
        "-map",
        "1:a",
    ];
    if !dts_only {
        args.extend(["-map", "2:a"]);
    }
    args.extend([
        "-c:v",
        "mpeg4",
        "-c:a:0",
        "dca",
        "-strict",
        "-2",
        "-c:a:1",
        "aac",
        "-metadata:s:a:0",
        "language=eng",
        "-metadata:s:a:1",
        "language=jpn",
    ]);
    let result = tools::command(&env.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(&args)
        .arg(&path)
        .output()
        .expect("ffmpeg runs");
    if !result.status.success() {
        println!("SKIP: this FFmpeg build cannot encode DTS");
        return None;
    }
    Some(path)
}

fn metadata(env: &Env, path: &Path) -> MediaMetadata {
    mkv::read_metadata(None, Some(&env.ffprobe), path).expect("metadata")
}

struct Outcome {
    success: bool,
    messages: String,
}

fn run(env: &Env, input: &Path, output: &Path, settings: JobSettings) -> Outcome {
    let events = worker::run_queue_blocking(QueueSpec {
        jobs: vec![JobSpec {
            execution: None,
            id: 1,
            input: input.to_path_buf(),
            output: output.to_path_buf(),
            metadata: metadata(env, input),
            settings,
        }],
        tools: env.tools.clone(),
        encoders: env.encoders.clone(),
        remux_engine: RemuxEngine::Ffmpeg,
        existing: ExistingPolicy::Overwrite,
        dry_run: false,
    });
    let success = events.iter().any(|event| {
        matches!(
            event,
            WorkerEvent::JobUpdated {
                status: FileStatus::Success,
                ..
            }
        )
    });
    let messages = events
        .iter()
        .filter_map(|event| match event {
            WorkerEvent::Log(message) => Some(message.clone()),
            WorkerEvent::JobUpdated {
                detail: Some(message),
                ..
            } => Some(message.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    Outcome { success, messages }
}

fn streams(env: &Env, path: &Path) -> Vec<serde_json::Value> {
    let output = tools::command(&env.ffprobe)
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(path)
        .output()
        .expect("ffprobe runs");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("ffprobe json");
    json["streams"].as_array().cloned().unwrap_or_default()
}

fn of_type<'a>(streams: &'a [serde_json::Value], kind: &str) -> Vec<&'a serde_json::Value> {
    streams
        .iter()
        .filter(|stream| stream["codec_type"] == kind)
        .collect()
}

fn mp4_settings() -> JobSettings {
    JobSettings {
        container: Container::Mp4,
        ..JobSettings::default()
    }
}

#[test]
fn dts_remuxed_to_mp4_is_dropped_with_a_warning_not_converted() {
    let Some(env) = env("dts-drop") else {
        return;
    };
    let Some(source) = dts_source(&env, false) else {
        return;
    };
    let meta = metadata(&env, &source);
    assert_eq!(meta.audio_tracks[0].codec_id, "dts");

    // Plain remux path.
    let settings = mp4_settings();
    assert!(settings.is_remux_only());
    let warnings = compat::job_warnings(&meta, &settings);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("Audio track 1 will be dropped") && w.contains("DTS")),
        "{warnings:?}"
    );
    let plain = env.dir.join("plain.mp4");
    let outcome = run(&env, &source, &plain, settings);
    assert!(outcome.success, "{}", outcome.messages);
    let streams_out = streams(&env, &plain);
    let audio = of_type(&streams_out, "audio");
    assert_eq!(audio.len(), 1, "only the AAC track survives");
    assert_eq!(audio[0]["codec_name"], "aac");
    assert_eq!(audio[0]["tags"]["language"], "jpn");

    // Track-edit path (a retitle routes the job through the edit builder) keeps the title in
    // `handler_name`, the only stream title an MP4 stores.
    let mut edited = mp4_settings();
    edited.track_edits = TrackEdits {
        tracks: vec![TrackEdit {
            id: meta.audio_tracks[1].id,
            title: Some("Japanese".to_owned()),
            ..TrackEdit::default()
        }],
        ..TrackEdits::default()
    };
    let edit_out = env.dir.join("edit.mp4");
    let outcome = run(&env, &source, &edit_out, edited);
    assert!(outcome.success, "{}", outcome.messages);
    let streams_out = streams(&env, &edit_out);
    let audio = of_type(&streams_out, "audio");
    assert_eq!(audio.len(), 1);
    assert_eq!(audio[0]["codec_name"], "aac");
    assert_eq!(audio[0]["tags"]["handler_name"], "Japanese");
    assert!(
        outcome
            .messages
            .contains("DTS audio cannot be stored in MP4"),
        "{}",
        outcome.messages
    );
}

#[test]
fn mp4_remux_fails_before_starting_when_the_only_audio_is_dts() {
    let Some(env) = env("dts-only") else {
        return;
    };
    let Some(source) = dts_source(&env, true) else {
        return;
    };
    let expected = "DTS audio cannot be stored in MP4; choose MKV or use Transcode";
    let plain = env.dir.join("plain.mp4");
    let outcome = run(&env, &source, &plain, mp4_settings());
    assert!(!outcome.success);
    assert!(outcome.messages.contains(expected), "{}", outcome.messages);
    assert!(!plain.exists());

    let mut edited = mp4_settings();
    edited.track_edits = TrackEdits {
        tracks: vec![TrackEdit {
            id: metadata(&env, &source).audio_tracks[0].id,
            title: Some("Main".to_owned()),
            ..TrackEdit::default()
        }],
        ..TrackEdits::default()
    };
    let edit_out = env.dir.join("edit.mp4");
    let outcome = run(&env, &source, &edit_out, edited);
    assert!(!outcome.success);
    assert!(outcome.messages.contains(expected), "{}", outcome.messages);
    assert!(!edit_out.exists());
}

#[test]
fn latin1_srt_stream_copied_into_mkv_is_stored_as_utf8() {
    let Some(env) = env("charset") else {
        return;
    };
    let source = env.dir.join("source.mkv");
    ffmpeg(
        &env,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=10:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
        ],
        &source,
    );
    let srt = env.dir.join("latin1.srt");
    let mut bytes = b"1\n00:00:00,100 --> 00:00:01,500\nCaf".to_vec();
    bytes.extend([0xE9, b'\n']);
    fs::write(&srt, bytes).expect("latin-1 srt");
    let mut settings = JobSettings::default();
    settings.subtitles.external = vec![ExternalTrack {
        path: srt,
        kind: ExternalKind::Subtitle,
        language: Some("fra".to_owned()),
        charset: Some("ISO-8859-1".to_owned()),
        ..ExternalTrack::default()
    }];
    let output = env.dir.join("out.mkv");
    let outcome = run(&env, &source, &output, settings);
    assert!(outcome.success, "{}", outcome.messages);
    let extracted = tools::command(&env.ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&output)
        .args(["-map", "0:s:0", "-f", "srt", "-"])
        .output()
        .expect("ffmpeg extracts");
    let text = String::from_utf8(extracted.stdout).expect("extracted subtitle is valid UTF-8");
    assert!(text.contains("Café"), "{text:?}");
}

/// Path from `from` to `to` using `..`; `None` when they share no root (another drive).
fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for component in &to[common..] {
        path.push(component);
    }
    Some(path)
}

#[test]
fn relative_external_subtitle_works_when_ffmpeg_runs_in_the_temp_directory() {
    let Some(env) = env("relative") else {
        return;
    };
    if !env.encoders.supports(&VideoEncoder::X264) {
        println!("SKIP: libx264 is unavailable");
        return;
    }
    let embedded = env.dir.join("embedded.srt");
    fs::write(&embedded, "1\n00:00:00,100 --> 00:00:01,500\nBurn me\n").unwrap();
    let source = env.dir.join("source.mkv");
    ffmpeg(
        &env,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=10:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-i",
            embedded.to_str().unwrap(),
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:s",
            "-metadata:s:s:0",
            "language=eng",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            "-c:s",
            "srt",
        ],
        &source,
    );
    let external = env.dir.join("extra.srt");
    fs::write(&external, "1\n00:00:00,100 --> 00:00:01,500\nSoft\n").unwrap();
    let cwd = std::env::current_dir().unwrap();
    let Some(relative) = relative_path(&cwd, &external) else {
        println!("SKIP: scratch directory is on another drive than the working directory");
        return;
    };
    assert!(relative.is_relative());
    let mut settings = JobSettings {
        video: VideoSettings::Encode(VideoEncode {
            encoder: VideoEncoder::X264,
            speed: 8,
            deinterlace: Deinterlace::Off,
            ten_bit: TenBit::Off,
            ..VideoEncode::default()
        }),
        ..JobSettings::default()
    };
    settings.subtitles.burn_in = BurnIn::FirstSelected;
    settings.subtitles.external = vec![ExternalTrack {
        path: relative,
        language: Some("eng".to_owned()),
        ..ExternalTrack::default()
    }];
    let output = env.dir.join("out.mkv");
    let outcome = run(&env, &source, &output, settings);
    assert!(outcome.success, "{}", outcome.messages);
    let streams_out = streams(&env, &output);
    assert_eq!(of_type(&streams_out, "subtitle").len(), 1);
}

#[test]
fn single_axis_height_limit_encodes_with_libx264() {
    let Some(env) = env("axis") else {
        return;
    };
    if !env.encoders.supports(&VideoEncoder::X264) {
        println!("SKIP: libx264 is unavailable");
        return;
    }
    let source = env.dir.join("source.mkv");
    ffmpeg(
        &env,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=800x600:rate=10:duration=1",
            "-c:v",
            "mpeg4",
            "-an",
        ],
        &source,
    );
    for (max_width, max_height) in [(None, Some(333)), (Some(333), None)] {
        let settings = JobSettings {
            video: VideoSettings::Encode(VideoEncode {
                encoder: VideoEncoder::X264,
                speed: 8,
                max_width,
                max_height,
                deinterlace: Deinterlace::Off,
                ten_bit: TenBit::Off,
                ..VideoEncode::default()
            }),
            ..JobSettings::default()
        };
        let output = env
            .dir
            .join(format!("out-{max_width:?}-{max_height:?}.mkv"));
        let outcome = run(&env, &source, &output, settings);
        assert!(outcome.success, "{}", outcome.messages);
        let streams_out = streams(&env, &output);
        let video = of_type(&streams_out, "video")[0];
        let (width, height) = (
            video["width"].as_u64().unwrap(),
            video["height"].as_u64().unwrap(),
        );
        assert!(width % 2 == 0 && height % 2 == 0, "{width}x{height}");
        assert!(
            width <= 333 && max_width.is_some() || height <= 333 && max_height.is_some(),
            "{width}x{height}"
        );
    }
}
