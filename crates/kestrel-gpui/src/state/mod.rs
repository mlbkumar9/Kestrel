//! GPUI-free application state: view models, reducers and their tests.

pub mod activity;
pub mod format;
pub mod library;
pub mod nav;
pub mod persist;
pub mod presets;
pub mod preview;
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

impl AppSettings {
    /// Priority every tool process started from now on runs at.
    pub fn tool_priority(&self) -> kestrel_core::tools::Priority {
        kestrel_core::tools::Priority::from_index(self.priority)
    }

    /// Makes the stored priority the one `kestrel_core::tools::command` applies.
    pub fn apply_priority(&self) {
        kestrel_core::tools::set_priority(self.tool_priority());
    }

    /// Jobs the queue may run at once (the select stores the index of "1".."4").
    pub fn parallel_job_count(&self) -> usize {
        self.parallel_jobs + 1
    }
}

pub const PRIORITIES: [&str; 3] = ["Normal", "Below normal", "Low"];
pub const PARALLEL_JOBS: [&str; 4] = ["1", "2", "3", "4"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_settings_map_to_the_values_the_worker_uses() {
        use kestrel_core::tools::{Priority, priority};
        let mut settings = AppSettings::default();
        assert_eq!(settings.parallel_job_count(), 1);
        settings.parallel_jobs = 3;
        assert_eq!(settings.parallel_job_count(), 4);
        assert_eq!(PARALLEL_JOBS[settings.parallel_jobs], "4");
        assert_eq!(settings.tool_priority(), Priority::Normal);
        settings.priority = 2;
        settings.apply_priority();
        assert_eq!(priority(), Priority::Low);
        settings.priority = 1;
        settings.apply_priority();
        assert_eq!(priority(), Priority::BelowNormal);
        settings.priority = 0;
        settings.apply_priority();
        assert_eq!(priority(), Priority::Normal);
        assert_eq!(PRIORITIES.len(), 3);
    }

    #[test]
    fn page_names_parse_case_insensitively() {
        assert_eq!(Page::from_name("transcode"), Some(Page::Transcode));
        assert_eq!(Page::from_name(" Metrics "), Some(Page::Metrics));
        assert_eq!(Page::from_name("nope"), None);
        assert_eq!(Page::ALL.len(), 7);
    }
}
