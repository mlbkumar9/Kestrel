use crate::tools;
use crate::transcode::{HwCodec, HwVendor, VideoEncoder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct EncoderSet {
    pub software: Vec<VideoEncoder>,
    pub hardware: Vec<VideoEncoder>,
    /// FFmpeg names of the audio encoders the build lists (for example `libfdk_aac`).
    pub audio: Vec<String>,
}

impl EncoderSet {
    /// Keeps a supported encoder or chooses an available encoder in the same codec family.
    /// Software alternatives are preferred to hardware alternatives.
    pub fn resolve(&self, requested: &VideoEncoder) -> Result<VideoEncoder, String> {
        if self.supports(requested) {
            return Ok(requested.clone());
        }
        self.software
            .iter()
            .chain(&self.hardware)
            .find(|candidate| candidate.codec_family() == requested.codec_family())
            .cloned()
            .ok_or_else(|| {
                format!(
                    "Video encoder {} is unavailable; no available {} encoder can replace it",
                    crate::transcode::video_encoder_name_for_detection(requested),
                    requested.codec_family()
                )
            })
    }

    pub fn supports(&self, encoder: &VideoEncoder) -> bool {
        match encoder {
            VideoEncoder::Hardware { .. } => self.hardware.contains(encoder),
            _ => self.software.contains(encoder),
        }
    }

    /// Whether FFmpeg lists the audio encoder (`libfdk_aac`, `libopus`, ...).
    pub fn has_audio_encoder(&self, name: &str) -> bool {
        self.audio.iter().any(|encoder| encoder == name)
    }
}

/// How long `ffmpeg -encoders` may run before detection gives up.
const ENCODER_LIST_TIMEOUT: Duration = Duration::from_secs(15);

pub fn detect(ffmpeg: &Path) -> Result<EncoderSet, String> {
    detect_with_timeout(ffmpeg, ENCODER_LIST_TIMEOUT)
}

fn detect_with_timeout(ffmpeg: &Path, timeout: Duration) -> Result<EncoderSet, String> {
    let mut command = tools::command(ffmpeg);
    command.args(["-hide_banner", "-encoders"]);
    let output = output_with_timeout(command, timeout)
        .map_err(|error| format!("Failed to list FFmpeg encoders: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "FFmpeg encoder listing failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let combined_output = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let names = parse_encoder_list(&combined_output);
    let audio = parse_audio_encoder_list(&combined_output)
        .into_iter()
        .collect();
    let software = [
        ("libx264", VideoEncoder::X264),
        ("libx265", VideoEncoder::X265),
        ("libsvtav1", VideoEncoder::SvtAv1),
        ("libaom-av1", VideoEncoder::AomAv1),
        ("libvpx-vp9", VideoEncoder::Vp9),
    ]
    .into_iter()
    .filter_map(|(name, encoder)| names.contains(name).then_some(encoder))
    .collect();

    let hardware_candidates = [
        (HwVendor::Nvenc, HwCodec::H264),
        (HwVendor::Nvenc, HwCodec::Hevc),
        (HwVendor::Nvenc, HwCodec::Av1),
        (HwVendor::Qsv, HwCodec::H264),
        (HwVendor::Qsv, HwCodec::Hevc),
        (HwVendor::Qsv, HwCodec::Av1),
        (HwVendor::Amf, HwCodec::H264),
        (HwVendor::Amf, HwCodec::Hevc),
        (HwVendor::Amf, HwCodec::Av1),
        (HwVendor::VideoToolbox, HwCodec::H264),
        (HwVendor::VideoToolbox, HwCodec::Hevc),
        (HwVendor::VideoToolbox, HwCodec::Av1),
        (HwVendor::Vaapi, HwCodec::H264),
        (HwVendor::Vaapi, HwCodec::Hevc),
        (HwVendor::Vaapi, HwCodec::Av1),
    ];
    let mut hardware = Vec::new();
    for (vendor, codec) in hardware_candidates {
        let encoder = VideoEncoder::Hardware { vendor, codec };
        let name = crate::transcode::video_encoder_name_for_detection(&encoder);
        if names.contains(name) && probe_hardware_encoder(ffmpeg, name) {
            hardware.push(encoder);
        }
    }

    Ok(EncoderSet {
        software,
        hardware,
        audio,
    })
}

/// Runs `command` to completion, killing it when it exceeds `timeout`.
fn output_with_timeout(
    mut command: std::process::Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::io::Read;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let drain = |stream: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut stream) = stream {
                let _ = stream.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = drain(child.stdout.take().map(|s| Box::new(s) as _));
    let stderr = drain(child.stderr.take().map(|s| Box::new(s) as _));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} s", timeout.as_secs_f32()));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    };
    Ok(std::process::Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

pub fn detect_async(ffmpeg: PathBuf) -> mpsc::Receiver<Result<EncoderSet, String>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(detect(&ffmpeg));
    });
    receiver
}

/// Names of the video encoders in `ffmpeg -encoders` output.
pub fn parse_encoder_list(output: &str) -> BTreeSet<String> {
    parse_encoder_names(output, 'V')
}

/// Names of the audio encoders in `ffmpeg -encoders` output.
pub fn parse_audio_encoder_list(output: &str) -> BTreeSet<String> {
    parse_encoder_names(output, 'A')
}

fn parse_encoder_names(output: &str, media: char) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let flags = fields.next()?;
            let name = fields.next()?;
            (flags.len() == 6
                && flags.starts_with(media)
                && flags
                    .chars()
                    .all(|flag| flag == '.' || flag.is_ascii_alphabetic()))
            .then(|| name.to_owned())
        })
        .collect()
}

fn probe_hardware_encoder(ffmpeg: &Path, encoder: &str) -> bool {
    let mut child = match tools::command(ffmpeg)
        .args([
            "-hide_banner",
            "-f",
            "lavfi",
            "-i",
            "color=black:s=256x256:d=0.1",
            "-frames:v",
            "1",
            "-c:v",
            encoder,
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    wait_for_encoder_probe(&mut child, Duration::from_secs(10))
}

fn wait_for_encoder_probe(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hangs for a minute when run by name; `encoder_listing_times_out` starts it as a child.
    #[test]
    #[ignore = "helper process for encoder_listing_times_out"]
    fn hang_for_encoder_timeout_test() {
        std::thread::sleep(Duration::from_secs(60));
    }

    #[test]
    fn encoder_listing_times_out_instead_of_hanging() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--ignored",
            "--exact",
            "encoders::tests::hang_for_encoder_timeout_test",
        ]);
        let started = Instant::now();
        let error = output_with_timeout(command, Duration::from_millis(700)).unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn output_with_timeout_returns_output_of_a_finishing_process() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--list", "--format", "terse"]);
        let output = output_with_timeout(command, Duration::from_secs(30)).unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("parses_ffmpeg_encoder"));
    }

    #[test]
    fn parses_ffmpeg_encoder_listing_flags() {
        let captured = r#"
Encoders:
 V..... libx264              H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10
 V..... libx265              H.265 / HEVC
 V..... h264_nvenc           NVIDIA NVENC H.264 encoder
 V..... libsvtav1            SVT-AV1(Scalable Video Technology for AV1) encoder
 A..... aac                  AAC (Advanced Audio Coding)
 V..... libvpx-vp9           libvpx VP9
"#;
        let parsed = parse_encoder_list(captured);
        assert!(parsed.contains("libx264"));
        assert!(parsed.contains("h264_nvenc"));
        assert!(parsed.contains("libsvtav1"));
        assert!(parsed.contains("libvpx-vp9"));
        assert!(!parsed.contains("aac"));
    }

    /// `ffmpeg -hide_banner -encoders` (FFmpeg 9.0.1, Windows build), trimmed to the lines the
    /// detector looks at. This machine has an Intel iGPU and no NVIDIA driver, yet FFmpeg lists
    /// every NVENC encoder: a listing says what the build contains, not what the host can run.
    const CAPTURED_ENCODERS: &str = r#"Encoders:
 V..... = Video
 A..... = Audio
 S..... = Subtitle
 .F.... = Frame-level multithreading
 ..S... = Slice-level multithreading
 ...X.. = Codec is experimental
 ....B. = Supports draw_horiz_band
 .....D = Supports direct rendering method 1
 ------
 V....D libaom-av1           libaom AV1 (codec av1)
 V..... libsvtav1            SVT-AV1(Scalable Video Technology for AV1) encoder (codec av1)
 V....D av1_nvenc            NVIDIA NVENC av1 encoder (codec av1)
 V..... av1_qsv              AV1 (Intel Quick Sync Video acceleration) (codec av1)
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)
 V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)
 V..... h264_qsv             H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (Intel Quick Sync Video acceleration) (codec h264)
 V....D libx265              libx265 H.265 / HEVC (codec hevc)
 V....D hevc_nvenc           NVIDIA NVENC hevc encoder (codec hevc)
 V..... hevc_qsv             HEVC (Intel Quick Sync Video acceleration) (codec hevc)
 V....D libvpx-vp9           libvpx VP9 (codec vp9)
 A....D aac                  AAC (Advanced Audio Coding)
 A....D libopus              libopus Opus (codec opus)
"#;

    #[test]
    fn captured_listing_shows_libx265_and_the_nvenc_names() {
        let video = parse_encoder_list(CAPTURED_ENCODERS);
        for name in [
            "libx264",
            "libx265",
            "libsvtav1",
            "libaom-av1",
            "libvpx-vp9",
            "h264_nvenc",
            "hevc_nvenc",
            "av1_nvenc",
            "h264_qsv",
            "hevc_qsv",
        ] {
            assert!(video.contains(name), "{name} missing from {video:?}");
        }
        assert!(
            !video.contains("aac"),
            "audio encoders stay out of the video list"
        );
        let audio = parse_audio_encoder_list(CAPTURED_ENCODERS);
        assert!(audio.contains("aac") && audio.contains("libopus"));
        assert!(!audio.contains("libx265"));
    }

    /// Regression: the GPUI queue used `EncoderSet::default()` while detection was still
    /// running, which makes every transcode fail with "libx265 is unavailable" although the
    /// listing above contains libx265. A detected set resolves it; the empty set cannot.
    #[test]
    fn libx265_resolves_from_a_detected_set_but_not_from_the_empty_one() {
        let detected = EncoderSet {
            software: vec![VideoEncoder::X264, VideoEncoder::X265, VideoEncoder::SvtAv1],
            hardware: vec![
                VideoEncoder::Hardware {
                    vendor: HwVendor::Qsv,
                    codec: HwCodec::H264,
                },
                VideoEncoder::Hardware {
                    vendor: HwVendor::Qsv,
                    codec: HwCodec::Hevc,
                },
            ],
            ..EncoderSet::default()
        };
        assert_eq!(
            detected.resolve(&VideoEncoder::X265),
            Ok(VideoEncoder::X265)
        );
        assert_eq!(
            EncoderSet::default()
                .resolve(&VideoEncoder::X265)
                .unwrap_err(),
            "Video encoder libx265 is unavailable; no available HEVC encoder can replace it"
        );
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    #[test]
    fn fallback_keeps_available_choice_and_prefers_same_family_software() {
        let hardware = VideoEncoder::Hardware {
            vendor: HwVendor::Nvenc,
            codec: HwCodec::Av1,
        };
        let encoders = EncoderSet {
            software: vec![VideoEncoder::X264, VideoEncoder::AomAv1],
            hardware: vec![hardware.clone()],
            ..EncoderSet::default()
        };
        assert_eq!(
            encoders.resolve(&VideoEncoder::SvtAv1),
            Ok(VideoEncoder::AomAv1)
        );
        assert_eq!(encoders.resolve(&hardware), Ok(hardware.clone()));
        assert_eq!(
            EncoderSet {
                software: vec![],
                hardware: vec![hardware.clone()],
                ..EncoderSet::default()
            }
            .resolve(&VideoEncoder::SvtAv1),
            Ok(hardware)
        );
    }
    #[test]
    fn fallback_rejects_other_codec_families_and_names_missing_encoder() {
        let encoders = EncoderSet {
            software: vec![VideoEncoder::X264],
            hardware: vec![],
            ..EncoderSet::default()
        };
        assert_eq!(
            encoders.resolve(&VideoEncoder::SvtAv1).unwrap_err(),
            "Video encoder libsvtav1 is unavailable; no available AV1 encoder can replace it"
        );
        assert!(EncoderSet::default().resolve(&VideoEncoder::X264).is_err());
    }
}
