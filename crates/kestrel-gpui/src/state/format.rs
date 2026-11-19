//! Small display helpers shared by pages and view models.

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// `h:mm:ss`, or `-` when the duration is unknown.
pub fn format_duration(seconds: Option<f64>) -> String {
    let Some(seconds) = seconds.filter(|value| value.is_finite() && *value >= 0.0) else {
        return "-".to_owned();
    };
    let total = seconds.round() as u64;
    format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
}

/// `hh:mm:ss` clock used by ETA and elapsed readouts.
pub fn format_clock(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds.round() as u64
    } else {
        0
    };
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        total / 60 % 60,
        total % 60
    )
}

/// Readable codec name from an FFmpeg or MKVToolNix codec string.
pub fn codec_label(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let known = [
        ("hevc", "HEVC"),
        ("h.265", "HEVC"),
        ("h265", "HEVC"),
        ("avc", "H.264"),
        ("h264", "H.264"),
        ("h.264", "H.264"),
        ("av1", "AV1"),
        ("vp9", "VP9"),
        ("vp8", "VP8"),
        ("mpeg-4", "MPEG-4"),
        ("mpeg4", "MPEG-4"),
        ("mpeg-2", "MPEG-2"),
        ("mpeg2", "MPEG-2"),
        ("truehd", "TrueHD"),
        ("dts-hd", "DTS-HD"),
        ("dts", "DTS"),
        ("eac3", "E-AC3"),
        ("e-ac-3", "E-AC3"),
        ("ac3", "AC3"),
        ("ac-3", "AC3"),
        ("aac", "AAC"),
        ("opus", "Opus"),
        ("flac", "FLAC"),
        ("vorbis", "Vorbis"),
        ("mp3", "MP3"),
        ("pcm", "PCM"),
        ("subrip", "SRT"),
        ("srt", "SRT"),
        ("ass", "ASS"),
        ("ssa", "SSA"),
        ("webvtt", "VTT"),
        ("vtt", "VTT"),
        ("pgs", "PGS"),
        ("hdmv", "PGS"),
        ("dvd_subtitle", "VobSub"),
        ("vobsub", "VobSub"),
        ("mov_text", "mov_text"),
    ];
    for (needle, label) in known {
        if lower.contains(needle) {
            return label.to_owned();
        }
    }
    raw.to_owned()
}

/// Version number from the first line a tool prints, such as `7.1.1` or `95.0`.
pub fn tool_version(raw: &str) -> String {
    raw.split_whitespace()
        .map(|token| token.trim_start_matches('v'))
        .find(|token| token.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
        .map(|token| {
            token
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
                .collect::<String>()
        })
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "git".to_owned())
}

pub fn channel_label(channels: Option<u32>) -> String {
    match channels {
        Some(1) => "1.0".to_owned(),
        Some(2) => "2.0".to_owned(),
        Some(6) => "5.1".to_owned(),
        Some(8) => "7.1".to_owned(),
        Some(count) => format!("{count}ch"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1_500_000), "1.5 MB");
        assert_eq!(format_size(18_400_000_000), "18.4 GB");
        assert_eq!(format_size(250_000_000_000), "250 GB");
    }

    #[test]
    fn durations_and_clocks() {
        assert_eq!(format_duration(Some(7900.4)), "2:11:40");
        assert_eq!(format_duration(None), "-");
        assert_eq!(format_clock(862.0), "00:14:22");
        assert_eq!(format_clock(f64::NAN), "00:00:00");
    }

    #[test]
    fn codec_names_are_normalised() {
        assert_eq!(codec_label("hevc"), "HEVC");
        assert_eq!(codec_label("HEVC/H.265/MPEG-H"), "HEVC");
        assert_eq!(codec_label("SubRip/SRT"), "SRT");
        assert_eq!(codec_label("weird"), "weird");
        assert_eq!(channel_label(Some(6)), "5.1");
        assert_eq!(
            tool_version("ffmpeg version 7.1.1-essentials_build"),
            "7.1.1"
        );
        assert_eq!(tool_version("mkvmerge v95.0 ('Fumbling') 64-bit"), "95.0");
        assert_eq!(tool_version("ffmpeg version N-1234-gabc"), "git");
    }
}
