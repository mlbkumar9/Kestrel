use kestrel_core::encoders::{self, EncoderSet};
use kestrel_core::formats::Container;
use kestrel_core::inputs::ExistingPolicy;
use kestrel_core::mkv::{self, MediaMetadata};
use kestrel_core::presets;
use kestrel_core::tools::{self, Toolchain};
use kestrel_core::transcode::{BurnIn, Crop};
use kestrel_core::worker::{self, FileStatus, JobSpec, QueueSpec, RemuxEngine, WorkerEvent};
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
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/v3-integration");
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

    let encoders = encoders::detect(&ffmpeg.path).expect("FFmpeg encoders should be detected");
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
                    execution: None,
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
                parallel_jobs: 1,
                keep_source_dates: false,
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
                .contains("kestrel-partial")),
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
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/v3-integration");
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
            execution: None,
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
        parallel_jobs: 1,
        keep_source_dates: false,
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
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/v3-integration");
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

    let crop = kestrel_core::analysis::detect_crop(&ffmpeg.path, &input, 3.0)
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

// ---- HandBrake-parity encodes: real FFmpeg, verified with ffprobe ----

mod parity {
    use kestrel_core::encoders::{self, EncoderSet};
    use kestrel_core::external::ExternalTrack;
    use kestrel_core::formats::Container;
    use kestrel_core::inputs::ExistingPolicy;
    use kestrel_core::mkv;
    use kestrel_core::tools::{self, Toolchain};
    use kestrel_core::transcode::{
        AudioChoice, AudioEncoder, AudioMixdown, AudioSettings, AudioTrackSetting, BorderFill,
        ColorSpaceTarget, Crop, Deinterlace, DeinterlaceFilter, DeinterlacePreset, Denoise,
        DenoiseFilter, DenoiseTune, Detelecine, FilterStrength, FrameRate, FrameRateMode,
        JobSettings, MediaTags, RateControl, Rotation, SharpenFilter, TenBit, VideoEncode,
        VideoEncoder, VideoSettings,
    };
    use kestrel_core::worker::{self, FileStatus, JobSpec, QueueSpec, RemuxEngine, WorkerEvent};
    use std::fs;
    use std::path::{Path, PathBuf};

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
            println!("SKIP: integration fixtures require ffmpeg and ffprobe on PATH");
            return None;
        };
        let (ffmpeg, ffprobe) = (ffmpeg.path.clone(), ffprobe.path.clone());
        let encoders = encoders::detect(&ffmpeg).expect("encoders should be detected");
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/parity-integration")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("test directory should be created");
        Some(Env {
            tools,
            ffmpeg,
            ffprobe,
            encoders,
            dir,
        })
    }

    fn require(env: &Env, encoder: &VideoEncoder) -> bool {
        let present = env.encoders.supports(encoder);
        if !present {
            println!("SKIP: encoder {encoder:?} is not available");
        }
        present
    }

    /// 320x240 source: a 320x180 picture with 30-pixel black bars, 3 seconds, mono sine audio.
    fn source(env: &Env) -> PathBuf {
        let path = env.dir.join("source.mkv");
        let output = tools::command(&env.ffmpeg)
            .args(["-hide_banner", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=320x180:rate=24:duration=3")
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
            .args(["-vf", "pad=320:240:0:30:black", "-c:v", "libx264"])
            .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-t", "3"])
            .arg(&path)
            .output()
            .expect("ffmpeg should generate the source");
        assert!(
            output.status.success(),
            "source generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        path
    }

    fn write_srt(path: &Path, text: &str) {
        fs::write(path, format!("1\n00:00:00,250 --> 00:00:02,500\n{text}\n"))
            .expect("SRT should be written");
    }

    fn run(env: &Env, input: &Path, output: &Path, settings: JobSettings) -> Result<(), String> {
        let metadata = mkv::read_metadata(None, Some(&env.ffprobe), input)
            .expect("source metadata should be readable");
        let events = worker::run_queue_blocking(QueueSpec {
            jobs: vec![JobSpec {
                execution: None,
                id: 1,
                input: input.to_path_buf(),
                output: output.to_path_buf(),
                metadata,
                settings,
            }],
            tools: env.tools.clone(),
            encoders: env.encoders.clone(),
            remux_engine: RemuxEngine::Ffmpeg,
            existing: ExistingPolicy::Overwrite,
            dry_run: false,
            parallel_jobs: 1,
            keep_source_dates: false,
        });
        if events.iter().any(|event| {
            matches!(
                event,
                WorkerEvent::JobUpdated {
                    status: FileStatus::Success,
                    ..
                }
            )
        }) {
            return Ok(());
        }
        Err(events
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
            .join("\n"))
    }

    fn probe(env: &Env, path: &Path) -> serde_json::Value {
        let output = tools::command(&env.ffprobe)
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(path)
            .output()
            .expect("ffprobe should run");
        assert!(
            output.status.success(),
            "ffprobe failed on {}",
            path.display()
        );
        serde_json::from_slice(&output.stdout).expect("ffprobe JSON should parse")
    }

    fn streams<'a>(json: &'a serde_json::Value, kind: &str) -> Vec<&'a serde_json::Value> {
        json["streams"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|stream| stream["codec_type"] == kind)
            .collect()
    }

    fn encode(encoder: VideoEncoder) -> VideoEncode {
        VideoEncode {
            encoder,
            speed: 4,
            deinterlace: Deinterlace::Off,
            ..VideoEncode::default()
        }
    }

    fn fast_x264() -> VideoEncode {
        encode(VideoEncoder::X264)
    }

    #[test]
    fn crop_scale_rotate_ten_bit_x265_and_opus_stereo() {
        let Some(env) = env("crop-scale-rotate") else {
            return;
        };
        if !require(&env, &VideoEncoder::X265) {
            return;
        }
        let input = source(&env);
        let output = env.dir.join("out.mkv");
        let settings = JobSettings {
            video: VideoSettings::Encode(VideoEncode {
                crop: Crop::Custom {
                    top: 30,
                    bottom: 30,
                    left: 0,
                    right: 0,
                },
                rotation: Rotation::Deg90,
                max_height: Some(160),
                ten_bit: TenBit::On,
                ..encode(VideoEncoder::X265)
            }),
            audio: AudioSettings {
                tracks: vec![AudioTrackSetting {
                    encoder: AudioEncoder::Opus,
                    mixdown: AudioMixdown::Stereo,
                    bitrate_kbps: Some(64),
                    ..AudioTrackSetting::default()
                }],
                ..AudioSettings::default()
            },
            ..JobSettings::default()
        };
        run(&env, &input, &output, settings).unwrap();
        let json = probe(&env, &output);
        let video = streams(&json, "video")[0];
        assert_eq!(video["codec_name"], "hevc");
        assert_eq!(video["pix_fmt"], "yuv420p10le");
        // 320x180 after cropping, 180x320 after rotation, 90x160 after the height limit.
        assert_eq!(
            (video["width"].as_u64(), video["height"].as_u64()),
            (Some(90), Some(160))
        );
        let audio = streams(&json, "audio");
        assert_eq!(audio.len(), 1);
        assert_eq!(audio[0]["codec_name"], "opus");
        assert_eq!(audio[0]["channels"], 2);
    }

    #[test]
    fn automatic_crop_removes_the_letterbox() {
        let Some(env) = env("auto-crop") else {
            return;
        };
        let input = source(&env);
        let output = env.dir.join("out.mkv");
        let settings = JobSettings {
            video: VideoSettings::Encode(VideoEncode {
                crop: Crop::Automatic,
                ..fast_x264()
            }),
            ..JobSettings::default()
        };
        run(&env, &input, &output, settings).unwrap();
        let json = probe(&env, &output);
        let video = streams(&json, "video")[0];
        let height = video["height"].as_u64().unwrap();
        assert!((176..=180).contains(&height), "cropped height was {height}");
        assert_eq!(video["width"], 320);
    }

    #[test]
    fn imported_srt_keeps_language_and_title_in_mkv() {
        let Some(env) = env("imported-srt-mkv") else {
            return;
        };
        let input = source(&env);
        let srt = env.dir.join("french.srt");
        write_srt(&srt, "Bonjour");
        let output = env.dir.join("out.mkv");
        let mut settings = JobSettings {
            video: VideoSettings::Encode(fast_x264()),
            ..JobSettings::default()
        };
        settings.subtitles.external = vec![ExternalTrack {
            path: srt,
            language: Some("fra".to_owned()),
            title: Some("Francais".to_owned()),
            default: true,
            offset_ms: 500,
            ..ExternalTrack::default()
        }];
        run(&env, &input, &output, settings).unwrap();
        let json = probe(&env, &output);
        let subtitles = streams(&json, "subtitle");
        assert_eq!(subtitles.len(), 1);
        assert_eq!(subtitles[0]["codec_name"], "subrip");
        assert_eq!(subtitles[0]["tags"]["language"], "fra");
        assert_eq!(subtitles[0]["tags"]["title"], "Francais");
        assert_eq!(subtitles[0]["disposition"]["default"], 1);
    }

    fn position(bytes: &[u8], tag: &[u8]) -> usize {
        bytes
            .windows(tag.len())
            .position(|window| window == tag)
            .unwrap()
    }

    #[test]
    fn mp4_converts_text_subtitles_to_mov_text_and_moves_the_index_first() {
        let Some(env) = env("mp4-mov-text") else {
            return;
        };
        let input = source(&env);
        let srt = env.dir.join("english.srt");
        write_srt(&srt, "Hello");
        let output = env.dir.join("out.mp4");
        let mut settings = JobSettings {
            container: Container::Mp4,
            video: VideoSettings::Encode(fast_x264()),
            ..JobSettings::default()
        };
        settings.subtitles.external = vec![ExternalTrack {
            path: srt,
            language: Some("eng".to_owned()),
            ..ExternalTrack::default()
        }];
        run(&env, &input, &output, settings.clone()).unwrap();
        let json = probe(&env, &output);
        let subtitles = streams(&json, "subtitle");
        assert_eq!(subtitles.len(), 1);
        assert_eq!(subtitles[0]["codec_name"], "mov_text");
        assert_eq!(subtitles[0]["tags"]["language"], "eng");
        let bytes = fs::read(&output).unwrap();
        assert!(
            position(&bytes, b"moov") < position(&bytes, b"mdat"),
            "faststart should put moov first"
        );

        settings.web_optimized = false;
        let plain = env.dir.join("plain.mp4");
        run(&env, &input, &plain, settings).unwrap();
        let bytes = fs::read(&plain).unwrap();
        assert!(
            position(&bytes, b"mdat") < position(&bytes, b"moov"),
            "without faststart the index is last"
        );
    }

    #[test]
    fn webm_vp9_opus() {
        let Some(env) = env("webm") else {
            return;
        };
        if !require(&env, &VideoEncoder::Vp9) {
            return;
        }
        let input = source(&env);
        let output = env.dir.join("out.webm");
        let settings = JobSettings {
            container: Container::WebM,
            video: VideoSettings::Encode(encode(VideoEncoder::Vp9)),
            audio: AudioSettings {
                codec: AudioChoice::Opus,
                bitrate_kbps: 64,
                ..AudioSettings::default()
            },
            ..JobSettings::default()
        };
        run(&env, &input, &output, settings).unwrap();
        let json = probe(&env, &output);
        assert_eq!(streams(&json, "video")[0]["codec_name"], "vp9");
        assert_eq!(streams(&json, "audio")[0]["codec_name"], "opus");
    }

    #[test]
    fn two_pass_average_bitrate_completes_and_removes_pass_logs() {
        let Some(env) = env("two-pass") else {
            return;
        };
        let input = source(&env);
        for (index, encoder) in [VideoEncoder::X264, VideoEncoder::X265, VideoEncoder::Vp9]
            .into_iter()
            .enumerate()
        {
            if !require(&env, &encoder) {
                continue;
            }
            let output = env.dir.join(format!("pass-{index}.mkv"));
            let settings = JobSettings {
                video: VideoSettings::Encode(VideoEncode {
                    rate: RateControl::Bitrate(300),
                    passes: 2,
                    turbo_first_pass: false,
                    ..encode(encoder.clone())
                }),
                ..JobSettings::default()
            };
            run(&env, &input, &output, settings)
                .unwrap_or_else(|error| panic!("two-pass {encoder:?} failed: {error}"));
            let json = probe(&env, &output);
            assert!(!streams(&json, "video").is_empty());
        }
        let prefix = format!("kestrel-pass-{}-", std::process::id());
        let leftovers = fs::read_dir(std::env::temp_dir())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .count();
        assert_eq!(
            leftovers, 0,
            "pass-log folders were left in the temp directory"
        );
    }

    #[test]
    fn tags_title_and_chapter_names_are_written() {
        let Some(env) = env("tags-chapters") else {
            return;
        };
        let source_path = env.dir.join("chapters.mkv");
        let metadata_file = env.dir.join("chapters.ffmeta");
        fs::write(
            &metadata_file,
            ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1500\ntitle=Old one\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=1500\nEND=3000\ntitle=Old two\n",
        )
        .unwrap();
        let base = source(&env);
        let made = tools::command(&env.ffmpeg)
            .args(["-hide_banner", "-y", "-i"])
            .arg(&base)
            .arg("-i")
            .arg(&metadata_file)
            .args(["-map", "0", "-map_chapters", "1", "-c", "copy"])
            .arg(&source_path)
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "{}",
            String::from_utf8_lossy(&made.stderr)
        );
        let output = env.dir.join("out.mkv");
        let settings = JobSettings {
            video: VideoSettings::Encode(fast_x264()),
            title: Some("Kestrel Test".to_owned()),
            chapter_names: vec!["Opening".to_owned(), "Finale".to_owned()],
            tags: MediaTags {
                actors: Some("Ann, Ben".to_owned()),
                director: Some("Dee".to_owned()),
                release_date: Some("2026-11-19".to_owned()),
                genre: Some("Test".to_owned()),
                description: Some("A short test".to_owned()),
                comment: Some("Made by Kestrel".to_owned()),
            },
            ..JobSettings::default()
        };
        run(&env, &source_path, &output, settings).unwrap();
        let json = probe(&env, &output);
        let tags = json["format"]["tags"].as_object().unwrap();
        let tag = |key: &str| {
            tags.iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value.as_str().unwrap().to_owned())
                .unwrap_or_else(|| panic!("tag {key} missing in {tags:?}"))
        };
        assert_eq!(tag("title"), "Kestrel Test");
        assert_eq!(tag("artist"), "Ann, Ben");
        assert_eq!(tag("director"), "Dee");
        assert_eq!(tag("date"), "2026-11-19");
        assert_eq!(tag("genre"), "Test");
        assert_eq!(tag("description"), "A short test");
        assert_eq!(tag("comment"), "Made by Kestrel");
        let chapters = tools::command(&env.ffprobe)
            .args(["-v", "error", "-show_chapters", "-of", "json"])
            .arg(&output)
            .output()
            .unwrap();
        let chapters: serde_json::Value = serde_json::from_slice(&chapters.stdout).unwrap();
        let titles = chapters["chapters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|chapter| chapter["tags"]["title"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(titles, ["Opening", "Finale"]);
    }

    fn frame_hash(env: &Env, path: &Path) -> String {
        let output = tools::command(&env.ffmpeg)
            .args(["-v", "error", "-ss", "1", "-i"])
            .arg(path)
            .args(["-frames:v", "1", "-f", "md5", "-"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn burned_subtitle_from_a_path_with_special_characters_changes_the_picture() {
        let Some(env) = env("burn-in") else {
            return;
        };
        let input = source(&env);
        let special = env.dir.join("Bob's, [test]; x");
        fs::create_dir_all(&special).unwrap();
        let srt = special.join("burn.srt");
        write_srt(&srt, "BURNED SUBTITLE TEXT");
        let plain = env.dir.join("plain.mkv");
        let burned = env.dir.join("burned.mkv");
        let base = JobSettings {
            video: VideoSettings::Encode(VideoEncode {
                rate: RateControl::Native(12.0),
                ..fast_x264()
            }),
            ..JobSettings::default()
        };
        run(&env, &input, &plain, base.clone()).unwrap();
        let mut with_burn = base;
        with_burn.subtitles.external = vec![ExternalTrack {
            path: srt,
            burn_in: true,
            offset_ms: 100,
            ..ExternalTrack::default()
        }];
        run(&env, &input, &burned, with_burn).unwrap();
        let json = probe(&env, &burned);
        assert!(
            streams(&json, "subtitle").is_empty(),
            "burned text must not be soft"
        );
        assert_ne!(frame_hash(&env, &plain), frame_hash(&env, &burned));
    }

    #[test]
    fn audio_encoders_gain_drc_and_sample_rate() {
        let Some(env) = env("audio-encoders") else {
            return;
        };
        let input = source(&env);
        let output = env.dir.join("out.mkv");
        let encoders = [
            (AudioEncoder::Aac, "aac"),
            (AudioEncoder::Ac3, "ac3"),
            (AudioEncoder::Eac3, "eac3"),
            (AudioEncoder::Flac, "flac"),
            (AudioEncoder::Mp3, "mp3"),
            (AudioEncoder::Vorbis, "vorbis"),
            (AudioEncoder::Opus, "opus"),
        ];
        let settings = JobSettings {
            audio: AudioSettings {
                tracks: encoders
                    .iter()
                    .map(|(encoder, _)| AudioTrackSetting {
                        encoder: *encoder,
                        sample_rate: Some(48000),
                        gain_db: 3.0,
                        drc: 1.0,
                        bitrate_kbps: Some(96),
                        name: Some("Track".to_owned()),
                        ..AudioTrackSetting::default()
                    })
                    .collect(),
                ..AudioSettings::default()
            },
            ..JobSettings::default()
        };
        run(&env, &input, &output, settings).unwrap();
        let json = probe(&env, &output);
        let audio = streams(&json, "audio");
        let codecs = audio
            .iter()
            .map(|stream| stream["codec_name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            codecs,
            encoders
                .iter()
                .map(|(_, name)| (*name).to_owned())
                .collect::<Vec<_>>()
        );
        assert!(audio.iter().all(|stream| stream["sample_rate"] == "48000"));
        assert!(
            audio
                .iter()
                .all(|stream| stream["tags"]["title"] == "Track")
        );
    }

    #[test]
    fn dimension_and_filter_controls_all_run_in_ffmpeg() {
        let Some(env) = env("filters-smoke") else {
            return;
        };
        let input = source(&env);
        let variants = [
            VideoEncode {
                detelecine: Detelecine::Default,
                deinterlace: Deinterlace::On,
                deinterlace_filter: DeinterlaceFilter::Yadif,
                deinterlace_preset: DeinterlacePreset::Bob,
                denoise: Denoise::Medium,
                denoise_filter: DenoiseFilter::NlMeans,
                denoise_tune: DenoiseTune::Grain,
                chroma_smooth: FilterStrength::Medium,
                sharpen: SharpenFilter::Unsharp,
                sharpen_preset: FilterStrength::Strong,
                deblock: FilterStrength::Light,
                grayscale: true,
                ..fast_x264()
            },
            VideoEncode {
                deinterlace: Deinterlace::On,
                deinterlace_filter: DeinterlaceFilter::Bwdif,
                denoise: Denoise::Light,
                sharpen: SharpenFilter::LapSharp,
                sharpen_preset: FilterStrength::VeryStrong,
                deblock: FilterStrength::VeryStrong,
                colorspace: ColorSpaceTarget::Bt601,
                flip_horizontal: true,
                rotation: Rotation::Deg180,
                max_width: Some(256),
                max_height: Some(256),
                border_fill: BorderFill::Both,
                border_color: "#202020".to_owned(),
                frame_rate: FrameRate::Fixed(12.0),
                frame_rate_mode: FrameRateMode::Pfr,
                ..fast_x264()
            },
            VideoEncode {
                deinterlace: Deinterlace::On,
                deinterlace_filter: DeinterlaceFilter::Decomb,
                colorspace: ColorSpaceTarget::Bt2020,
                ten_bit: TenBit::On,
                allow_upscaling: true,
                max_width: Some(640),
                modulus: 8,
                ..fast_x264()
            },
        ];
        for (index, encode) in variants.into_iter().enumerate() {
            let output = env.dir.join(format!("filters-{index}.mkv"));
            run(
                &env,
                &input,
                &output,
                JobSettings {
                    video: VideoSettings::Encode(encode),
                    ..JobSettings::default()
                },
            )
            .unwrap_or_else(|error| panic!("filter variant {index} failed: {error}"));
            let json = probe(&env, &output);
            assert!(!streams(&json, "video").is_empty());
        }
    }
}
