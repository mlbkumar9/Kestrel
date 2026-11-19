use super::*;
use kestrel_core::mkv::{TrackInfo, VideoInfo};
use kestrel_core::presets::builtins;
use kestrel_core::transcode::{JobResources, build_ffmpeg_job_passes};
use std::path::Path;

fn software_set() -> EncoderSet {
    EncoderSet {
        software: vec![VideoEncoder::X264, VideoEncoder::X265],
        hardware: vec![VideoEncoder::Hardware {
            vendor: HwVendor::Qsv,
            codec: HwCodec::Hevc,
        }],
        audio: vec!["aac".to_owned(), "libopus".to_owned()],
    }
}

fn source_meta() -> MediaMetadata {
    MediaMetadata {
        video_codec: Some("hevc".to_owned()),
        video: Some(VideoInfo {
            codec_id: "hevc".to_owned(),
            width: Some(1920),
            height: Some(1080),
            fps: Some(24.0),
            interlaced: Some(false),
            ffprobe_index: Some(0),
            ..VideoInfo::default()
        }),
        audio_tracks: vec![
            TrackInfo {
                id: 1,
                ffprobe_index: Some(1),
                language: "jpn".to_owned(),
                codec: "aac".to_owned(),
                codec_id: "A_AAC".to_owned(),
                channels: Some(2),
                ..TrackInfo::default()
            },
            TrackInfo {
                id: 2,
                ffprobe_index: Some(2),
                language: "eng".to_owned(),
                codec: "ac3".to_owned(),
                codec_id: "A_AC3".to_owned(),
                channels: Some(6),
                ..TrackInfo::default()
            },
        ],
        container: Some("matroska".to_owned()),
        duration: Some(60.0),
        ..MediaMetadata::default()
    }
}

/// Every control the form carries set away from its default.
fn rich_settings() -> JobSettings {
    let languages = vec!["eng".to_owned(), "jpn".to_owned()];
    let mut subtitles = SubtitleSettings {
        languages: languages.clone(),
        include: true,
        selection: TrackSelection::FirstMatching,
        forced_only: true,
        default_track: SubtitleDefault::Forced,
        external: vec![ExternalTrack {
            path: PathBuf::from("extras/sub.fr.srt"),
            kind: ExternalKind::Subtitle,
            language: Some("fra".to_owned()),
            title: None,
            default: true,
            forced: false,
            offset_ms: 500,
            charset: Some("Windows-1252".to_owned()),
            burn_in: false,
        }],
        ..SubtitleSettings::default()
    };
    subtitles.set_burn_behavior(BurnBehavior::ForeignThenFirst);
    JobSettings {
        container: Container::Mp4,
        video: VideoSettings::Encode(VideoEncode {
            encoder: VideoEncoder::X265,
            ten_bit: TenBit::On,
            rate: RateControl::Bitrate(5000),
            max_width: Some(1280),
            max_height: Some(720),
            frame_rate: FrameRate::Fixed(25.0),
            frame_rate_mode: FrameRateMode::Cfr,
            deinterlace: Deinterlace::Auto,
            deinterlace_filter: DeinterlaceFilter::Bwdif,
            deinterlace_preset: DeinterlacePreset::Fast,
            interlace_detection: true,
            denoise: Denoise::Medium,
            denoise_filter: DenoiseFilter::NlMeans,
            denoise_tune: DenoiseTune::Grain,
            chroma_smooth: FilterStrength::Light,
            sharpen: SharpenFilter::Unsharp,
            sharpen_preset: FilterStrength::Strong,
            deblock: FilterStrength::Medium,
            colorspace: ColorSpaceTarget::Bt709,
            grayscale: true,
            detelecine: Detelecine::Default,
            tone_map_to_sdr: true,
            tone_map_operator: ToneMapOperator::Mobius,
            crop: Crop::Custom {
                top: 8,
                bottom: 8,
                left: 0,
                right: 2,
            },
            rotation: Rotation::Deg90,
            flip_horizontal: true,
            anamorphic: Anamorphic::Custom { num: 32, den: 27 },
            modulus: 8,
            allow_upscaling: true,
            border_fill: BorderFill::Custom {
                top: 4,
                bottom: 6,
                left: 2,
                right: 2,
            },
            border_color: "#112233".to_owned(),
            passes: 2,
            turbo_first_pass: false,
            encoder_preset: Some("slow".to_owned()),
            tune: Some("grain".to_owned()),
            profile: Some("main10".to_owned()),
            level: Some("4.1".to_owned()),
            fast_decode: true,
            advanced_options: "ref=4:bframes=6".to_owned(),
            ..VideoEncode::default()
        }),
        audio: AudioSettings {
            languages,
            codec: AudioChoice::Opus,
            bitrate_kbps: 192,
            mixdown: Mixdown::Stereo,
            tracks: vec![
                AudioTrackSetting {
                    source: 0,
                    encoder: AudioEncoder::Opus,
                    mixdown: AudioMixdown::Surround51,
                    sample_rate: Some(48_000),
                    bitrate_kbps: Some(320),
                    gain_db: 3.0,
                    drc: 2.0,
                    name: Some("Main".to_owned()),
                },
                AudioTrackSetting {
                    source: 1,
                    encoder: AudioEncoder::Passthru(PassthruCodec::TrueHd),
                    mixdown: AudioMixdown::Source,
                    sample_rate: None,
                    bitrate_kbps: Some(160),
                    gain_db: 0.0,
                    drc: 0.0,
                    name: None,
                },
            ],
            selection: TrackSelection::FirstMatching,
            allowed_passthru: PassthruCodec::ALL
                .into_iter()
                .filter(|codec| !matches!(codec, PassthruCodec::Dts | PassthruCodec::Mp3))
                .collect(),
            fallback_encoder: Some(AudioEncoder::Ac3),
        },
        subtitles,
        keep_chapters: false,
        keep_metadata: false,
        title: Some("My Title".to_owned()),
        web_optimized: false,
        align_av_start: true,
        keep_attachments: true,
        chapter_names: Vec::new(),
        tags: MediaTags {
            actors: Some("A. Actor".to_owned()),
            director: Some("D. Director".to_owned()),
            release_date: Some("2026-11-19".to_owned()),
            genre: Some("Drama".to_owned()),
            description: Some("A description".to_owned()),
            comment: Some("A comment".to_owned()),
        },
        track_edits: Default::default(),
    }
}

#[test]
fn defaults_match_the_prototype() {
    let form = TranscodeForm::default();
    assert_eq!(form.encoder, VideoEncoder::X265);
    assert_eq!(form.ten_bit, TenBit::On);
    assert_eq!(form.video_summary(), "H.265 10-bit \u{b7} CRF 22");
    assert_eq!(form.rf, 22);
    assert_eq!(form.speed_label(), "medium");
    assert_eq!(form.quality_label(), "CRF");
    assert_eq!(form.container, Container::Mkv);
    assert!(form.web_optimized && form.align_av && form.passthru_metadata);
    assert!(!form.keep_attachments);
}

#[test]
fn form_to_settings_to_form_is_lossless_for_every_control() {
    let settings = rich_settings();
    let form = TranscodeForm::from_job_settings("Rich", &settings);
    assert_eq!(form.preset, "Rich");
    assert_eq!(form.to_job_settings(), settings);
}

#[test]
fn settings_survive_a_second_round_through_the_form() {
    let first = TranscodeForm::from_job_settings("Rich", &rich_settings());
    let mapped = first.to_job_settings();
    let second = TranscodeForm::from_job_settings("Rich", &mapped);
    assert_eq!(second.to_job_settings(), mapped);
}

#[test]
fn constant_quality_maps_to_the_native_encoder_value() {
    let form = TranscodeForm {
        rf: 19,
        ..TranscodeForm::default()
    };
    let VideoSettings::Encode(encode) = form.to_job_settings().video else {
        panic!("expected an encode");
    };
    assert_eq!(encode.rate, RateControl::Native(19.0));
    let back = TranscodeForm::from_job_settings("x", &form.to_job_settings());
    assert_eq!(back.rf, 19);
}

#[test]
fn every_builtin_preset_loads_and_maps_back() {
    for preset in builtins(&EncoderSet::default()) {
        let form = TranscodeForm::from_job_settings(&preset.name, &preset.settings);
        assert_eq!(form.preset, preset.name);
        let back = form.to_job_settings();
        assert_eq!(back.container, preset.settings.container, "{}", preset.id);
        if let (VideoSettings::Encode(want), VideoSettings::Encode(got)) =
            (&preset.settings.video, &back.video)
        {
            assert_eq!(got.encoder, want.encoder, "{}", preset.id);
            assert_eq!(got.max_height, want.max_height, "{}", preset.id);
            assert_eq!(got.tone_map_to_sdr, want.tone_map_to_sdr, "{}", preset.id);
            assert_eq!(got.crop, want.crop, "{}", preset.id);
            match (&got.rate, &want.rate) {
                (RateControl::Native(rf), RateControl::Quality(quality)) => {
                    assert_eq!(*rf, f32::from(rf_from_quality(&want.encoder, *quality)));
                }
                (a, b) => assert_eq!(a, b, "{}", preset.id),
            }
            let presets = encoder_presets(&want.encoder);
            assert!(
                presets.is_empty()
                    || got
                        .encoder_preset
                        .as_deref()
                        .is_some_and(|name| presets.contains(&name)),
                "{} preset {:?}",
                preset.id,
                got.encoder_preset
            );
        }
        assert_eq!(
            back.audio.codec, preset.settings.audio.codec,
            "{}",
            preset.id
        );
        assert_eq!(back.subtitles.burn_in, preset.settings.subtitles.burn_in);
    }
}

#[test]
fn archive_preset_selects_ten_bit_svt_av1() {
    let archive = kestrel_core::presets::builtin_preset("archive").unwrap();
    let form = TranscodeForm::from_job_settings(&archive.name, &archive.settings);
    assert_eq!(form.encoder, VideoEncoder::SvtAv1);
    assert_eq!(form.ten_bit, TenBit::On);
    assert_eq!(form.container, Container::Mkv);
}

#[test]
fn encoder_picker_lists_only_what_detection_found() {
    let form = TranscodeForm::default();
    let set = software_set();
    let labels: Vec<String> = form
        .encoder_options(Some(&set))
        .into_iter()
        .map(|choice| choice.label)
        .collect();
    assert!(
        labels.contains(&"H.265 (x265) 10-bit".to_owned()),
        "{labels:?}"
    );
    assert!(labels.contains(&"H.264 (x264) 8-bit".to_owned()));
    assert!(labels.contains(&"H.265 (QSV) 10-bit".to_owned()));
    assert!(!labels.iter().any(|label| label.contains("NVENC")));
    assert!(!labels.iter().any(|label| label.contains("SVT-AV1")));
    let options = form.encoder_options(Some(&set));
    assert_eq!(
        options[form.encoder_index(&options).unwrap()].encoder,
        VideoEncoder::X265
    );
}

#[test]
fn encoder_picker_keeps_the_current_choice_visible_when_unavailable() {
    let form = TranscodeForm {
        encoder: VideoEncoder::SvtAv1,
        ..TranscodeForm::default()
    };
    let options = form.encoder_options(Some(&software_set()));
    let current = &options[form.encoder_index(&options).unwrap()];
    assert_eq!(current.encoder, VideoEncoder::SvtAv1);
    assert!(
        current.label.ends_with("(not available)"),
        "{}",
        current.label
    );
    assert!(
        TranscodeForm::default()
            .encoder_options(None)
            .iter()
            .all(|choice| choice.encoder == VideoEncoder::X265)
    );
}

#[test]
fn preset_and_tune_lists_come_from_core_and_follow_the_encoder() {
    let mut form = TranscodeForm::default();
    assert_eq!(form.preset_name, "medium");
    assert!(encoder_presets(&form.encoder).contains(&"medium"));
    form.tune = "grain".to_owned();
    form.profile = "main10".to_owned();
    form.fast_decode = true;
    let svt = EncoderChoice {
        label: String::new(),
        encoder: VideoEncoder::SvtAv1,
        ten_bit: TenBit::On,
    };
    form.set_encoder(&svt);
    assert!(encoder_presets(&VideoEncoder::SvtAv1).contains(&form.preset_name.as_str()));
    assert_eq!(form.tune, "", "x265 tune is not an SVT-AV1 tune");
    assert_eq!(form.profile, "");
    assert!(form.fast_decode, "SVT-AV1 supports fast decode");
    assert_eq!(form.rf, default_rf(&VideoEncoder::SvtAv1));
    let vp9 = EncoderChoice {
        encoder: VideoEncoder::Vp9,
        ..svt.clone()
    };
    form.set_encoder(&vp9);
    assert!(!form.fast_decode, "VP9 has no fast decode");
    assert_eq!(form.advanced, "");
}

#[test]
fn preset_slider_runs_fastest_to_slowest_for_every_encoder() {
    let mut form = TranscodeForm::default();
    form.set_preset_fraction(0.0);
    assert_eq!(form.preset_name, "ultrafast");
    form.set_preset_fraction(1.0);
    assert_eq!(form.preset_name, "placebo");
    form.set_encoder(&EncoderChoice {
        label: String::new(),
        encoder: VideoEncoder::SvtAv1,
        ten_bit: TenBit::Off,
    });
    form.set_preset_fraction(0.0);
    assert_eq!(form.preset_name, "13", "SVT-AV1 preset 13 is the fastest");
    form.set_preset_fraction(1.0);
    assert_eq!(form.preset_name, "0");
}

#[test]
fn he_aac_is_listed_only_with_libfdk_aac() {
    let without = audio_encoder_options(Some(&software_set()), None);
    assert!(
        without
            .iter()
            .all(|(encoder, _)| *encoder != AudioEncoder::HeAac)
    );
    let mut with = software_set();
    with.audio.push("libfdk_aac".to_owned());
    let listed = audio_encoder_options(Some(&with), None);
    assert!(
        listed
            .iter()
            .any(|(encoder, _)| *encoder == AudioEncoder::HeAac)
    );
    assert!(
        audio_encoder_options(None, None)
            .iter()
            .all(|(e, _)| *e != AudioEncoder::HeAac)
    );
    let kept = audio_encoder_options(None, Some(AudioEncoder::HeAac));
    assert!(
        kept.iter()
            .any(|(encoder, _)| *encoder == AudioEncoder::HeAac)
    );
}

#[test]
fn source_tracks_seed_audio_rows_and_scaled_size() {
    let mut form = TranscodeForm {
        res_limit: 4,
        ..TranscodeForm::default()
    };
    form.seed_from_source(&source_meta(), "movie", "out");
    assert_eq!(form.audio_tracks.len(), 2);
    assert_eq!(form.audio_tracks[1].source_index, 1);
    assert_eq!((form.scaled_width, form.scaled_height), (1280, 720));
    assert_eq!(form.save_as, "movie.mkv");
    let tracks = form.to_job_settings().audio.tracks;
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[1].source, 1);
    assert_eq!(tracks[0].encoder, AudioEncoder::AutoPassthru);
}

#[test]
fn scaled_size_follows_crop_limit_and_modulus() {
    let mut form = TranscodeForm {
        crop_mode: CROP_CUSTOM,
        crop: [140, 140, 0, 0],
        ..TranscodeForm::default()
    };
    assert_eq!(form.output_size((1920, 1080)), (1920, 800));
    form.res_limit = 5;
    assert_eq!(form.output_size((1920, 1080)), (1382, 576));
    form.modulus = 1;
    assert_eq!(form.output_size((1920, 1080)), (1380, 576), "modulus 4");
    form.optimal_size = false;
    form.scaled_width = 640;
    form.scaled_height = 0;
    assert_eq!(form.output_size((1920, 1080)).0, 640);
    form.scaled_width = 4000;
    assert_eq!(
        form.output_size((1920, 1080)).0,
        1920,
        "no upscaling by default"
    );
    form.allow_upscale = true;
    assert_eq!(form.output_size((1920, 1080)).0, 4000);
}

#[test]
fn final_dimensions_apply_crop_and_limit() {
    let form = TranscodeForm {
        crop_mode: CROP_CUSTOM,
        crop: [280, 280, 0, 0],
        res_limit: 3,
        ..TranscodeForm::default()
    };
    assert_eq!(
        form.final_dimensions(Some((3840, 2160))),
        "3840x2160 -> crop 3840x1600 -> 2592x1080 (2.40:1)"
    );
}

#[test]
fn imported_subtitles_map_to_external_tracks() {
    let mut form = TranscodeForm {
        charset: 1,
        ..TranscodeForm::default()
    };
    form.sub_tracks.push(SubTrackForm {
        source: "movie.es.srt".to_owned(),
        kind: "SRT (external)".to_owned(),
        language: "es".to_owned(),
        forced_only: true,
        burn: true,
        default: true,
        offset_ms: -250,
        external: Some(PathBuf::from("movie.es.srt")),
    });
    form.sub_tracks.push(SubTrackForm {
        source: "1 \u{b7} eng SRT".to_owned(),
        kind: "SRT".to_owned(),
        language: "eng".to_owned(),
        forced_only: false,
        burn: false,
        default: false,
        offset_ms: 0,
        external: None,
    });
    let settings = form.to_job_settings();
    assert_eq!(
        settings.subtitles.external,
        vec![ExternalTrack {
            path: PathBuf::from("movie.es.srt"),
            kind: ExternalKind::Subtitle,
            language: Some("spa".to_owned()),
            title: None,
            default: true,
            forced: true,
            offset_ms: -250,
            charset: Some("Windows-1252".to_owned()),
            burn_in: true,
        }]
    );
}

#[test]
fn subtitle_behaviours_map_to_core() {
    let form = TranscodeForm {
        sub_selection: 0,
        sub_burn: 3,
        sub_forced_only: true,
        sub_default: 1,
        ..TranscodeForm::default()
    };
    let subtitles = form.to_job_settings().subtitles;
    assert!(!subtitles.include);
    assert_eq!(subtitles.selection, TrackSelection::None);
    assert_eq!(subtitles.burn_behavior(), BurnBehavior::ForeignThenFirst);
    assert!(subtitles.forced_only);
    assert_eq!(subtitles.default_track, SubtitleDefault::None);
}

#[test]
fn chapter_names_are_sent_only_when_a_row_was_renamed() {
    let row = |name: &str| ChapterRow {
        start: 0.0,
        duration: 10.0,
        name: name.to_owned(),
        original: name.to_owned(),
    };
    let mut form = TranscodeForm {
        chapters: vec![row("Chapter 1"), row("Chapter 2")],
        ..TranscodeForm::default()
    };
    assert!(form.to_job_settings().chapter_names.is_empty());
    form.chapters[1].name = "Finale".to_owned();
    assert_eq!(
        form.to_job_settings().chapter_names,
        vec!["Chapter 1".to_owned(), "Finale".to_owned()]
    );
    form.create_chapters = false;
    assert!(form.to_job_settings().chapter_names.is_empty());
    form.create_chapters = true;
    assert!(
        form.apply_chapter_names(vec!["Only one".to_owned()])
            .is_err()
    );
    form.apply_chapter_names(vec!["A".to_owned(), "B".to_owned()])
        .unwrap();
    assert_eq!(form.chapters[0].name, "A");
}

#[test]
fn tags_and_container_options_map_to_core() {
    let form = TranscodeForm {
        tag_actors: " Cast ".to_owned(),
        tag_genre: "Drama".to_owned(),
        tag_comment: "   ".to_owned(),
        web_optimized: false,
        align_av: false,
        keep_attachments: true,
        ..TranscodeForm::default()
    };
    let settings = form.to_job_settings();
    assert_eq!(settings.tags.actors.as_deref(), Some("Cast"));
    assert_eq!(settings.tags.genre.as_deref(), Some("Drama"));
    assert_eq!(settings.tags.comment, None);
    assert!(!settings.web_optimized && !settings.align_av_start && settings.keep_attachments);
}

#[test]
fn mapped_settings_build_a_valid_ffmpeg_command() {
    let meta = source_meta();
    let mut form = TranscodeForm {
        crop_mode: 2,
        ..TranscodeForm::default()
    };
    form.seed_from_source(&meta, "movie", "out");
    form.audio_tracks[0].encoder = AudioEncoder::Aac;
    let passes = build_ffmpeg_job_passes(
        Path::new("in.mkv"),
        Path::new("out.mkv"),
        &meta,
        &form.to_job_settings(),
        &software_set(),
        &JobResources::default(),
    )
    .expect("core accepts the mapped settings");
    assert_eq!(passes.len(), 1);
    let text: Vec<String> = passes[0]
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(
        text.windows(2).any(|pair| pair == ["-c:v", "libx265"]),
        "{text:?}"
    );
    assert!(
        text.windows(2).any(|pair| pair == ["-crf", "22"]),
        "{text:?}"
    );
    assert!(
        text.windows(2).any(|pair| pair == ["-preset", "medium"]),
        "{text:?}"
    );

    form.rate_mode = 1;
    let two_pass = build_ffmpeg_job_passes(
        Path::new("in.mkv"),
        Path::new("out.mkv"),
        &meta,
        &form.to_job_settings(),
        &software_set(),
        &JobResources {
            pass_log_prefix: Some(PathBuf::from("pass")),
            ..JobResources::default()
        },
    )
    .expect("two-pass bitrate settings are valid");
    assert_eq!(two_pass.len(), 2);
}

#[test]
fn language_codes_normalise_to_three_letters() {
    assert_eq!(language_code("es").as_deref(), Some("spa"));
    assert_eq!(language_code(" ENG ").as_deref(), Some("eng"));
    assert_eq!(language_code("und"), None);
    assert_eq!(language_code("xx"), None);
    assert_eq!(language_code("toolong"), None);
}

#[test]
fn container_change_renames_the_output() {
    let mut form = TranscodeForm {
        save_as: "movie.mkv".to_owned(),
        ..TranscodeForm::default()
    };
    form.set_container(Container::Mp4);
    assert_eq!(form.save_as, "movie.mp4");
}

#[test]
fn heuristic_estimate_shrinks_with_higher_rf() {
    let mut form = TranscodeForm {
        rf: 18,
        ..TranscodeForm::default()
    };
    let high = heuristic_size(&form, 10_000_000_000, Some(3600.0));
    form.rf = 28;
    let low = heuristic_size(&form, 10_000_000_000, Some(3600.0));
    assert!(low < high);
    assert!(heuristic_minutes(&form, Some(3600.0)) >= 1);
}

#[test]
fn copy_video_round_trips_through_the_form_and_the_builtin_remux_preset() {
    let remux = kestrel_core::presets::builtin_preset("remux").unwrap();
    assert_eq!(remux.settings.video, VideoSettings::Copy);
    // A form that was encoding loads the Copy preset and stops encoding.
    let mut form = TranscodeForm::from_job_settings("Rich", &rich_settings());
    assert!(!form.copy_video);
    assert!(matches!(
        form.to_job_settings().video,
        VideoSettings::Encode(_)
    ));
    form.apply_job_settings(&remux.name, &remux.settings);
    assert!(form.copy_video);
    assert_eq!(form.to_job_settings().video, VideoSettings::Copy);
    assert_eq!(form.video_summary(), COPY_VIDEO_LABEL);
    // Loading an encode preset afterwards switches back, with that preset's encoder.
    let archive = kestrel_core::presets::builtin_preset("archive").unwrap();
    form.apply_job_settings(&archive.name, &archive.settings);
    assert!(!form.copy_video);
    assert_eq!(form.encoder, VideoEncoder::SvtAv1);
    assert!(matches!(
        form.to_job_settings().video,
        VideoSettings::Encode(_)
    ));
    // Settings saved from a copy form reload as copy.
    let copying = TranscodeForm {
        copy_video: true,
        ..TranscodeForm::default()
    };
    let saved = copying.to_job_settings();
    assert_eq!(saved.video, VideoSettings::Copy);
    assert!(TranscodeForm::from_job_settings("Saved", &saved).copy_video);
}

#[test]
fn encoder_picker_offers_copy_first_and_maps_positions() {
    let set = software_set();
    let mut form = TranscodeForm::default();
    let options = form.encoder_options(Some(&set));
    let labels = TranscodeForm::picker_labels(&options);
    assert_eq!(labels[0], COPY_VIDEO_LABEL);
    assert_eq!(labels.len(), options.len() + 1);
    assert_eq!(
        form.picker_index(&options),
        form.encoder_index(&options).unwrap() + 1
    );
    form.pick_encoder(&options, 0);
    assert!(form.copy_video);
    assert_eq!(form.picker_index(&options), 0);
    let x264 = options
        .iter()
        .position(|choice| choice.encoder == VideoEncoder::X264)
        .unwrap();
    form.pick_encoder(&options, x264 + 1);
    assert!(!form.copy_video);
    assert_eq!(form.encoder, VideoEncoder::X264);
}

#[test]
fn copy_video_with_burn_in_is_reported_as_a_warning() {
    let form = TranscodeForm {
        copy_video: true,
        sub_burn: 2,
        ..TranscodeForm::default()
    };
    let warnings = kestrel_core::compat::job_warnings(&source_meta(), &form.to_job_settings());
    assert!(
        warnings.iter().any(|warning| warning.contains("burn-in")),
        "{warnings:?}"
    );
}
