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
}

impl EncoderSet {
    pub fn supports(&self, encoder: &VideoEncoder) -> bool {
        match encoder {
            VideoEncoder::Hardware { .. } => self.hardware.contains(encoder),
            _ => self.software.contains(encoder),
        }
    }
}

pub fn detect(ffmpeg: &Path) -> Result<EncoderSet, String> {
    let output = tools::command(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
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
    let software = [
        ("libx264", VideoEncoder::X264),
        ("libx265", VideoEncoder::X265),
        ("libsvtav1", VideoEncoder::SvtAv1),
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

    Ok(EncoderSet { software, hardware })
}

pub fn detect_async(ffmpeg: PathBuf) -> mpsc::Receiver<Result<EncoderSet, String>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(detect(&ffmpeg));
    });
    receiver
}

pub fn parse_encoder_list(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let flags = fields.next()?;
            let name = fields.next()?;
            (flags.len() == 6
                && flags.starts_with('V')
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
}
