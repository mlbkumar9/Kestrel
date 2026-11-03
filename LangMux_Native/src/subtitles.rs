use crate::tools;
use std::path::{Path, PathBuf};
use std::process::Stdio;

pub struct PreparedTextSubtitle {
    path: PathBuf,
    filename: String,
}

impl PreparedTextSubtitle {
    pub fn filename(&self) -> &str {
        &self.filename
    }
}

impl Drop for PreparedTextSubtitle {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn prepare_text_subtitle(
    ffmpeg: &Path,
    input: &Path,
    ffprobe_index: u64,
) -> Result<PreparedTextSubtitle, String> {
    let directory = std::env::temp_dir();
    for index in 0..1000 {
        let filename = format!("langmux-sub-{}-{index}.ass", std::process::id());
        let path = directory.join(&filename);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                drop(file);
                let output = tools::command(ffmpeg)
                    .args(["-nostdin", "-y", "-i"])
                    .arg(input)
                    .args(["-map", &format!("0:{ffprobe_index}"), "-c:s", "ass"])
                    .arg(&path)
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|error| {
                        let _ = std::fs::remove_file(&path);
                        format!("Failed to prepare subtitle file: {error}")
                    })?;
                if !output.status.success() {
                    let _ = std::fs::remove_file(&path);
                    return Err(format!(
                        "FFmpeg subtitle extraction failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
                return Ok(PreparedTextSubtitle { path, filename });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("Could not reserve subtitle file: {error}")),
        }
    }
    Err("Could not reserve a temporary subtitle file".to_owned())
}

pub fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| format!("Could not resolve relative path: {error}"))
    }
}
