//! GPUI-free application state: view models, reducers and their tests.

pub mod activity;
pub mod format;
pub mod library;
pub mod persist;
pub mod probe;
pub mod queue;
pub mod remux;
pub mod storage;
pub mod transcode;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Page {
    #[default]
    Library,
    Remux,
    Transcode,
    Queue,
    Activity,
    Metrics,
    Settings,
}

impl Page {
    pub const ALL: [Self; 7] = [
        Self::Library,
        Self::Remux,
        Self::Transcode,
        Self::Queue,
        Self::Activity,
        Self::Metrics,
        Self::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::Remux => "Remux",
            Self::Transcode => "Transcode",
            Self::Queue => "Queue",
            Self::Activity => "Activity",
            Self::Metrics => "Metrics",
            Self::Settings => "Settings",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|page| page.label().eq_ignore_ascii_case(name.trim()))
    }
}

/// Settings page values that are not part of a job.
#[derive(Clone, Debug, PartialEq)]
pub struct AppSettings {
    pub parallel_jobs: usize,
    pub priority: usize,
    pub notify_when_done: bool,
    pub keep_source_dates: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            parallel_jobs: 0,
            priority: 0,
            notify_when_done: true,
            keep_source_dates: true,
        }
    }
}

pub const PRIORITIES: [&str; 3] = ["Normal", "Below normal", "Low"];
pub const PARALLEL_JOBS: [&str; 4] = ["1", "2", "3", "4"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_names_parse_case_insensitively() {
        assert_eq!(Page::from_name("transcode"), Some(Page::Transcode));
        assert_eq!(Page::from_name(" Metrics "), Some(Page::Metrics));
        assert_eq!(Page::from_name("nope"), None);
        assert_eq!(Page::ALL.len(), 7);
    }
}
