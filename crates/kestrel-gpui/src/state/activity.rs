//! Activity log lines with time, level and message.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Info,
    Cmd,
    Done,
    Warn,
    Error,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO ",
            Self::Cmd => "CMD  ",
            Self::Done => "DONE ",
            Self::Warn => "WARN ",
            Self::Error => "ERROR",
        }
    }

    /// Guesses a level for a line of worker output.
    pub fn classify(text: &str) -> Self {
        let lower = text.to_ascii_lowercase();
        if lower.starts_with("ffmpeg ") || lower.starts_with("mkvmerge ") || lower.starts_with("\"")
        {
            Self::Cmd
        } else if lower.contains("error") || lower.contains("failed") {
            Self::Error
        } else if lower.contains("warn") || lower.contains("falling back") {
            Self::Warn
        } else if lower.contains("finished")
            || lower.contains("complete")
            || lower.starts_with("done")
        {
            Self::Done
        } else {
            Self::Info
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub time: String,
    pub level: Level,
    pub message: String,
    /// File name of the job this line belongs to, when known.
    pub job: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ActivityLog {
    pub lines: Vec<LogLine>,
    /// Job name the view is filtered to; `None` shows everything.
    pub filter: Option<String>,
}

const MAX_LINES: usize = 5000;

impl ActivityLog {
    pub fn push(&mut self, level: Level, message: impl Into<String>, job: Option<String>) {
        self.lines.push(LogLine {
            time: clock_time(),
            level,
            message: message.into(),
            job,
        });
        if self.lines.len() > MAX_LINES {
            self.lines.drain(..self.lines.len() - MAX_LINES);
        }
    }

    pub fn push_classified(&mut self, message: impl Into<String>, job: Option<String>) {
        let message = message.into();
        let level = Level::classify(&message);
        self.push(level, message, job);
    }

    pub fn visible(&self) -> Vec<&LogLine> {
        self.lines
            .iter()
            .filter(|line| {
                self.filter
                    .as_ref()
                    .is_none_or(|name| line.job.as_ref() == Some(name))
            })
            .collect()
    }

    /// Names of jobs that have log lines, in first-seen order.
    pub fn jobs(&self) -> Vec<String> {
        let mut jobs = Vec::new();
        for line in &self.lines {
            if let Some(job) = &line.job
                && !jobs.contains(job)
            {
                jobs.push(job.clone());
            }
        }
        jobs
    }

    pub fn as_text(&self) -> String {
        self.visible()
            .iter()
            .map(|line| format!("{}  {}  {}", line.time, line.level.label(), line.message))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.filter = None;
    }
}

#[cfg(windows)]
fn local_hms() -> (u32, u32, u32) {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    unsafe extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut time = SystemTime::default();
    // SAFETY: GetLocalTime only writes a SYSTEMTIME through the valid pointer it is given.
    unsafe { GetLocalTime(&mut time) };
    (
        u32::from(time.hour),
        u32::from(time.minute),
        u32::from(time.second),
    )
}

#[cfg(not(windows))]
fn local_hms() -> (u32, u32, u32) {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() % 86_400) as u32;
    (seconds / 3600, seconds / 60 % 60, seconds % 60)
}

pub fn clock_time() -> String {
    let (hour, minute, second) = local_hms();
    format!("{hour:02}:{minute:02}:{second:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_guessed_from_text() {
        assert_eq!(Level::classify("ffmpeg -i a.mkv out.mkv"), Level::Cmd);
        assert_eq!(Level::classify("a.mkv: NVENC failed"), Level::Error);
        assert_eq!(Level::classify("DTS-HD: falling back to AAC"), Level::Warn);
        assert_eq!(Level::classify("Queue started"), Level::Info);
    }

    #[test]
    fn filter_limits_visible_lines_and_export() {
        let mut log = ActivityLog::default();
        log.push(Level::Info, "one", Some("a.mkv".to_owned()));
        log.push(Level::Info, "two", Some("b.mkv".to_owned()));
        assert_eq!(log.jobs(), vec!["a.mkv".to_owned(), "b.mkv".to_owned()]);
        log.filter = Some("b.mkv".to_owned());
        assert_eq!(log.visible().len(), 1);
        assert!(log.as_text().contains("two") && !log.as_text().contains("one"));
        log.clear();
        assert!(log.lines.is_empty());
    }

    #[test]
    fn clock_has_hh_mm_ss_shape() {
        let clock = clock_time();
        assert_eq!(clock.len(), 8);
        assert_eq!(clock.matches(':').count(), 2);
    }
}
