//! Chapter probing and chapter file import/export.

use std::path::Path;

use kestrel_core::tools;
use kestrel_core::transcode::chapters;

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
                        original: name.clone(),
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

/// Chapter names as `number,name` lines, the format core reads back.
pub fn chapters_to_csv(chapters: &[ChapterRow]) -> String {
    let names: Vec<String> = chapters
        .iter()
        .map(|chapter| chapter.name.clone())
        .collect();
    chapters::chapters_to_csv(&names)
}

/// Chapter names from `number,name` lines.
pub fn chapter_names_from_csv(text: &str) -> Result<Vec<String>, String> {
    chapters::chapters_from_csv(text)
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
    fn timestamps_format_with_milliseconds() {
        assert_eq!(timestamp(724.12), "00:12:04.120");
    }

    #[test]
    fn csv_round_trip_keeps_names_including_commas() {
        let row = |name: &str| ChapterRow {
            start: 0.0,
            duration: 60.0,
            name: name.into(),
            original: name.into(),
        };
        let chapters = vec![row("Intro, part 1"), row("B")];
        let csv = chapters_to_csv(&chapters);
        assert_eq!(
            csv,
            "1,Intro, part 1
2,B
"
        );
        assert_eq!(
            chapter_names_from_csv(&csv).unwrap(),
            vec!["Intro, part 1".to_owned(), "B".to_owned()]
        );
        assert!(chapter_names_from_csv("2,Skipped one").is_err());
    }
}
