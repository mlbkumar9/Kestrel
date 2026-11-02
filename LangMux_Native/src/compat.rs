use crate::formats::Container;
use crate::mkv::{self, MediaMetadata, TrackInfo};
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

    let selected_audio = selected_tracks(&metadata.audio_tracks, &settings.audio.languages, true);
    let audio_tracks = if selected_audio.is_empty() && !metadata.audio_tracks.is_empty() {
        metadata.audio_tracks.iter().collect::<Vec<_>>()
    } else {
        selected_audio
    };
    let requested_languages = mkv::normalize_languages(&settings.audio.languages);
    if !settings.audio.languages.is_empty()
        && !metadata.audio_tracks.is_empty()
        && metadata
            .audio_tracks
            .iter()
            .all(|track| !requested_languages.contains(&track.language.to_ascii_lowercase()))
    {
        warnings.push("No matching audio languages found; keeping all audio tracks".to_owned());
    }
    for track in audio_tracks {
        if let StreamPlan::Drop(reason) = audio_copy_plan(&track.codec_id, settings.container) {
            warnings.push(reason);
        }
    }

    if settings.subtitles.include {
        for track in selected_tracks(
            &metadata.subtitle_tracks,
            &settings.subtitles.languages,
            true,
        ) {
            if let StreamPlan::Drop(reason) = subtitle_plan(&track.codec_id, settings.container) {
                warnings.push(reason);
            }
        }
    }
    warnings
}

fn selected_tracks<'a>(
    tracks: &'a [TrackInfo],
    languages: &[String],
    empty_means_all: bool,
) -> Vec<&'a TrackInfo> {
    if languages.is_empty() && empty_means_all {
        return tracks.iter().collect();
    }
    tracks
        .iter()
        .filter(|track| {
            languages
                .iter()
                .any(|language| language.trim().eq_ignore_ascii_case(&track.language))
        })
        .collect()
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
