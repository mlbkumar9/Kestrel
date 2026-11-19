//! Chapter probing and chapter file import/export.

use std::path::Path;

use kestrel_core::tools;

use super::format::format_clock;
use super::transcode::ChapterRow;

/// Reads chapters with `ffprobe -show_chapters`. Returns an empty list when there are none.
pub fn read_chapters(ffprobe: &Path, input: &Path) -> Vec<ChapterRow> {
    let output = tools::command(ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_chapters"])
        .arg(input)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            parse_chapters(&String::from_utf8_lossy(&output.stdout))
        }
        _ => Vec::new(),
    }
}

pub fn parse_chapters(json: &str) -> Vec<ChapterRow> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let seconds = |item: &serde_json::Value, key: &str| {
        item[key]
            .as_str()
            .and_then(|text| text.parse::<f64>().ok())
            .or_else(|| item[key].as_f64())
    };
    value["chapters"]
        .as_array()
        .map(|chapters| {
            chapters
                .iter()
                .enumerate()
                .filter_map(|(index, chapter)| {
                    let start = seconds(chapter, "start_time")?;
                    let end = seconds(chapter, "end_time").unwrap_or(start);
                    let name = chapter["tags"]["title"]
                        .as_str()
                        .map_or_else(|| format!("Chapter {}", index + 1), str::to_owned);
                    Some(ChapterRow {
                        start,
                        duration: (end - start).max(0.0),
                        name,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `hh:mm:ss.mmm`
pub fn timestamp(seconds: f64) -> String {
    let millis = ((seconds.max(0.0) - seconds.max(0.0).floor()) * 1000.0).round() as u32;
    format!("{}.{:03}", format_clock(seconds.floor()), millis.min(999))
}

pub fn parse_timestamp(text: &str) -> Option<f64> {
    let mut seconds = 0.0;
    for part in text.trim().split(':') {
        seconds = seconds * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(seconds)
}

/// CSV with `start,name` per line.
pub fn chapters_to_csv(chapters: &[ChapterRow]) -> String {
    chapters
        .iter()
        .map(|chapter| {
            format!(
                "{},{}",
                timestamp(chapter.start),
                chapter.name.replace(',', " ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parses `start,name` lines. Lines that do not parse are skipped.
pub fn chapters_from_csv(text: &str, total: Option<f64>) -> Vec<ChapterRow> {
    let mut chapters: Vec<ChapterRow> = text
        .lines()
        .filter_map(|line| {
            let (start, name) = line.split_once(',')?;
            Some(ChapterRow {
                start: parse_timestamp(start)?,
                duration: 0.0,
                name: name.trim().to_owned(),
            })
        })
        .collect();
    for index in 0..chapters.len() {
        let end = chapters
            .get(index + 1)
            .map(|next| next.start)
            .or(total)
            .unwrap_or(chapters[index].start);
        chapters[index].duration = (end - chapters[index].start).max(0.0);
    }
    chapters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffprobe_chapters() {
        let json = r#"{"chapters":[
            {"start_time":"0.000000","end_time":"724.120000","tags":{"title":"Opening"}},
            {"start_time":"724.120000","end_time":"2021.900000"}]}"#;
        let chapters = parse_chapters(json);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].name, "Opening");
        assert_eq!(chapters[1].name, "Chapter 2");
        assert!((chapters[1].duration - 1297.78).abs() < 0.01);
    }

    #[test]
    fn timestamps_round_trip() {
        assert_eq!(timestamp(724.12), "00:12:04.120");
        assert!((parse_timestamp("00:12:04.120").unwrap() - 724.12).abs() < 1e-6);
        assert_eq!(parse_timestamp("nope"), None);
    }

    #[test]
    fn csv_round_trip_keeps_names_and_starts() {
        let chapters = vec![
            ChapterRow {
                start: 0.0,
                duration: 60.0,
                name: "A".into(),
            },
            ChapterRow {
                start: 60.0,
                duration: 30.0,
                name: "B".into(),
            },
        ];
        let csv = chapters_to_csv(&chapters);
        let back = chapters_from_csv(&csv, Some(90.0));
        assert_eq!(back, chapters);
    }
}
