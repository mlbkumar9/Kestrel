//! Real-FFmpeg checks for probed added files, applied settings and parallel queue jobs.
//! Every test skips (and says so) when ffmpeg or ffprobe is missing.

use kestrel_core::external::{ExternalKind, ExternalTrack};
use kestrel_core::formats::Container;
use kestrel_core::inputs::ExistingPolicy;
use kestrel_core::mkv::{self, MediaMetadata};
use kestrel_core::remux_edit::TrackEdits;
use kestrel_core::tools::{self, Toolchain};
use kestrel_core::transcode::{JobSettings, RateControl, VideoEncode, VideoEncoder, VideoSettings};
use kestrel_core::worker::{self, FileStatus, JobSpec, QueueSpec, RemuxEngine, WorkerEvent};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

struct Env {
    tools: Toolchain,
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
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
    let directory =
        std::env::temp_dir().join(format!("kestrel-gaps-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("scratch directory");
    Some(Env {
        ffmpeg: ffmpeg.path.clone(),
        ffprobe: ffprobe.path.clone(),
        tools,
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

/// 3 s MPEG-4 + AAC Matroska source.
fn source(env: &Env, name: &str, seconds: u32) -> (PathBuf, MediaMetadata) {
    let path = env.directory.join(name);
    ffmpeg(
        env,
        &[
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc2=size=320x240:rate=10:duration={seconds}"),
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

fn queue(env: &Env, jobs: Vec<JobSpec>, engine: RemuxEngine, parallel: usize) -> Vec<WorkerEvent> {
    worker::run_queue_blocking(QueueSpec {
        jobs,
        tools: env.tools.clone(),
        encoders: kestrel_core::encoders::detect(&env.ffmpeg).unwrap_or_default(),
        remux_engine: engine,
        existing: ExistingPolicy::Overwrite,
        dry_run: false,
        parallel_jobs: parallel,
        keep_source_dates: false,
    })
}

fn succeeded(events: &[WorkerEvent], id: u64) -> bool {
    events.iter().any(|event| {
        matches!(event, WorkerEvent::JobUpdated { job_id, status: FileStatus::Success, .. } if *job_id == id)
    })
}

fn log_text(events: &[WorkerEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            WorkerEvent::Log(text) => Some(text.clone()),
            WorkerEvent::JobUpdated {
                detail: Some(text), ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn streams(env: &Env, path: &Path) -> Vec<(String, String)> {
    let output = tools::command(&env.ffprobe)
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(path)
        .output()
        .expect("ffprobe runs");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("ffprobe json");
    json["streams"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|stream| {
            (
                stream["codec_type"].as_str().unwrap_or_default().to_owned(),
                stream["codec_name"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn remux_job(
    input: &Path,
    metadata: &MediaMetadata,
    output: PathBuf,
    container: Container,
    external: Vec<ExternalTrack>,
) -> JobSpec {
    JobSpec {
        execution: None,
        id: 1,
        input: input.to_path_buf(),
        output,
        metadata: metadata.clone(),
        settings: JobSettings {
            container,
            track_edits: TrackEdits {
                external,
                ..TrackEdits::default()
            },
            ..JobSettings::default()
        },
    }
}

fn added(path: &Path, kind: ExternalKind) -> ExternalTrack {
    ExternalTrack {
        path: path.to_path_buf(),
        kind,
        language: Some("fra".to_owned()),
        ..ExternalTrack::default()
    }
}

#[test]
fn probing_reads_the_real_codec_of_files_with_unknown_extensions() {
    let Some(env) = env("probe") else { return };
    let opus = env.directory.join("dub.mka");
    ffmpeg(
        &env,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=duration=1",
            "-c:a",
            "opus",
            "-strict",
            "-2",
            opus.to_str().unwrap(),
        ],
    );
    let flac = env.directory.join("lossless.mka");
    ffmpeg(
        &env,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=duration=1",
            "-c:a",
            "flac",
            flac.to_str().unwrap(),
        ],
    );
    let thd = env.directory.join("movie.thd");
    ffmpeg(
        &env,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=duration=1",
            "-c:a",
            "truehd",
            "-strict",
            "-2",
            "-f",
            "truehd",
            thd.to_str().unwrap(),
        ],
    );
    for (path, codec) in [(&opus, "opus"), (&flac, "flac"), (&thd, "truehd")] {
        let found = mkv::probe_external_codec(&env.ffprobe, path, ExternalKind::Audio).unwrap();
        assert_eq!(found, codec, "{}", path.display());
    }
    // An audio file is not a subtitle file.
    assert!(mkv::probe_external_codec(&env.ffprobe, &opus, ExternalKind::Subtitle).is_err());
    // Probing fills the codec, and leaves already-probed tracks alone.
    let mut tracks = vec![
        added(&thd, ExternalKind::Audio),
        ExternalTrack {
            codec: Some("keep".to_owned()),
            ..added(&opus, ExternalKind::Audio)
        },
    ];
    let warnings = kestrel_core::external::resolve_codecs(Some(&env.ffprobe), &mut tracks);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(tracks[0].codec.as_deref(), Some("truehd"));
    assert_eq!(tracks[1].codec.as_deref(), Some("keep"));
    // A file that cannot be probed stays on the extension fallback and says so.
    let broken = env.directory.join("broken.mka");
    fs::write(&broken, b"not media").unwrap();
    let mut tracks = vec![added(&broken, ExternalKind::Audio)];
    let warnings = kestrel_core::external::resolve_codecs(Some(&env.ffprobe), &mut tracks);
    assert_eq!(tracks[0].codec, None);
    assert!(
        warnings[0].contains("guessed from its extension"),
        "{warnings:?}"
    );
}

#[test]
fn added_mka_and_thd_files_keep_their_real_codec_in_mkv_and_mp4() {
    let Some(env) = env("added-audio") else {
        return;
    };
    let (input, metadata) = source(&env, "source.mkv", 3);
    let opus = env.directory.join("dub.mka");
    ffmpeg(
        &env,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:duration=3",
            "-c:a",
            "opus",
            "-strict",
            "-2",
            opus.to_str().unwrap(),
        ],
    );
    let thd = env.directory.join("movie.thd");
    ffmpeg(
        &env,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=550:duration=3",
            "-c:a",
            "truehd",
            "-strict",
            "-2",
            "-f",
            "truehd",
            thd.to_str().unwrap(),
        ],
    );

    // MP4 holds Opus: the .mka was dropped as "unknown" audio before probing.
    let mp4 = env.directory.join("opus.mp4");
    let events = queue(
        &env,
        vec![remux_job(
            &input,
            &metadata,
            mp4.clone(),
            Container::Mp4,
            vec![added(&opus, ExternalKind::Audio)],
        )],
        RemuxEngine::Ffmpeg,
        1,
    );
    assert!(succeeded(&events, 1), "{}", log_text(&events));
    let audio: Vec<String> = streams(&env, &mp4)
        .into_iter()
        .filter(|(kind, _)| kind == "audio")
        .map(|(_, codec)| codec)
        .collect();
    assert_eq!(audio, ["aac", "opus"], "{}", log_text(&events));

    // MP4 cannot hold TrueHD: the file is skipped with a warning, the rest is remuxed.
    let mp4 = env.directory.join("thd.mp4");
    let events = queue(
        &env,
        vec![remux_job(
            &input,
            &metadata,
            mp4.clone(),
            Container::Mp4,
            vec![added(&thd, ExternalKind::Audio)],
        )],
        RemuxEngine::Ffmpeg,
        1,
    );
    assert!(succeeded(&events, 1), "{}", log_text(&events));
    let text = log_text(&events);
    assert!(
        text.contains("TRUEHD audio cannot be stored in MP4"),
        "{text}"
    );
    assert_eq!(
        streams(&env, &mp4)
            .iter()
            .filter(|(kind, _)| kind == "audio")
            .count(),
        1
    );

    // MKV holds both, copied without re-encoding, with either engine.
    let mut engines = vec![RemuxEngine::Ffmpeg];
    if env.tools.mkvmerge.is_some() {
        engines.push(RemuxEngine::MkvToolNix);
    }
    for engine in engines {
        let metadata = if engine == RemuxEngine::MkvToolNix {
            mkv::read_metadata(
                env.tools.mkvmerge.as_ref().map(|tool| tool.path.as_path()),
                Some(&env.ffprobe),
                &input,
            )
            .unwrap()
        } else {
            metadata.clone()
        };
        let mkv_out = env.directory.join(format!("both-{}.mkv", engine.label()));
        let events = queue(
            &env,
            vec![remux_job(
                &input,
                &metadata,
                mkv_out.clone(),
                Container::Mkv,
                vec![
                    added(&opus, ExternalKind::Audio),
                    added(&thd, ExternalKind::Audio),
                ],
            )],
            engine,
            1,
        );
        assert!(
            succeeded(&events, 1),
            "{}: {}",
            engine.label(),
            log_text(&events)
        );
        let audio: Vec<String> = streams(&env, &mkv_out)
            .into_iter()
            .filter(|(kind, _)| kind == "audio")
            .map(|(_, codec)| codec)
            .collect();
        assert_eq!(audio, ["aac", "opus", "truehd"], "{}", engine.label());
    }
}

#[test]
fn added_subtitle_files_with_unknown_extensions_are_probed_into_mp4() {
    let Some(env) = env("added-subs") else { return };
    let (input, metadata) = source(&env, "source.mkv", 3);
    let text = env.directory.join("captions.txt");
    fs::write(&text, "1\n00:00:01,000 --> 00:00:02,000\nHello\n").unwrap();
    let mks = env.directory.join("subs.mks");
    ffmpeg(
        &env,
        &[
            "-i",
            text.to_str().unwrap(),
            "-c:s",
            "srt",
            "-f",
            "matroska",
            mks.to_str().unwrap(),
        ],
    );
    for file in [&text, &mks] {
        let out = env.directory.join(format!(
            "{}.mp4",
            file.file_stem().unwrap().to_string_lossy()
        ));
        let events = queue(
            &env,
            vec![remux_job(
                &input,
                &metadata,
                out.clone(),
                Container::Mp4,
                vec![added(file, ExternalKind::Subtitle)],
            )],
            RemuxEngine::Ffmpeg,
            1,
        );
        assert!(succeeded(&events, 1), "{}", log_text(&events));
        let subtitles: Vec<String> = streams(&env, &out)
            .into_iter()
            .filter(|(kind, _)| kind == "subtitle")
            .map(|(_, codec)| codec)
            .collect();
        assert_eq!(
            subtitles,
            ["mov_text"],
            "{}: {}",
            file.display(),
            log_text(&events)
        );
    }
}

fn encode_job(id: u64, input: &Path, metadata: &MediaMetadata, output: PathBuf) -> JobSpec {
    JobSpec {
        execution: None,
        id,
        input: input.to_path_buf(),
        output,
        metadata: metadata.clone(),
        settings: JobSettings {
            container: Container::Mkv,
            video: VideoSettings::Encode(VideoEncode {
                encoder: VideoEncoder::X264,
                rate: RateControl::Quality(40),
                speed: 4,
                ..VideoEncode::default()
            }),
            ..JobSettings::default()
        },
    }
}

fn index_of(events: &[WorkerEvent], id: u64, wanted: FileStatus) -> usize {
    events
        .iter()
        .position(|event| {
            matches!(event, WorkerEvent::JobUpdated { job_id, status, .. } if *job_id == id && *status == wanted)
        })
        .unwrap_or_else(|| panic!("job {id} never reached {wanted:?}"))
}

fn duration(env: &Env, path: &Path) -> f64 {
    let output = tools::command(&env.ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe runs");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("duration")
}

#[test]
fn two_parallel_jobs_overlap_and_both_outputs_are_valid() {
    let Some(env) = env("parallel") else { return };
    if kestrel_core::encoders::detect(&env.ffmpeg)
        .map(|set| !set.software.contains(&VideoEncoder::X264))
        .unwrap_or(true)
    {
        println!("SKIP: libx264 is not available");
        return;
    }
    let (first_in, first_meta) = source(&env, "one.mkv", 6);
    let (second_in, second_meta) = source(&env, "two.mkv", 6);
    let jobs = vec![
        encode_job(1, &first_in, &first_meta, env.directory.join("one-out.mkv")),
        encode_job(
            2,
            &second_in,
            &second_meta,
            env.directory.join("two-out.mkv"),
        ),
    ];
    let events = queue(&env, jobs, RemuxEngine::Ffmpeg, 2);
    let text = log_text(&events);
    assert!(text.contains("Running up to 2 jobs at a time"), "{text}");
    for id in [1, 2] {
        assert!(succeeded(&events, id), "job {id}: {text}");
    }
    // Both jobs started before either finished: they ran at the same time.
    let first_done =
        index_of(&events, 1, FileStatus::Success).min(index_of(&events, 2, FileStatus::Success));
    let both_started = index_of(&events, 1, FileStatus::Processing).max(index_of(
        &events,
        2,
        FileStatus::Processing,
    ));
    assert!(both_started < first_done, "jobs ran one after the other");
    // Each job reports its own progress.
    for id in [1, 2] {
        assert!(
            events.iter().any(
                |event| matches!(event, WorkerEvent::Progress { job_id, .. } if *job_id == id)
            )
        );
    }
    // Both outputs are complete, playable files; no partial files are left behind.
    for name in ["one-out.mkv", "two-out.mkv"] {
        let path = env.directory.join(name);
        let seconds = duration(&env, &path);
        assert!((seconds - 6.0).abs() < 0.6, "{name}: {seconds}");
        assert!(
            streams(&env, &path)
                .iter()
                .any(|(kind, codec)| kind == "video" && codec == "h264")
        );
    }
    let leftovers: Vec<_> = fs::read_dir(&env.directory)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("partial") || name.contains("kestrel"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn parallel_jobs_aiming_at_one_output_never_write_the_same_file() {
    let Some(env) = env("parallel-claim") else {
        return;
    };
    let (input, metadata) = source(&env, "source.mkv", 3);
    let output = env.directory.join("out.mkv");
    let jobs = (1..=2)
        .map(|id| {
            let mut job = remux_job(
                &input,
                &metadata,
                output.clone(),
                Container::Mkv,
                Vec::new(),
            );
            job.id = id;
            job
        })
        .collect();
    let events = worker::run_queue_blocking(QueueSpec {
        jobs,
        tools: env.tools.clone(),
        encoders: Default::default(),
        remux_engine: RemuxEngine::Ffmpeg,
        existing: ExistingPolicy::Rename,
        dry_run: false,
        parallel_jobs: 2,
        keep_source_dates: false,
    });
    assert!(
        succeeded(&events, 1) && succeeded(&events, 2),
        "{}",
        log_text(&events)
    );
    assert!(output.is_file());
    assert!(env.directory.join("out (2).mkv").is_file());
}

fn set_modified(path: &Path, when: SystemTime) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_times(fs::FileTimes::new().set_modified(when).set_accessed(when))
        .unwrap();
}

#[test]
fn keep_source_dates_copies_the_source_times_to_the_output() {
    let Some(env) = env("dates") else { return };
    let (input, metadata) = source(&env, "source.mkv", 2);
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_500_000_000);
    set_modified(&input, old);
    for (keep, name) in [(true, "kept.mkv"), (false, "fresh.mkv")] {
        let output = env.directory.join(name);
        let events = worker::run_queue_blocking(QueueSpec {
            jobs: vec![remux_job(
                &input,
                &metadata,
                output.clone(),
                Container::Mkv,
                Vec::new(),
            )],
            tools: env.tools.clone(),
            encoders: Default::default(),
            remux_engine: RemuxEngine::Ffmpeg,
            existing: ExistingPolicy::Overwrite,
            dry_run: false,
            parallel_jobs: 1,
            keep_source_dates: keep,
        });
        assert!(succeeded(&events, 1), "{}", log_text(&events));
        let modified = fs::metadata(&output).unwrap().modified().unwrap();
        let apart = modified
            .duration_since(old)
            .unwrap_or_else(|error| error.duration());
        if keep {
            assert!(apart < Duration::from_secs(2), "{name}: {apart:?}");
        } else {
            assert!(apart > Duration::from_secs(86_400), "{name}: {apart:?}");
        }
    }
}
