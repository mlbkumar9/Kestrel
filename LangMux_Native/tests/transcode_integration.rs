use kestrel_media::encoders::{self, EncoderSet};
use kestrel_media::formats::Container;
use kestrel_media::inputs::ExistingPolicy;
use kestrel_media::mkv::{self, MediaMetadata};
use kestrel_media::presets;
use kestrel_media::tools::{self, Toolchain};
use kestrel_media::transcode::{BurnIn, Crop, VideoEncoder};
use kestrel_media::worker::{self, FileStatus, JobSpec, QueueSpec, RemuxEngine, WorkerEvent};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

#[test]
fn built_in_software_presets_transcode_supported_fixture_containers() {
    let tools = Toolchain::discover();
    let (Some(ffmpeg), Some(ffprobe)) = (&tools.ffmpeg, &tools.ffprobe) else {
        println!("SKIP: integration fixtures require ffmpeg and ffprobe on PATH");
        return;
    };
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/v3-integration");
    fs::create_dir_all(&fixture_dir).expect("fixture directory should be created");
    let subtitles = fixture_dir.join("english.srt");
    fs::write(&subtitles, "1\n00:00:00,250 --> 00:00:02,500\nHello\n")
        .expect("SRT fixture should be written");

    let fixtures = [
        (Container::Mp4, "libx264", "aac", Some("mov_text")),
        (Container::Mkv, "libx264", "aac", Some("srt")),
        (Container::Mkv, "mpeg4", "mp3", None),
        (Container::WebM, "libvpx-vp9", "libopus", Some("webvtt")),
    ];
    let mut inputs = Vec::new();
    for (container, video, audio, subtitle) in fixtures {
        let extension = match (container, video) {
            (Container::Mkv, "mpeg4") => "avi",
            (container, _) => container.extension(),
        };
        let path = fixture_dir.join(format!("fixture-{video}.{extension}"));
        let mut command = tools::command(&ffmpeg.path);
        command.args([
            "-hide_banner",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=24:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=660:duration=3",
        ]);
        if let Some(subtitle_codec) = subtitle {
            command.arg("-i").arg(&subtitles);
            command.args([
                "-map",
                "3:s:0",
                "-metadata:s:s:0",
                "language=eng",
                "-c:s",
                subtitle_codec,
            ]);
        }
        let output = command
            .args([
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-map",
                "2:a:0",
                "-metadata:s:a:0",
                "language=eng",
                "-metadata:s:a:1",
                "language=jpn",
                "-c:v",
                video,
                "-c:a",
                audio,
                "-t",
                "3",
            ])
            .arg(&path)
            .output()
            .expect("ffmpeg should generate fixture");
        assert!(
            output.status.success(),
            "fixture generation failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        inputs.push(path);
    }

    let encoders = EncoderSet {
        software: vec![
            VideoEncoder::X264,
            VideoEncoder::X265,
            VideoEncoder::SvtAv1,
            VideoEncoder::Vp9,
        ],
        hardware: Vec::new(),
    };
    let builtin_presets = presets::builtins(&EncoderSet::default())
        .into_iter()
        .filter(|preset| preset.id != "hardware")
        .collect::<Vec<_>>();
    let mut job_id = 0;
    let mut generated_outputs = Vec::new();
    for input in &inputs {
        let metadata: MediaMetadata = mkv::read_metadata(
            tools.mkvmerge.as_ref().map(|tool| tool.path.as_path()),
            Some(&ffprobe.path),
            input,
        )
        .expect("fixture metadata should be readable");
        for preset in &builtin_presets {
            let output_dir = fixture_dir.join("outputs").join(&preset.id);
            fs::create_dir_all(&output_dir).expect("output directory should be created");
            let output = output_dir.join(format!(
                "{}.{}",
                input.file_stem().unwrap().to_string_lossy(),
                preset.settings.container.extension()
            ));
            let events = worker::run_queue_blocking(QueueSpec {
                jobs: vec![JobSpec {
                    id: job_id,
                    input: input.clone(),
                    output: output.clone(),
                    metadata: metadata.clone(),
                    settings: preset.settings.clone(),
                }],
                tools: tools.clone(),
                encoders: encoders.clone(),
                remux_engine: if tools.mkvmerge.is_some() {
                    RemuxEngine::MkvToolNix
                } else {
                    RemuxEngine::Ffmpeg
                },
                existing: ExistingPolicy::Overwrite,
                dry_run: false,
            });
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    WorkerEvent::JobUpdated { job_id: id, status: FileStatus::Success, .. }
                        if *id == job_id
                )),
                "preset {} failed for {}",
                preset.id,
                input.display()
            );
            let output_metadata = mkv::read_metadata(None, Some(&ffprobe.path), &output)
                .expect("output metadata should be readable");
            assert_eq!(
                output.extension().and_then(|extension| extension.to_str()),
                Some(preset.settings.container.extension())
            );
            let expected_format = match preset.settings.container {
                Container::Mkv => "matroska",
                Container::Mp4 | Container::Mov => "mov",
                Container::WebM => "webm",
            };
            assert!(
                output_metadata
                    .container
                    .as_deref()
                    .is_some_and(|format| format.contains(expected_format)),
                "unexpected output container for preset {}: {:?}",
                preset.id,
                output_metadata.container
            );
            assert!(
                output_metadata.video.is_some(),
                "output video stream missing"
            );
            let expected_video_codec = match preset.id.as_str() {
                "smaller" => "hevc",
                "anywhere" => "h264",
                "archive" => "av1",
                "web" => "vp9",
                _ => metadata.video.as_ref().unwrap().codec_id.as_str(),
            };
            assert_eq!(
                output_metadata.video.as_ref().unwrap().codec_id,
                expected_video_codec,
                "unexpected video codec for preset {} and {}",
                preset.id,
                input.display()
            );
            assert_eq!(output_metadata.audio_tracks.len(), 2);
            for (index, track) in output_metadata.audio_tracks.iter().enumerate() {
                let expected_codec = match preset.id.as_str() {
                    "anywhere" => "aac",
                    "web" => "opus",
                    _ => metadata.audio_tracks[index].codec_id.as_str(),
                };
                assert_eq!(
                    track.codec_id,
                    expected_codec,
                    "unexpected audio codec for preset {} and {}",
                    preset.id,
                    input.display()
                );
            }
            let audio_languages = output_metadata
                .audio_tracks
                .iter()
                .map(|track| track.language.as_str())
                .collect::<BTreeSet<_>>();
            if input.extension().and_then(|ext| ext.to_str()) == Some("avi") {
                assert!(
                    output_metadata
                        .audio_tracks
                        .iter()
                        .all(|track| track.language == "und")
                );
            } else {
                assert!(audio_languages.contains("eng"));
                assert!(audio_languages.contains("jpn"));
            }
            let source_has_subtitles =
                input.extension().and_then(|ext| ext.to_str()) != Some("avi");
            let expected_subtitle_count = usize::from(source_has_subtitles);
            assert_eq!(
                output_metadata.subtitle_tracks.len(),
                expected_subtitle_count
            );
            if source_has_subtitles && preset.settings.container == Container::Mp4 {
                assert_eq!(output_metadata.subtitle_tracks[0].codec_id, "mov_text");
            } else if source_has_subtitles && preset.settings.container == Container::WebM {
                assert_eq!(output_metadata.subtitle_tracks[0].codec_id, "webvtt");
            }
            generated_outputs.push(output);
            job_id += 1;
        }
    }
    for output in generated_outputs {
        let parent = output.parent().unwrap();
        assert!(
            fs::read_dir(parent).unwrap().flatten().all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .contains("langmux-partial")),
            "partial output leaked in {}",
            parent.display()
        );
    }
    assert!(fixture_dir.exists());
    println!(
        "Generated real-FFmpeg fixtures and outputs: {}",
        fixture_dir.display()
    );
}

#[test]
fn real_ffmpeg_burn_in_handles_special_paths_and_image_subtitles() {
    let tools = Toolchain::discover();
    let (Some(ffmpeg), Some(ffprobe)) = (&tools.ffmpeg, &tools.ffprobe) else {
        println!("SKIP: integration fixtures require ffmpeg and ffprobe on PATH");
        return;
    };
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/v3-integration");
    let special_dir = fixture_dir.join("Bob's, [test]");
    fs::create_dir_all(&special_dir).expect("special-path fixture directory should be created");
    let subtitles = fixture_dir.join("burn-in.srt");
    fs::write(
        &subtitles,
        "1\n00:00:00,250 --> 00:00:02,500\nBurned text\n",
    )
    .expect("subtitle fixture should be written");

    let text_input = special_dir.join("text-burn-in.mkv");
    let generated = tools::command(&ffmpeg.path)
        .args([
            "-hide_banner",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=24:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-i",
        ])
        .arg(&subtitles)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-map",
            "2:s:0",
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
            "-c:s",
            "srt",
            "-metadata:s:s:0",
            "language=eng",
            "-t",
            "3",
        ])
        .arg(&text_input)
        .output()
        .expect("ffmpeg should generate special-path fixture");
    assert!(
        generated.status.success(),
        "text fixture generation failed: {}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let text_metadata = mkv::read_metadata(
        tools.mkvmerge.as_ref().map(|tool| tool.path.as_path()),
        Some(&ffprobe.path),
        &text_input,
    )
    .expect("text fixture metadata should be readable");
    let encoder_set =
        encoders::detect(&ffmpeg.path).expect("FFmpeg encoder list should be readable");
    let mut anywhere = presets::builtin_preset("anywhere").unwrap();
    anywhere.settings.subtitles.burn_in = BurnIn::FirstSelected;
    let text_output = fixture_dir.join("outputs/burn-in/text-output.mp4");
    fs::create_dir_all(text_output.parent().unwrap()).unwrap();
    let text_events = worker::run_queue_blocking(QueueSpec {
        jobs: vec![JobSpec {
            id: 901,
            input: text_input,
            output: text_output.clone(),
            metadata: text_metadata,
            settings: anywhere.settings,
        }],
        tools: tools.clone(),
        encoders: encoder_set.clone(),
        remux_engine: RemuxEngine::Ffmpeg,
        existing: ExistingPolicy::Overwrite,
        dry_run: false,
    });
    assert!(
        text_events.iter().any(|event| matches!(
            event,
            WorkerEvent::JobUpdated {
                job_id: 901,
                status: FileStatus::Success,
                ..
            }
        )),
        "special-path text burn-in failed: {}",
        event_messages(&text_events)
    );
    let text_output_metadata = mkv::read_metadata(None, Some(&ffprobe.path), &text_output).unwrap();
    assert!(text_output_metadata.video.is_some());
    assert!(text_output_metadata.subtitle_tracks.is_empty());
}

#[test]
fn real_ffmpeg_detects_letterbox_crop_from_multiple_frames() {
    let Some(ffmpeg) = Toolchain::discover().ffmpeg else {
        println!("SKIP: crop detection integration test requires ffmpeg on PATH");
        return;
    };
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/v3-integration");
    fs::create_dir_all(&fixture_dir).expect("fixture directory should be created");
    let input = fixture_dir.join("letterboxed-crop.mkv");
    let output = tools::command(&ffmpeg.path)
        .args([
            "-hide_banner",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=24:duration=3",
            "-vf",
            "pad=640:480:0:60:black",
            "-an",
            "-c:v",
            "libx264",
        ])
        .arg(&input)
        .output()
        .expect("ffmpeg should generate the letterboxed fixture");
    assert!(
        output.status.success(),
        "letterbox fixture generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let crop = kestrel_media::analysis::detect_crop(&ffmpeg.path, &input, 3.0)
        .expect("crop detection should find the letterbox");
    let Crop::Custom {
        top,
        bottom,
        left,
        right,
    } = crop
    else {
        panic!("expected a custom crop for the letterboxed fixture");
    };
    assert!(top.abs_diff(60) <= 4, "top crop was {top}");
    assert!(bottom.abs_diff(60) <= 4, "bottom crop was {bottom}");
    assert!(left <= 4, "left crop was {left}");
    assert!(right <= 4, "right crop was {right}");
    assert!(
        480_u32.abs_diff(top + bottom + 360) <= 4,
        "detected crop height was {}",
        480_u32.saturating_sub(top + bottom)
    );
}

fn event_messages(events: &[WorkerEvent]) -> String {
    events
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
        .join("\n")
}
