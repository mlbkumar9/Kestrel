//! View model of the Transcode preview: where the segment starts, the run in flight, and the
//! numbers shown once it finishes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kestrel_core::preview::{
    LENGTHS_SECONDS, PreviewControl, PreviewError, PreviewOutcome, plan_segment,
};
use kestrel_core::transcode::Segment;

use super::format::{format_clock, format_size};

/// Quick positions for the segment start, as a fraction of the source.
pub const POSITIONS: [(&str, f64); 3] = [("25%", 0.25), ("50%", 0.5), ("75%", 0.75)];

pub enum PreviewPhase {
    Idle,
    Running { percent: u32 },
    Done(Box<PreviewOutcome>),
    Failed(String),
}

pub struct PreviewState {
    /// Segment start as a fraction of the source duration.
    pub position: f64,
    pub phase: PreviewPhase,
    /// The source the running or finished preview belongs to.
    pub source: Option<PathBuf>,
    /// Bumped by every start and every clear, so a late result of an old run is recognised.
    run: u64,
    control: Option<Arc<PreviewControl>>,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            position: 0.5,
            phase: PreviewPhase::Idle,
            source: None,
            run: 0,
            control: None,
        }
    }
}

impl Drop for PreviewState {
    /// Closing the window stops a running encode and deletes the finished one's files.
    fn drop(&mut self) {
        self.clear();
    }
}

/// What the finished preview shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewSummary {
    pub size: String,
    pub bitrate: String,
    pub full_size: String,
    pub range: String,
}

impl PreviewState {
    pub fn is_running(&self) -> bool {
        matches!(self.phase, PreviewPhase::Running { .. })
    }

    /// Selected length in seconds for a `PREVIEW_LENGTHS` index.
    pub fn length_seconds(index: usize) -> f64 {
        LENGTHS_SECONDS[index.min(LENGTHS_SECONDS.len() - 1)]
    }

    /// The segment a start would encode.
    pub fn segment(&self, duration: Option<f64>, length_index: usize) -> Option<Segment> {
        plan_segment(duration?, Self::length_seconds(length_index), self.position)
    }

    /// Index of the quick position that matches the current start, if any.
    pub fn position_index(&self) -> Option<usize> {
        POSITIONS
            .iter()
            .position(|(_, value)| (value - self.position).abs() < 0.005)
    }

    /// Starts a run for `source`: any earlier result is discarded. Returns the run number and
    /// the handle that cancels it.
    pub fn begin(&mut self, source: PathBuf) -> (u64, Arc<PreviewControl>) {
        self.clear();
        self.run += 1;
        let control = Arc::new(PreviewControl::default());
        self.control = Some(control.clone());
        self.source = Some(source);
        self.phase = PreviewPhase::Running { percent: 0 };
        (self.run, control)
    }

    pub fn progress(&mut self, run: u64, percent: u32) {
        if run == self.run && self.is_running() {
            self.phase = PreviewPhase::Running {
                percent: percent.min(100),
            };
        }
    }

    /// Records the end of a run. Returns the message to show, if any. A result of a run that
    /// was replaced or cleared meanwhile is deleted and ignored.
    pub fn finish(
        &mut self,
        run: u64,
        result: Result<PreviewOutcome, PreviewError>,
    ) -> Option<String> {
        if run != self.run || !self.is_running() {
            if let Ok(outcome) = result {
                outcome.discard();
            }
            return None;
        }
        self.control = None;
        match result {
            Ok(outcome) => {
                let note = outcome
                    .still_error
                    .as_ref()
                    .map(|error| format!("Preview ready, but the stills failed ({error})"));
                self.phase = PreviewPhase::Done(Box::new(outcome));
                note
            }
            Err(PreviewError::Cancelled) => {
                self.phase = PreviewPhase::Idle;
                self.source = None;
                None
            }
            Err(PreviewError::Failed(error)) => {
                self.phase = PreviewPhase::Failed(error.clone());
                Some(error)
            }
        }
    }

    /// Stops the encode in flight. The result arrives as a cancelled finish.
    pub fn cancel(&mut self) {
        if let Some(control) = &self.control {
            control.cancel();
        }
    }

    /// Drops the preview: stops a running encode and deletes the files of a finished one.
    pub fn clear(&mut self) {
        self.cancel();
        self.control = None;
        self.run += 1;
        if let PreviewPhase::Done(outcome) = &self.phase {
            outcome.discard();
        }
        self.phase = PreviewPhase::Idle;
        self.source = None;
    }

    /// Clears the preview unless it belongs to `current`.
    pub fn keep_only_for(&mut self, current: Option<&Path>) {
        if self.source.as_deref().is_some() && self.source.as_deref() != current {
            self.clear();
        }
    }

    pub fn summary(&self) -> Option<PreviewSummary> {
        let PreviewPhase::Done(outcome) = &self.phase else {
            return None;
        };
        Some(PreviewSummary {
            size: format_size(outcome.bytes),
            bitrate: format!("{:.0} kb/s", outcome.bitrate_kbps),
            full_size: format!("~{}", format_size(outcome.estimated_full_bytes)),
            range: format!(
                "{} to {}",
                format_clock(outcome.segment.start_seconds),
                format_clock(outcome.segment.start_seconds + outcome.segment.length_seconds)
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(file: &Path) -> PreviewOutcome {
        PreviewOutcome {
            file: file.to_path_buf(),
            segment: Segment {
                start_seconds: 90.0,
                length_seconds: 30.0,
            },
            bytes: 3_750_000,
            bitrate_kbps: 1000.0,
            estimated_full_bytes: 900_000_000,
            source_still: None,
            encoded_still: None,
            still_error: None,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kestrel-preview-state-{}-{name}",
            std::process::id()
        ));
        std::fs::write(&path, b"preview").unwrap();
        path
    }

    #[test]
    fn segment_follows_the_position_and_length() {
        let mut state = PreviewState::default();
        let start = |state: &PreviewState, length| {
            state
                .segment(Some(600.0), length)
                .map(|segment| (segment.start_seconds, segment.length_seconds))
        };
        assert_eq!(start(&state, 0), Some((300.0, 15.0)));
        state.position = 0.75;
        assert_eq!(start(&state, 1), Some((450.0, 30.0)));
        // A start near the end moves earlier so the 60 s segment still fits.
        state.position = 0.99;
        assert_eq!(start(&state, 2), Some((540.0, 60.0)));
        assert_eq!(state.segment(None, 0), None);
        assert_eq!(state.position_index(), None);
        state.position = 0.5;
        assert_eq!(state.position_index(), Some(1));
    }

    #[test]
    fn a_run_goes_from_progress_to_a_summary() {
        let file = scratch("run");
        let mut state = PreviewState::default();
        let (run, control) = state.begin(PathBuf::from("a.mkv"));
        assert!(state.is_running());
        state.progress(run, 40);
        assert!(matches!(state.phase, PreviewPhase::Running { percent: 40 }));
        // Progress of another run is ignored.
        state.progress(run + 1, 90);
        assert!(matches!(state.phase, PreviewPhase::Running { percent: 40 }));
        assert!(!control.is_cancelled());
        assert_eq!(state.finish(run, Ok(outcome(&file))), None);
        assert_eq!(
            state.summary(),
            Some(PreviewSummary {
                size: "3.8 MB".to_owned(),
                bitrate: "1000 kb/s".to_owned(),
                full_size: "~900 MB".to_owned(),
                range: "00:01:30 to 00:02:00".to_owned(),
            })
        );
        state.clear();
        assert!(!file.exists(), "clearing deletes the preview file");
        assert!(state.summary().is_none());
    }

    #[test]
    fn cancel_stops_the_run_and_returns_to_idle() {
        let mut state = PreviewState::default();
        let (run, control) = state.begin(PathBuf::from("a.mkv"));
        state.cancel();
        assert!(control.is_cancelled());
        assert!(state.is_running(), "the worker still has to report");
        assert_eq!(state.finish(run, Err(PreviewError::Cancelled)), None);
        assert!(matches!(state.phase, PreviewPhase::Idle));
        assert!(state.source.is_none());
    }

    #[test]
    fn a_failed_run_keeps_its_message() {
        let mut state = PreviewState::default();
        let (run, _) = state.begin(PathBuf::from("a.mkv"));
        let message = state.finish(run, Err(PreviewError::Failed("FFmpeg failed".to_owned())));
        assert_eq!(message.as_deref(), Some("FFmpeg failed"));
        assert!(matches!(&state.phase, PreviewPhase::Failed(text) if text == "FFmpeg failed"));
    }

    #[test]
    fn a_late_result_of_a_cleared_run_is_deleted() {
        let file = scratch("late");
        let mut state = PreviewState::default();
        let (run, control) = state.begin(PathBuf::from("a.mkv"));
        state.clear();
        assert!(control.is_cancelled(), "clearing stops the encode");
        assert_eq!(state.finish(run, Ok(outcome(&file))), None);
        assert!(!file.exists());
        assert!(matches!(state.phase, PreviewPhase::Idle));
    }

    #[test]
    fn changing_the_source_clears_the_preview() {
        let file = scratch("source");
        let mut state = PreviewState::default();
        let (run, _) = state.begin(PathBuf::from("a.mkv"));
        state.finish(run, Ok(outcome(&file)));
        state.keep_only_for(Some(Path::new("a.mkv")));
        assert!(file.exists(), "the same source keeps its preview");
        state.keep_only_for(Some(Path::new("b.mkv")));
        assert!(!file.exists());
        assert!(matches!(state.phase, PreviewPhase::Idle));
        // Nothing to clear for an idle state.
        state.keep_only_for(None);
    }

    #[test]
    fn starting_again_discards_the_previous_result() {
        let file = scratch("again");
        let mut state = PreviewState::default();
        let (run, _) = state.begin(PathBuf::from("a.mkv"));
        state.finish(run, Ok(outcome(&file)));
        state.begin(PathBuf::from("a.mkv"));
        assert!(!file.exists());
        assert!(state.is_running());
    }
}
