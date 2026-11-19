use crate::formats::Container;
use crate::mkv::{self, MediaMetadata};
use crate::transcode::{JobSettings, VideoSettings};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamPlan {
    Copy,
    Convert(&'static str),
    Drop(String),
}

pub fn video_copy_plan(codec_id: &str, container: Container) -> StreamPlan {
    let codec = normalize(codec_id);
    let compatible = match container {
        Container::Mkv => true,
        Container::Mp4 => matches!(codec.as_str(), "h264" | "hevc" | "av1" | "mpeg4" | "vp9"),
        Container::Mov => matches!(
            codec.as_str(),
            "h264" | "hevc" | "av1" | "mpeg4" | "mpeg2video"
        ),
        Container::WebM => matches!(codec.as_str(), "vp8" | "vp9" | "av1"),
    };
    if compatible {
        StreamPlan::Copy
    } else {
        StreamPlan::Drop(format!(
            "Video codec {codec_id} needs re-encoding for {}",
            container.label()
        ))
    }
}

pub fn audio_copy_plan(codec_id: &str, container: Container) -> StreamPlan {
    let codec = normalize(codec_id);
    let compatible = match container {
        Container::Mkv => true,
        Container::Mp4 => matches!(
            codec.as_str(),
            "aac" | "ac3" | "eac3" | "mp3" | "opus" | "flac" | "alac"
        ),
        Container::Mov => {
            matches!(codec.as_str(), "aac" | "ac3" | "eac3" | "mp3" | "alac")
                || codec.starts_with("pcm_")
        }
        Container::WebM => matches!(codec.as_str(), "opus" | "vorbis"),
    };
    if compatible {
        StreamPlan::Copy
    } else {
        StreamPlan::Convert(if container == Container::WebM {
            "libopus"
        } else {
            "aac"
        })
    }
}

/// Reason a remux-only job cannot keep an audio track in `container`; `None` when it can.
/// A remux never converts audio, so such a track is dropped.
pub fn remux_audio_drop_reason(codec_id: &str, container: Container) -> Option<String> {
    match audio_copy_plan(codec_id, container) {
        StreamPlan::Convert(_) => Some(format!(
            "{} audio cannot be stored in {}",
            codec_id.trim().to_ascii_uppercase(),
            container.label()
        )),
        StreamPlan::Copy | StreamPlan::Drop(_) => None,
    }
}

/// Error for a remux-only job whose every audio track was dropped (`reasons` come from
/// [`remux_audio_drop_reason`]).
pub fn remux_audio_failure(reasons: &[String], container: Container) -> String {
    let mut codecs: Vec<&str> = Vec::new();
    for reason in reasons {
        let codec = reason.split(" audio").next().unwrap_or(reason);
        if !codecs.contains(&codec) {
            codecs.push(codec);
        }
    }
    format!(
        "{} audio cannot be stored in {}; choose MKV or use Transcode",
        codecs.join(", "),
        container.label()
    )
}

/// Selected audio tracks that a remux-only job drops, as `(number, reason)` warnings.
pub fn remux_audio_warnings(metadata: &MediaMetadata, settings: &JobSettings) -> Vec<String> {
    let Ok(tracks) = crate::transcode::audio::selected_audio_tracks(metadata, settings) else {
        return Vec::new();
    };
    tracks
        .into_iter()
        .filter(|track| {
            settings
                .track_edits
                .edit_for(track.id)
                .is_none_or(|edit| edit.keep)
        })
        .filter_map(|track| {
            let number = metadata
                .audio_tracks
                .iter()
                .position(|candidate| std::ptr::eq(candidate, track))
                .map_or(0, |position| position + 1);
            let codec = if track.codec_id.is_empty() {
                track.codec.as_str()
            } else {
                track.codec_id.as_str()
            };
            remux_audio_drop_reason(codec, settings.container)
                .map(|reason| format!("Audio track {number} will be dropped: {reason}"))
        })
        .collect()
}

pub fn subtitle_plan(codec_id: &str, container: Container) -> StreamPlan {
    let codec = normalize(codec_id);
    if is_image_subtitle(&codec) {
        return match container {
            Container::Mkv => StreamPlan::Copy,
            Container::Mp4 | Container::Mov => StreamPlan::Drop(
                "Picture-based subtitles can't be stored in MP4/MOV — burn in or choose MKV"
                    .to_owned(),
            ),
            Container::WebM => StreamPlan::Drop(
                "Picture-based subtitles can't be stored in WebM — burn in or choose MKV"
                    .to_owned(),
            ),
        };
    }

    if !is_text_subtitle(&codec) {
        return StreamPlan::Drop(format!(
            "Subtitle codec {codec_id} is not supported in {}",
            container.label()
        ));
    }

    match container {
        Container::Mkv if matches!(codec.as_str(), "subrip" | "srt" | "ass" | "ssa" | "webvtt") => {
            StreamPlan::Copy
        }
        Container::Mkv => StreamPlan::Convert("srt"),
        Container::Mp4 | Container::Mov if codec == "mov_text" => StreamPlan::Copy,
        Container::Mp4 | Container::Mov => StreamPlan::Convert("mov_text"),
        Container::WebM if codec == "webvtt" => StreamPlan::Copy,
        Container::WebM => StreamPlan::Convert("webvtt"),
    }
}

pub fn job_warnings(metadata: &MediaMetadata, settings: &JobSettings) -> Vec<String> {
    let mut warnings = Vec::new();
    let video_codec = metadata
        .video
        .as_ref()
        .map(|video| video.codec_id.as_str())
        .or(metadata.video_codec.as_deref());
    if let Some(codec) = video_codec
        && matches!(&settings.video, VideoSettings::Copy)
        && let StreamPlan::Drop(_) = video_copy_plan(codec, settings.container)
    {
        warnings.push(format!(
            "Video must be re-encoded for {}",
            settings.container.label()
        ));
    }

    if matches!(&settings.video, VideoSettings::Copy)
        && (settings.subtitles.burn_in != crate::transcode::BurnIn::None
            || settings
                .subtitles
                .external
                .iter()
                .any(|track| track.burn_in))
    {
        warnings.push("Subtitle burn-in needs the video to be re-encoded".to_owned());
    }

    if settings.audio.tracks.is_empty()
        && !mkv::normalize_languages(&settings.audio.languages).is_empty()
        && !metadata.audio_tracks.is_empty()
        && settings.audio.selection == crate::transcode::TrackSelection::AllMatching
        && mkv::matching_tracks(&metadata.audio_tracks, &settings.audio.languages).is_empty()
    {
        warnings.push("No matching audio languages found; keeping all audio tracks".to_owned());
    }
    if let Err(error) = crate::transcode::audio::plan_audio(metadata, settings) {
        warnings.push(error);
    }
    if settings.is_remux_only() {
        warnings.extend(remux_audio_warnings(metadata, settings));
    }

    if settings.subtitles.include {
        for track in crate::transcode::soft_subtitle_tracks(metadata, settings) {
            if let StreamPlan::Drop(reason) = subtitle_plan(&track.codec_id, settings.container) {
                warnings.push(reason);
            }
        }
        for track in settings
            .subtitles
            .external
            .iter()
            .filter(|track| !track.burn_in)
        {
            match crate::transcode::subs::external_codec_id(&track.path) {
                Ok(codec) => {
                    if let StreamPlan::Drop(reason) = subtitle_plan(codec, settings.container) {
                        warnings.push(format!("{}: {reason}", track.path.display()));
                    }
                }
                Err(error) => warnings.push(error),
            }
        }
        if let Err(error) = crate::transcode::subs::validate_external(&settings.subtitles.external)
        {
            warnings.push(error);
        }
    }

    if let VideoSettings::Encode(encode) = &settings.video {
        let name = crate::transcode::video_encoder_name_for_detection(&encode.encoder);
        if encode.fast_decode && !encode.encoder.supports_fast_decode() {
            warnings.push(format!("Fast decode is not available for {name}"));
        }
        if !encode.advanced_options.trim().is_empty() && !encode.encoder.supports_advanced_options()
        {
            warnings.push(format!("Advanced options are not available for {name}"));
        }
        if encode.passes >= 2 {
            if !matches!(encode.rate, crate::transcode::RateControl::Bitrate(_)) {
                warnings.push("Two-pass encoding only applies to average bitrate".to_owned());
            } else if !encode.encoder.supports_multi_pass() {
                warnings.push(format!("Two-pass encoding is not available for {name}"));
            }
        }
        if encode.ten_bit == crate::transcode::TenBit::Twelve
            && encode.encoder != crate::transcode::VideoEncoder::X265
        {
            warnings.push("12-bit output is only available with x265".to_owned());
        }
        if encode.tone_map_to_sdr
            && encode.tone_map_operator == crate::transcode::ToneMapOperator::Bt2390
        {
            warnings.push(
                "The BT.2390 tone-map operator needs FFmpeg's libplacebo filter and a Vulkan device"
                    .to_owned(),
            );
        }
        if !encode.tone_map_to_sdr
            && encode.ten_bit == crate::transcode::TenBit::Off
            && metadata.video.as_ref().is_some_and(|video| video.hdr)
        {
            warnings.push("8-bit output of HDR video will show banding".to_owned());
        }
    }
    if settings.is_remux_only() {
        warnings.extend(crate::remux_edit::external_warnings(
            metadata,
            &settings.audio.languages,
            &settings.subtitles.languages,
            settings.subtitles.include,
            &settings.track_edits,
            settings.container,
        ));
    } else if !settings.track_edits.is_empty() {
        warnings.push(
            "Track edits apply to remux (stream-copy) jobs only and are ignored for transcode jobs"
                .to_owned(),
        );
    }
    warnings
}

fn normalize(codec_id: &str) -> String {
    codec_id.trim().to_ascii_lowercase()
}

fn is_image_subtitle(codec: &str) -> bool {
    matches!(
        codec,
        "hdmv_pgs_subtitle" | "pgssub" | "dvd_subtitle" | "vobsub" | "dvb_subtitle" | "xsub"
    )
}

fn is_text_subtitle(codec: &str) -> bool {
    matches!(
        codec,
        "subrip"
            | "srt"
            | "ass"
            | "ssa"
            | "webvtt"
            | "text"
            | "mov_text"
            | "microdvd"
            | "mpl2"
            | "pjs"
            | "realtext"
            | "sami"
            | "scc"
            | "stl"
            | "subviewer"
            | "subviewer1"
            | "vplayer"
            | "jacosub"
            | "aqtitle"
            | "eia_608"
            | "dvb_teletext"
            | "arib_caption"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::Container;
    use crate::mkv::{MediaMetadata, TrackInfo, VideoInfo};
    use crate::transcode::{AudioSettings, JobSettings, SubtitleSettings, VideoSettings};

    #[test]
    fn track_edits_on_a_transcode_job_warn_that_they_apply_to_remux_only() {
        let mut settings = JobSettings::default();
        settings.track_edits.order = vec![1];
        let metadata = MediaMetadata::default();
        assert!(
            !job_warnings(&metadata, &settings)
                .iter()
                .any(|warning| warning.contains("remux"))
        );
        settings.audio.codec = crate::transcode::AudioChoice::Aac;
        assert!(!settings.is_remux_only());
        assert!(
            job_warnings(&metadata, &settings)
                .iter()
                .any(|warning| warning.contains("remux (stream-copy) jobs only"))
        );
    }

    #[test]
    fn plans_video_copy_per_container() {
        assert_eq!(video_copy_plan("H264", Container::Mkv), StreamPlan::Copy);
        assert_eq!(video_copy_plan("vp9", Container::Mp4), StreamPlan::Copy);
        assert_eq!(
            video_copy_plan("mpeg2video", Container::Mp4),
            StreamPlan::Drop("Video codec mpeg2video needs re-encoding for MP4".to_owned())
        );
        assert_eq!(
            video_copy_plan("mpeg2video", Container::Mov),
            StreamPlan::Copy
        );
        assert_eq!(video_copy_plan("vp8", Container::WebM), StreamPlan::Copy);
        assert!(matches!(
            video_copy_plan("h264", Container::WebM),
            StreamPlan::Drop(_)
        ));
    }

    #[test]
    fn plans_audio_copy_or_container_appropriate_conversion() {
        assert_eq!(audio_copy_plan("aac", Container::Mp4), StreamPlan::Copy);
        assert_eq!(audio_copy_plan("opus", Container::WebM), StreamPlan::Copy);
        assert_eq!(
            audio_copy_plan("flac", Container::WebM),
            StreamPlan::Convert("libopus")
        );
        assert_eq!(
            audio_copy_plan("flac", Container::Mov),
            StreamPlan::Convert("aac")
        );
        assert_eq!(
            audio_copy_plan("pcm_s16le", Container::Mov),
            StreamPlan::Copy
        );
    }

    #[test]
    fn plans_text_and_image_subtitles() {
        assert_eq!(
            subtitle_plan("mov_text", Container::Mkv),
            StreamPlan::Convert("srt")
        );
        assert_eq!(subtitle_plan("subrip", Container::Mkv), StreamPlan::Copy);
        assert_eq!(
            subtitle_plan("hdmv_pgs_subtitle", Container::Mkv),
            StreamPlan::Copy
        );
        assert_eq!(subtitle_plan("webvtt", Container::WebM), StreamPlan::Copy);
        assert_eq!(
            subtitle_plan("microdvd", Container::Mkv),
            StreamPlan::Convert("srt")
        );
        assert_eq!(
            subtitle_plan("subrip", Container::Mp4),
            StreamPlan::Convert("mov_text")
        );
        assert_eq!(subtitle_plan("mov_text", Container::Mp4), StreamPlan::Copy);
        assert_eq!(
            subtitle_plan("ass", Container::WebM),
            StreamPlan::Convert("webvtt")
        );
        assert!(matches!(
            subtitle_plan("hdmv_pgs_subtitle", Container::Mp4),
            StreamPlan::Drop(_)
        ));
        assert_eq!(
            subtitle_plan("hdmv_pgs_subtitle", Container::Mkv),
            StreamPlan::Copy
        );
    }

    #[test]
    fn warns_for_audio_fallback_and_video_reencode() {
        let metadata = MediaMetadata {
            video: Some(VideoInfo {
                codec_id: "h264".to_owned(),
                ..VideoInfo::default()
            }),
            audio_tracks: vec![TrackInfo {
                language: "eng".to_owned(),
                codec_id: "aac".to_owned(),
                ..TrackInfo::default()
            }],
            ..MediaMetadata::default()
        };
        let settings = JobSettings {
            container: Container::WebM,
            video: VideoSettings::Copy,
            audio: AudioSettings {
                languages: vec!["jpn".to_owned()],
                ..AudioSettings::default()
            },
            subtitles: SubtitleSettings {
                include: false,
                ..SubtitleSettings::default()
            },
            ..JobSettings::default()
        };
        let warnings = job_warnings(&metadata, &settings);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("re-encoded"))
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("keeping all audio"))
        );
    }

    #[test]
    fn warns_when_an_unsupported_subtitle_is_dropped() {
        let metadata = MediaMetadata {
            subtitle_tracks: vec![TrackInfo {
                codec_id: "dvb_subtitle".to_owned(),
                language: "eng".to_owned(),
                ..TrackInfo::default()
            }],
            ..MediaMetadata::default()
        };
        let settings = JobSettings {
            container: Container::WebM,
            ..JobSettings::default()
        };
        let warnings = job_warnings(&metadata, &settings);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("Picture-based"))
        );

        let unknown = MediaMetadata {
            subtitle_tracks: vec![TrackInfo {
                codec_id: "unknown_text_format".to_owned(),
                language: "eng".to_owned(),
                ..TrackInfo::default()
            }],
            ..MediaMetadata::default()
        };
        let warnings = job_warnings(&unknown, &settings);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("not supported"))
        );
    }
}
