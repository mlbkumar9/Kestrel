//! Chapter names: CSV import/export and the temporary FFMETADATA file that renames chapters.

use crate::tools;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub title: String,
}

/// Formats chapter names as `n,name` lines (numbering starts at 1).
pub fn chapters_to_csv(names: &[String]) -> String {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| format!("{},{}\n", index + 1, name))
        .collect()
}

/// Parses `n,name` lines. Numbers must run 1, 2, 3 without gaps; the name is everything after
/// the first comma, so names may contain commas. Blank lines are skipped.
pub fn chapters_from_csv(text: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}').trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let (number, name) = line
            .split_once(',')
            .ok_or_else(|| format!("Line {}: expected `number,name`", line_number + 1))?;
        let number: usize = number.trim().parse().map_err(|_| {
            format!(
                "Line {}: `{}` is not a chapter number",
                line_number + 1,
                number
            )
        })?;
        if number != names.len() + 1 {
            return Err(format!(
                "Line {}: chapter number {number} should be {}",
                line_number + 1,
                names.len() + 1
            ));
        }
        names.push(name.trim().to_owned());
    }
    Ok(names)
}

pub fn parse_chapters_json(json: &str) -> Result<Vec<Chapter>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("Invalid ffprobe JSON: {error}"))?;
    let number = |value: &serde_json::Value| {
        value
            .as_str()
            .and_then(|text| text.parse::<f64>().ok())
            .or_else(|| value.as_f64())
    };
    Ok(value["chapters"]
        .as_array()
        .map(|chapters| {
            chapters
                .iter()
                .filter_map(|chapter| {
                    Some(Chapter {
                        start_seconds: number(&chapter["start_time"])?,
                        end_seconds: number(&chapter["end_time"])?,
                        title: chapter["tags"]["title"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

pub fn read_chapters(ffprobe: &Path, input: &Path) -> Result<Vec<Chapter>, String> {
    let output = tools::command(ffprobe)
        .args(["-v", "error", "-show_chapters", "-of", "json"])
        .arg(input)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Failed to read chapters: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "FFprobe chapter listing failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_chapters_json(&String::from_utf8_lossy(&output.stdout))
}

/// Source chapter titles as `n,name` CSV text, for the export button.
pub fn export_chapter_csv(ffprobe: &Path, input: &Path) -> Result<String, String> {
    let names = read_chapters(ffprobe, input)?
        .into_iter()
        .map(|chapter| chapter.title)
        .collect::<Vec<_>>();
    Ok(chapters_to_csv(&names))
}

fn escape_metadata(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '=' | ';' | '#' | '\\' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            '\n' | '\r' => escaped.push_str("\\\n"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// FFMETADATA1 text with the source chapter times and `names` replacing the titles.
/// Chapters beyond the list keep their source title.
pub fn ffmetadata(chapters: &[Chapter], names: &[String]) -> String {
    let mut text = String::from(";FFMETADATA1\n");
    for (index, chapter) in chapters.iter().enumerate() {
        let title = names
            .get(index)
            .filter(|name| !name.is_empty())
            .unwrap_or(&chapter.title);
        text.push_str(&format!(
            "[CHAPTER]\nTIMEBASE=1/1000\nSTART={}\nEND={}\ntitle={}\n",
            (chapter.start_seconds * 1000.0).round() as i64,
            (chapter.end_seconds * 1000.0).round() as i64,
            escape_metadata(title)
        ));
    }
    text
}

/// A temporary FFMETADATA file, removed when dropped.
pub struct PreparedChapters {
    path: PathBuf,
}

impl PreparedChapters {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PreparedChapters {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Writes the renamed-chapter metadata file for `input`.
pub fn prepare_chapter_file(
    ffprobe: &Path,
    input: &Path,
    names: &[String],
) -> Result<PreparedChapters, String> {
    let chapters = read_chapters(ffprobe, input)?;
    if chapters.is_empty() {
        return Err("The source has no chapters to rename".to_owned());
    }
    let text = ffmetadata(&chapters, names);
    for _ in 0..1000 {
        let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kestrel-chapters-{}-{id}.ffmeta",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write;
                let prepared = PreparedChapters { path };
                file.write_all(text.as_bytes())
                    .map_err(|error| format!("Could not write chapter file: {error}"))?;
                return Ok(prepared);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("Could not reserve chapter file: {error}")),
        }
    }
    Err("Could not reserve a temporary chapter file".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_round_trips_names_with_commas() {
        let names = vec!["Intro".to_owned(), "Fight, part 1".to_owned()];
        let csv = chapters_to_csv(&names);
        assert_eq!(csv, "1,Intro\n2,Fight, part 1\n");
        assert_eq!(chapters_from_csv(&csv).unwrap(), names);
    }

    #[test]
    fn csv_rejects_gaps_and_malformed_lines() {
        assert!(
            chapters_from_csv("1,A\n3,C\n")
                .unwrap_err()
                .contains("should be 2")
        );
        assert!(chapters_from_csv("A\n").is_err());
        assert!(chapters_from_csv("x,A\n").is_err());
        assert_eq!(chapters_from_csv("\n1,A\r\n\n2,B\n").unwrap(), ["A", "B"]);
    }

    #[test]
    fn ffmetadata_keeps_times_and_overrides_titles() {
        let chapters = parse_chapters_json(
            r#"{"chapters":[{"start_time":"0.000000","end_time":"1.500000","tags":{"title":"One"}},
                {"start_time":"1.5","end_time":"3.0","tags":{}}]}"#,
        )
        .unwrap();
        assert_eq!(chapters.len(), 2);
        let text = ffmetadata(&chapters, &["Opening=1".to_owned()]);
        assert!(text.starts_with(";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1500\n"));
        assert!(text.contains("title=Opening\\=1\n"));
        assert!(text.contains("START=1500\nEND=3000\ntitle=\n"));
    }
}
