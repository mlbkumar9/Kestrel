//! UI-independent media probing, output planning, settings and queue execution.
//!
//! Expand sources with [inputs::expand], probe each with [mkv::read_metadata],
//! and list audio and subtitle languages separately with [mkv::available_languages].
//! Use [inputs::plan_outputs] to assign output paths, construct [worker::JobSpec]
//! values with a settings snapshot, then pass a [worker::QueueSpec] to
//! [worker::start_queue]. Receive [worker::WorkerEvent] values through an mpsc
//! channel; the optional host wake-up behavior is supplied as [worker::EventNotifier].
//! Headless callers can collect events with [worker::run_queue_blocking].

/// Crop detection, preview rendering and size estimation.
pub mod analysis;
/// Application identifiers and legacy storage file migration.
pub mod branding;
/// Container and stream compatibility rules.
pub mod compat;
/// Encoder discovery and capability checks.
pub mod encoders;
/// Supported containers and source formats.
pub mod formats;
/// Source expansion and collision-free output planning.
pub mod inputs;
/// Detection and removal of legacy installations.
pub mod legacy;
/// Source probing, track selection and remux arguments.
pub mod mkv;
/// Built-in and user-defined job presets.
pub mod presets;
/// Persisted preferences, source state and migration.
pub mod settings;
/// Subtitle paths, conversion and burn-in preparation.
pub mod subtitles;
/// External tool discovery and platform process creation.
pub mod tools;
/// Job settings and FFmpeg command construction.
pub mod transcode;
/// Background scanning and queue execution with progress events.
pub mod worker;
