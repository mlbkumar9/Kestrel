use kestrel_core::external::{ExternalKind, ExternalTrack};
use kestrel_core::formats::Container;
use kestrel_core::inputs::ExistingPolicy;
use kestrel_core::mkv::{self, MediaMetadata, TrackInfo};
use kestrel_core::remux_edit::{TrackEdit, TrackEdits};
use kestrel_core::tools::{self, Toolchain};
use kestrel_core::transcode::JobSettings;
use kestrel_core::worker::{self, FileStatus, JobSpec, QueueSpec, RemuxEngine, WorkerEvent};
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    tools: Toolchain,
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    directory: PathBuf,
    source: PathBuf,
    metadata: MediaMetadata,
}

/// Generates a 3 s MKV: video, audio jpn, audio eng, SRT eng. `None` when ffmpeg is missing.
fn fixture(name: &str) -> Option<Fixture> {
    let tools = Toolchain::discover();
    let (Some(ffmpeg), Some(ffprobe)) = (&tools.ffmpeg, &tools.ffprobe) else {
        println!("SKIP: remux edit integration needs ffmpeg and ffprobe on PATH");
        return None;
    };
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/remux-edit-integration")
        .join(name);
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("fixture directory");
    let subtitles = directory.join("source.srt");
    fs::write(&subtitles, "1\n00:00:01,000 --> 00:00:02,000\nHello\n").expect("source srt");
    let source = directory.join("source.mkv");
    let output = tools::command(&ffmpeg.path)
        .args(["-hide_banner", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=320x240:rate=10:duration=3")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
        .args(["-f", "lavfi", "-i", "sine=frequency=660:duration=3"])
        .arg("-i")
        .arg(&subtitles)
        .args(["-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:s"])
        .args(["-metadata:s:a:0", "language=jpn"])
        .args(["-metadata:s:a:1", "language=eng"])
        .args(["-metadata:s:s:0", "language=eng"])
        .args(["-c:v", "mpeg4", "-c:a", "aac", "-c:s", "srt", "-t", "3"])
        .arg(&source)
        .output()
        .expect("ffmpeg runs");
    assert!(
        output.status.success(),
        "fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata = mkv::read_metadata(None, Some(&ffprobe.path), &source).expect("metadata");
    Some(Fixture {
        ffmpeg: ffmpeg.path.clone(),
        ffprobe: ffprobe.path.clone(),
        tools,
        directory,
        source,
        metadata,
    })
}

fn by_language<'a>(tracks: &'a [TrackInfo], language: &str) -> &'a TrackInfo {
    tracks
        .iter()
        .find(|track| track.language == language)
        .unwrap_or_else(|| panic!("no {language} track"))
}

fn remux(fixture: &Fixture, settings: JobSettings, output: &Path) -> String {
    remux_with(fixture, settings, output, RemuxEngine::Ffmpeg)
}

fn remux_with(
    fixture: &Fixture,
    settings: JobSettings,
    output: &Path,
    engine: RemuxEngine,
) -> String {
    let events = worker::run_queue_blocking(QueueSpec {
        jobs: vec![JobSpec {
            execution: None,
            id: 1,
            input: fixture.source.clone(),
            output: output.to_path_buf(),
            metadata: fixture.metadata.clone(),
            settings,
        }],
        tools: fixture.tools.clone(),
        encoders: Default::default(),
        remux_engine: engine,
        existing: ExistingPolicy::Overwrite,
        dry_run: false,
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
    assert!(
        events.iter().any(|event| matches!(
            event,
            WorkerEvent::JobUpdated {
                status: FileStatus::Success,
                ..
            }
        )),
        "job did not succeed:\n{messages}"
    );
    messages
}

fn probe_streams(fixture: &Fixture, path: &Path) -> Vec<serde_json::Value> {
    let output = tools::command(&fixture.ffprobe)
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

fn tag<'a>(stream: &'a serde_json::Value, name: &str) -> &'a str {
    stream["tags"][name].as_str().unwrap_or_default()
}

fn disposition(stream: &serde_json::Value, name: &str) -> u64 {
    stream["disposition"][name].as_u64().unwrap_or_default()
}

fn first_packet_seconds(fixture: &Fixture, path: &Path, selector: &str) -> f64 {
    let output = tools::command(&fixture.ffprobe)
        .args(["-v", "error", "-select_streams", selector])
        .args(["-show_entries", "packet=pts_time", "-of", "json"])
        .arg(path)
        .output()
        .expect("ffprobe runs");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("packet json");
    json["packets"][0]["pts_time"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .expect("first packet time")
}

fn extract_srt(fixture: &Fixture, path: &Path, selector: &str) -> String {
    let output = tools::command(&fixture.ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", selector, "-f", "srt", "-"])
        .output()
        .expect("ffmpeg runs");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn ffmpeg_remux_edits_tracks_and_adds_external_subtitle_and_audio() {
    let Some(fixture) = fixture("mkv") else {
        return;
    };
    edit_scenario(&fixture, RemuxEngine::Ffmpeg);
}

#[test]
fn mkvmerge_remux_edits_tracks_and_adds_external_subtitle_and_audio() {
    let Some(mut fixture) = fixture("mkvmerge") else {
        return;
    };
    let Some(mkvmerge) = fixture.tools.mkvmerge.clone() else {
        println!("SKIP: mkvmerge end-to-end test needs MKVToolNix");
        return;
    };
    // mkvmerge track ids come from `mkvmerge -J`, so read the metadata through it.
    fixture.metadata = mkv::read_metadata(
        Some(&mkvmerge.path),
        Some(&fixture.ffprobe),
        &fixture.source,
    )
    .expect("mkvmerge metadata");
    let messages = edit_scenario(&fixture, RemuxEngine::MkvToolNix);
    assert!(
        messages.to_ascii_lowercase().contains("mkvmerge"),
        "job did not run through mkvmerge:\n{messages}"
    );
}

fn edit_scenario(fixture: &Fixture, engine: RemuxEngine) -> String {
    let ac3 = fixture.directory.join("commentary.ac3");
    let generated = tools::command(&fixture.ffmpeg)
        .args(["-hide_banner", "-y", "-f", "lavfi", "-i"])
        .arg("sine=frequency=880:duration=3")
        .args(["-c:a", "ac3"])
        .arg(&ac3)
        .output()
        .expect("ffmpeg runs");
    assert!(generated.status.success());
    // Latin-1 bytes: "Café" with 0xE9, starting at 1.0 s.
    let spanish = fixture.directory.join("spanish.srt");
    fs::write(
        &spanish,
        b"1\n00:00:01,000 --> 00:00:02,000\nCaf\xe9\n".as_slice(),
    )
    .expect("latin-1 srt");

    let eng_audio = by_language(&fixture.metadata.audio_tracks, "eng").id;
    let jpn_audio = by_language(&fixture.metadata.audio_tracks, "jpn").id;
    let eng_subtitle = by_language(&fixture.metadata.subtitle_tracks, "eng").id;
    let settings = JobSettings {
        track_edits: TrackEdits {
            tracks: vec![
                TrackEdit {
                    id: eng_audio,
                    keep: false,
                    ..TrackEdit::default()
                },
                TrackEdit {
                    id: jpn_audio,
                    title: Some("Original".to_owned()),
                    ..TrackEdit::default()
                },
                TrackEdit {
                    id: eng_subtitle,
                    title: Some("English SDH".to_owned()),
                    forced: Some(true),
                    ..TrackEdit::default()
                },
            ],
            order: Vec::new(),
            external: vec![
                ExternalTrack {
                    path: spanish,
                    kind: ExternalKind::Subtitle,
                    language: Some("spa".to_owned()),
                    title: Some("Spanish".to_owned()),
                    offset_ms: 500,
                    charset: Some("CP1252".to_owned()),
                    ..ExternalTrack::default()
                },
                ExternalTrack {
                    path: ac3,
                    kind: ExternalKind::Audio,
                    language: Some("eng".to_owned()),
                    title: Some("Commentary".to_owned()),
                    default: true,
                    ..ExternalTrack::default()
                },
            ],
        },
        ..JobSettings::default()
    };
    assert!(settings.is_remux_only());
    let output = fixture.directory.join("edited.mkv");
    let messages = remux_with(fixture, settings, &output, engine);

    let streams = probe_streams(fixture, &output);
    assert_eq!(of_type(&streams, "video").len(), 1);
    let audio = of_type(&streams, "audio");
    assert_eq!(audio.len(), 2, "eng source audio must be dropped");
    assert_eq!(tag(audio[0], "language"), "jpn");
    assert_eq!(tag(audio[0], "title"), "Original");
    assert_eq!(audio[0]["codec_name"], "aac");
    assert_eq!(tag(audio[1], "language"), "eng");
    assert_eq!(tag(audio[1], "title"), "Commentary");
    assert_eq!(audio[1]["codec_name"], "ac3");
    assert_eq!(disposition(audio[0], "default"), 0);
    assert_eq!(disposition(audio[1], "default"), 1);

    let subtitles = of_type(&streams, "subtitle");
    assert_eq!(subtitles.len(), 2);
    assert_eq!(tag(subtitles[0], "language"), "eng");
    assert_eq!(tag(subtitles[0], "title"), "English SDH");
    assert_eq!(disposition(subtitles[0], "forced"), 1);
    assert_eq!(tag(subtitles[1], "language"), "spa");
    assert_eq!(tag(subtitles[1], "title"), "Spanish");
    assert_eq!(disposition(subtitles[1], "forced"), 0);

    let source_start = first_packet_seconds(fixture, &output, "s:0");
    let shifted_start = first_packet_seconds(fixture, &output, "s:1");
    assert!(
        (shifted_start - source_start - 0.5).abs() < 0.05,
        "subtitle moved from {source_start} to {shifted_start}"
    );
    assert!(
        extract_srt(fixture, &output, "0:s:1").contains("Caf\u{e9}"),
        "CP1252 charset was not applied"
    );
    messages
}

#[test]
fn ffmpeg_mp4_remux_converts_text_subtitles_and_skips_picture_subtitles_with_a_warning() {
    let Some(fixture) = fixture("mp4") else {
        return;
    };
    let spanish = fixture.directory.join("spanish.srt");
    fs::write(&spanish, "1\n00:00:01,000 --> 00:00:02,000\nHola\n").expect("srt");
    let picture = fixture.directory.join("forced.sup");
    fs::write(&picture, b"PG").expect("sup placeholder");
    let settings = JobSettings {
        container: Container::Mp4,
        track_edits: TrackEdits {
            external: vec![
                ExternalTrack {
                    path: spanish,
                    language: Some("spa".to_owned()),
                    ..ExternalTrack::default()
                },
                ExternalTrack {
                    path: picture,
                    language: Some("eng".to_owned()),
                    ..ExternalTrack::default()
                },
            ],
            ..TrackEdits::default()
        },
        ..JobSettings::default()
    };
    let output = fixture.directory.join("edited.mp4");
    let messages = remux(&fixture, settings, &output);

    assert!(
        messages.contains("warning: Skipped external subtitle forced.sup"),
        "missing skip warning in:\n{messages}"
    );
    let streams = probe_streams(&fixture, &output);
    let subtitles = of_type(&streams, "subtitle");
    assert_eq!(subtitles.len(), 2, "source SRT and external SRT only");
    for subtitle in &subtitles {
        assert_eq!(subtitle["codec_name"], "mov_text");
    }
    assert_eq!(tag(subtitles[1], "language"), "spa");
    assert_eq!(of_type(&streams, "audio").len(), 2);
}
