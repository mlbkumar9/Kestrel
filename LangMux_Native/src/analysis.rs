use crate::encoders::EncoderSet;
use crate::mkv::MediaMetadata;
use crate::tools;
use crate::transcode::{Crop, JobSettings, RateControl, VideoSettings, build_ffmpeg_job_arguments};
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_SAMPLE: AtomicUsize = AtomicUsize::new(0);

pub fn extract_frame(
    ffmpeg: &Path,
    input: &Path,
    at_seconds: f64,
    filter_chain: Option<&str>,
    max_width: u32,
) -> Result<Vec<u8>, String> {
    let mut filters = Vec::new();
    if let Some(filter_chain) = filter_chain.filter(|chain| !chain.trim().is_empty()) {
        filters.push(filter_chain.to_owned());
    }
    filters.push(format!("scale='min(iw,{max_width})':-2"));
    let filter = filters.join(",");
    let output = tools::command(ffmpeg)
        .args(["-ss", &at_seconds.max(0.0).to_string(), "-i"])
        .arg(input)
        .args([
            "-frames:v",
            "1",
            "-vf",
            &filter,
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "-",
        ])
        .output()
        .map_err(|error| format!("Failed to extract preview frame: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "FFmpeg frame extraction failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

pub fn detect_crop(ffmpeg: &Path, input: &Path, duration: f64) -> Option<Crop> {
    let mut detections = Vec::new();
    for percentage in [0.1, 0.3, 0.5, 0.7, 0.9] {
        let seek = (duration.max(0.0) * percentage).to_string();
        let output = tools::command(ffmpeg)
            .args(["-ss", &seek, "-i"])
            .arg(input)
            .args(["-vf", "cropdetect", "-frames:v", "1", "-f", "null", "-"])
            .output()
            .ok()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let dimensions = parse_dimensions(&stderr)?;
        if let Some((width, height, x, y)) = parse_crop(&stderr) {
            let (input_width, input_height) = dimensions;
            if width <= input_width && height <= input_height {
                detections.push(Crop::Custom {
                    top: y,
                    bottom: input_height - y - height,
                    left: x,
                    right: input_width - x - width,
                });
            }
        }
    }
    let best = detections
        .iter()
        .map(|crop| {
            (
                crop,
                detections
                    .iter()
                    .filter(|candidate| *candidate == crop)
                    .count(),
            )
        })
        .max_by_key(|(_, count)| *count)
        .map(|(crop, _)| crop.clone())?;
    (best != Crop::None).then_some(best)
}

pub fn estimate_size(
    ffmpeg: &Path,
    input: &Path,
    metadata: &MediaMetadata,
    settings: &JobSettings,
    encoders: &EncoderSet,
) -> Result<u64, String> {
    let duration = metadata
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or_else(|| "Media duration is unavailable for size estimation".to_owned())?;
    if let VideoSettings::Encode(encode) = &settings.video
        && let RateControl::Bitrate(video_kbps) = &encode.rate
    {
        let audio_kbps = if settings.audio.codec == crate::transcode::AudioChoice::Passthrough {
            0
        } else {
            settings.audio.bitrate_kbps
        };
        return Ok(estimate_from_bitrate(
            video_kbps.saturating_add(audio_kbps),
            duration,
        ));
    }
    if matches!(&settings.video, VideoSettings::Copy) {
        return std::fs::metadata(input)
            .map(|metadata| metadata.len())
            .map_err(|error| format!("Could not estimate copied output size: {error}"));
    }

    let extension = settings.container.extension();
    let mut sample_sizes = Vec::new();
    for percentage in [0.25, 0.5, 0.75] {
        let sample_file = reserve_sample_path(extension)?;
        let sample_start = duration * percentage;
        let arguments =
            build_ffmpeg_job_arguments(input, &sample_file.path, metadata, settings, encoders)?;
        let arguments = insert_sampling_options(arguments, sample_start);
        let output = tools::command(ffmpeg)
            .args(arguments)
            .output()
            .map_err(|error| format!("Failed to estimate output size: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "FFmpeg size sample failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let size = std::fs::metadata(&sample_file.path)
            .map_err(|error| format!("Could not read size sample: {error}"))?
            .len();
        sample_sizes.push(size);
    }
    let total_sample_bytes = sample_sizes.into_iter().sum::<u64>();
    Ok((total_sample_bytes as f64 * duration / 12.0).round() as u64)
}

fn estimate_from_bitrate(total_kbps: u32, duration: f64) -> u64 {
    (total_kbps as f64 * 1000.0 * duration / 8.0).round() as u64
}

struct SampleOutput {
    path: std::path::PathBuf,
}

impl Drop for SampleOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn reserve_sample_path(extension: &str) -> Result<SampleOutput, String> {
    for _ in 0..1000 {
        let id = NEXT_SAMPLE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "langmux-size-{}-{id}.{extension}",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                drop(file);
                return Ok(SampleOutput { path });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("Could not reserve size sample: {error}")),
        }
    }
    Err("Could not reserve a temporary size sample".to_owned())
}

fn insert_sampling_options(arguments: Vec<OsString>, sample_start: f64) -> Vec<OsString> {
    let mut result = Vec::with_capacity(arguments.len() + 4);
    let mut inserted_seek = false;
    let mut inserted_duration = false;
    for argument in arguments {
        if !inserted_seek && argument == "-i" {
            result.extend(["-ss".into(), sample_start.to_string().into()]);
            inserted_seek = true;
        }
        if !inserted_duration && argument == "-progress" {
            result.extend(["-t".into(), "4".into()]);
            inserted_duration = true;
        }
        result.push(argument);
    }
    result
}

fn parse_dimensions(output: &str) -> Option<(u32, u32)> {
    let re = regex::Regex::new(r"\b(\d{2,5})x(\d{2,5})\b").ok()?;
    re.captures(output)
        .and_then(|captures| Some((captures[1].parse().ok()?, captures[2].parse().ok()?)))
}

fn parse_crop(output: &str) -> Option<(u32, u32, u32, u32)> {
    let re = regex::Regex::new(r"crop=(\d+):(\d+):(\d+):(\d+)").ok()?;
    re.captures_iter(output).last().and_then(|captures| {
        Some((
            captures[1].parse().ok()?,
            captures[2].parse().ok()?,
            captures[3].parse().ok()?,
            captures[4].parse().ok()?,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{estimate_from_bitrate, parse_crop, parse_dimensions};

    #[test]
    fn estimates_sizes_from_total_bitrate() {
        assert_eq!(estimate_from_bitrate(1000, 8.0), 1_000_000);
    }

    #[test]
    fn parses_crop_detection_output() {
        let output =
            "Stream #0:0: Video: h264, 1920x1080\n[Parsed_cropdetect] crop=1880:1040:20:20";
        assert_eq!(parse_dimensions(output), Some((1920, 1080)));
        assert_eq!(parse_crop(output), Some((1880, 1040, 20, 20)));
    }
}
