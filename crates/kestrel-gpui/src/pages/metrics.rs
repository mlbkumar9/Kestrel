//! Metrics: VMAF, PSNR, SSIM and XPSNR of one or more encodes against a reference, with CSV and
//! worst-frame exports and the FFmpeg command log.

use std::path::PathBuf;

use gpui::{AnyElement, ClipboardItem, Context, FontWeight, Window, div, prelude::*, px};
use kestrel_core::metrics::{self, MetricsControl, MetricsError, MetricsOutcome, MetricsProgress};

use super::page_scroll;
use crate::app::{Kestrel, Pick, UiEvent};
use crate::ctl::InputCfg;
use crate::state::activity::Level;
use crate::state::metrics::{MAX_WORST_FRAMES, MODELS, MetricsPhase, csv_file_names};
use crate::theme::RADIUS_CONTROL;
use crate::widgets::{
    BtnKind, Col, TagKind, body_row, button, card, field, header_row, icon, mono, page_title,
    progress_bar, scroll_table,
};

/// What a Metrics worker reports back.
pub struct MetricsEvent {
    pub run: u64,
    pub update: MetricsUpdate,
}

pub enum MetricsUpdate {
    Progress(MetricsProgress),
    /// The last event of a run, whatever happened.
    Done(Result<MetricsOutcome, MetricsError>),
    /// The last event of an export. `text` is empty when the user closed the dialog.
    Exported {
        text: String,
        error: bool,
        commands: Vec<String>,
    },
}

impl MetricsEvent {
    /// True for the event that takes a background job's count back down.
    pub fn ends_work(&self) -> bool {
        !matches!(self.update, MetricsUpdate::Progress(_))
    }
}

const TABLE_COLUMNS: [Col; 8] = [
    Col::grow(2.0),
    Col::fixed(70.0).right(),
    Col::fixed(80.0).right(),
    Col::fixed(80.0).right(),
    Col::fixed(80.0).right(),
    Col::fixed(80.0).right(),
    Col::fixed(80.0).right(),
    Col::fixed(80.0).right(),
];
const TABLE_LABELS: [&str; 8] = [
    "File", "Frames", "VMAF", "VMAF min", "VMAF 1%", "PSNR dB", "SSIM", "XPSNR dB",
];

impl Kestrel {
    /// Measures every listed file against the reference on a worker thread.
    pub fn start_metrics(&mut self, cx: &mut Context<Self>) {
        if self.metrics.is_running() {
            return;
        }
        if self.tools.ffmpeg.is_none() || self.tools.ffprobe.is_none() {
            self.toast("FFmpeg and FFprobe were not found", TagKind::Warn, cx);
            return;
        }
        let settings = match self.metrics.settings() {
            Ok(settings) => settings,
            Err(message) => {
                self.toast(message, TagKind::Warn, cx);
                return;
            }
        };
        let tools = self.tools.clone();
        let count = settings.distorted.len();
        self.activity.push(
            Level::Info,
            format!(
                "Measuring {count} file(s) against {}",
                settings.reference.display()
            ),
            None,
        );
        let (run, control) = self.metrics.begin();
        let sender = self.begin_background(cx);
        std::thread::spawn(move || {
            let result = metrics::run_metrics(&tools, &settings, &control, &mut |progress| {
                let _ = sender.send(UiEvent::Metrics(Box::new(MetricsEvent {
                    run,
                    update: MetricsUpdate::Progress(progress),
                })));
            });
            let _ = sender.send(UiEvent::Metrics(Box::new(MetricsEvent {
                run,
                update: MetricsUpdate::Done(result),
            })));
        });
        cx.notify();
    }

    pub fn cancel_metrics(&mut self, cx: &mut Context<Self>) {
        self.metrics.cancel();
        cx.notify();
    }

    pub fn on_metrics_event(&mut self, event: MetricsEvent, cx: &mut Context<Self>) {
        let run = event.run;
        match event.update {
            MetricsUpdate::Progress(progress) => self.metrics.progress(run, progress),
            MetricsUpdate::Done(result) => {
                match &result {
                    Ok(outcome) => {
                        for command in &outcome.commands {
                            self.activity.push(Level::Cmd, command.clone(), None);
                        }
                        self.activity
                            .push(Level::Done, "Quality metrics finished", None);
                    }
                    Err(MetricsError::Cancelled) => {
                        self.activity
                            .push(Level::Info, "Quality metrics cancelled", None);
                    }
                    Err(MetricsError::Failed(message)) => {
                        self.activity.push(
                            Level::Error,
                            format!("Quality metrics failed: {message}"),
                            None,
                        );
                    }
                }
                if let Some(message) = self.metrics.finish(run, result) {
                    self.toast(message, TagKind::Bad, cx);
                }
            }
            MetricsUpdate::Exported {
                text,
                error,
                commands,
            } => {
                for command in &commands {
                    self.activity.push(Level::Cmd, command.clone(), None);
                }
                self.metrics.log_commands(run, commands);
                if !text.is_empty() {
                    let kind = if error { TagKind::Bad } else { TagKind::Ok };
                    self.toast(text, kind, cx);
                }
            }
        }
        cx.notify();
    }

    /// Asks for a folder, then writes one per-frame CSV per measured file into it.
    pub fn export_metric_frames(&mut self, cx: &mut Context<Self>) {
        let Some(outcome) = &self.metrics.outcome else {
            return;
        };
        let measured: Vec<(String, String)> = outcome
            .files
            .iter()
            .filter_map(|file| {
                let metrics = file.outcome.as_ref().ok()?;
                Some((file.name(), metrics::frames_csv(&metrics.rows)))
            })
            .collect();
        if measured.is_empty() {
            self.toast("There are no results to export", TagKind::Warn, cx);
            return;
        }
        let names: Vec<String> = measured.iter().map(|(name, _)| name.clone()).collect();
        let files: Vec<(String, String)> = csv_file_names(&names)
            .into_iter()
            .zip(measured.into_iter().map(|(_, text)| text))
            .collect();
        let run = self.metrics.run_number();
        let sender = self.begin_background(cx);
        std::thread::spawn(move || {
            let update = match rfd::FileDialog::new()
                .set_title("Choose a folder for the per-frame CSV files")
                .pick_folder()
            {
                None => export_update(String::new(), false, Vec::new()),
                Some(folder) => {
                    let failure = files.iter().find_map(|(name, text)| {
                        let path = folder.join(name);
                        std::fs::write(&path, text)
                            .err()
                            .map(|error| format!("Could not save {}: {error}", path.display()))
                    });
                    match failure {
                        Some(message) => export_update(message, true, Vec::new()),
                        None => export_update(
                            format!("Saved {} CSV file(s) to {}", files.len(), folder.display()),
                            false,
                            Vec::new(),
                        ),
                    }
                }
            };
            let _ = sender.send(UiEvent::Metrics(Box::new(MetricsEvent { run, update })));
        });
    }

    /// Asks for a folder, then writes the worst frames of every measured file there as PNGs.
    pub fn export_worst_frames(&mut self, cx: &mut Context<Self>) {
        let Some(ffmpeg) = self.tools.ffmpeg.as_ref().map(|tool| tool.path.clone()) else {
            self.toast("FFmpeg was not found", TagKind::Warn, cx);
            return;
        };
        let Some(outcome) = &self.metrics.outcome else {
            return;
        };
        let range = outcome.range;
        let count = self.metrics.prefs.worst_frames;
        let mut jobs: Vec<(PathBuf, Vec<usize>)> = Vec::new();
        for file in &outcome.files {
            let Ok(measured) = &file.outcome else {
                continue;
            };
            match metrics::worst_frames(&measured.rows, count) {
                Ok(worst) => jobs.push((
                    file.distorted.clone(),
                    worst.into_iter().map(|(frame, _)| frame).collect(),
                )),
                Err(message) => {
                    self.toast(message, TagKind::Warn, cx);
                    return;
                }
            }
        }
        if jobs.is_empty() {
            self.toast("There are no results to export", TagKind::Warn, cx);
            return;
        }
        let run = self.metrics.run_number();
        let sender = self.begin_background(cx);
        std::thread::spawn(move || {
            let update = match rfd::FileDialog::new()
                .set_title("Choose a folder for the worst-frame PNG files")
                .pick_folder()
            {
                None => export_update(String::new(), false, Vec::new()),
                Some(folder) => {
                    let control = MetricsControl::default();
                    let mut commands = Vec::new();
                    let mut written = 0;
                    let mut failure = None;
                    for (distorted, frames) in &jobs {
                        match metrics::export_worst_frames(
                            &ffmpeg,
                            distorted,
                            range,
                            frames,
                            &folder,
                            &control,
                            &mut commands,
                        ) {
                            Ok(files) => written += files.len(),
                            Err(error) => {
                                failure = Some(format!(
                                    "Worst frames of {} failed: {error}",
                                    distorted.display()
                                ));
                                break;
                            }
                        }
                    }
                    match failure {
                        Some(message) => export_update(message, true, commands),
                        None => export_update(
                            format!("Saved {written} frame(s) to {}", folder.display()),
                            false,
                            commands,
                        ),
                    }
                }
            };
            let _ = sender.send(UiEvent::Metrics(Box::new(MetricsEvent { run, update })));
        });
    }

    pub fn render_metrics(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let running = self.metrics.is_running();
        let has_results = self.metrics.has_results();

        let action = if running {
            button("met-cancel", &t, BtnKind::Default, false)
                .child(icon("stop", 13.0, t.text))
                .child("Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.cancel_metrics(cx)))
        } else {
            button("met-run", &t, BtnKind::Primary, false)
                .child(icon("play", 13.0, t.on_accent))
                .child("Run")
                .when(!self.metrics.can_run(), |this| this.opacity(0.5))
                .on_click(cx.listener(|this, _, _, cx| this.start_metrics(cx)))
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(page_title(
                "Quality metrics",
                "VMAF, PSNR, SSIM and XPSNR of an encode against its source, per frame.",
                &t,
            ))
            .child(div().flex_1().flex().justify_end().child(action));

        let overall = (self.metrics.overall * 100.0).round();
        let progress = running.then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(div().flex_1().child(progress_bar(overall, false, 6.0, &t)))
                .child(mono(format!("{overall:.0}%")).text_color(t.muted))
        });
        let failure = match &self.metrics.phase {
            MetricsPhase::Failed(message) => Some(
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(RADIUS_CONTROL))
                    .border_1()
                    .border_color(t.bad)
                    .text_color(t.bad)
                    .child(message.clone()),
            ),
            _ => None,
        };

        // Files
        let reference_text = self
            .metrics
            .reference
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let reference = self.field_text(
            cx,
            window,
            "met-ref",
            &reference_text,
            InputCfg::default().mono().disabled(true).ph("Source file"),
            |_, _, _, _| {},
        );
        let reference_row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().flex_1().min_w_0().child(reference))
            .child(
                button("met-ref-pick", &t, BtnKind::Default, false)
                    .child(icon("folder", 13.0, t.text))
                    .child("Browse...")
                    .on_click(cx.listener(|this, _, _, cx| this.pick(Pick::MetricsReference, cx))),
            );
        let file_rows: Vec<AnyElement> = self
            .metrics
            .distorted
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into(),
                );
                let fraction = self.metrics.file_progress.get(index).copied();
                let bar = fraction
                    .filter(|_| running || self.metrics.has_results())
                    .map(|fraction| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .w(px(170.0))
                            .child(div().flex_1().child(progress_bar(
                                fraction * 100.0,
                                fraction >= 1.0,
                                6.0,
                                &t,
                            )))
                            .child(mono(format!("{:.0}%", fraction * 100.0)).text_color(t.muted))
                    });
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .min_h(px(36.0))
                    .px(px(12.0))
                    .border_b_1()
                    .border_color(t.line)
                    .child(icon("film", 14.0, t.muted))
                    .child(div().flex_1().min_w_0().truncate().child(name))
                    .children(bar)
                    .child(
                        button(format!("met-del-{index}"), &t, BtnKind::Ghost, true)
                            .child(icon("x", 12.0, t.muted))
                            .when(running, |this| this.opacity(0.4))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.metrics.remove_distorted(index);
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let empty_files = file_rows.is_empty();
        let distorted = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(card(&t).children(file_rows).when(empty_files, |this| {
                this.child(
                    div()
                        .px(px(12.0))
                        .py(px(14.0))
                        .text_color(t.faint)
                        .child("Add the encoded files to measure."),
                )
            }))
            .child(
                div().flex().child(
                    button("met-add", &t, BtnKind::Default, true)
                        .child(icon("plus", 12.0, t.text))
                        .child("Add files...")
                        .on_click(
                            cx.listener(|this, _, _, cx| this.pick(Pick::MetricsDistorted, cx)),
                        ),
                ),
            );
        let files = div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(field("Reference (source)", &t, reference_row))
            .child(field("Distorted (encodes)", &t, distorted));

        // Options
        let prefs = self.metrics.prefs.clone();
        let metric_checks = div()
            .flex()
            .items_center()
            .gap(px(16.0))
            .child(
                self.check(cx, "met-vmaf", prefs.vmaf, "VMAF", |this, on, _| {
                    this.metrics.prefs.vmaf = on;
                }),
            )
            .child(
                self.check(cx, "met-psnr", prefs.psnr, "PSNR", |this, on, _| {
                    this.metrics.prefs.psnr = on;
                }),
            )
            .child(
                self.check(cx, "met-ssim", prefs.ssim, "SSIM", |this, on, _| {
                    this.metrics.prefs.ssim = on;
                }),
            )
            .child(
                self.check(cx, "met-xpsnr", prefs.xpsnr, "XPSNR", |this, on, _| {
                    this.metrics.prefs.xpsnr = on;
                }),
            );
        let model = self.select(
            cx,
            "met-model",
            MODELS,
            prefs.model,
            false,
            |this, index, _| {
                this.metrics.prefs.model = index;
            },
        );
        let start_text = self.metrics.range_start.clone();
        let range_start = self.field_text(
            cx,
            window,
            "met-start",
            &start_text,
            InputCfg::default().mono().numeric().ph("0"),
            |this, text, _, _| this.metrics.range_start = text.to_owned(),
        );
        let duration_text = self.metrics.range_duration.clone();
        let range_duration = self.field_text(
            cx,
            window,
            "met-duration",
            &duration_text,
            InputCfg::default().mono().numeric().ph("Whole clip"),
            |this, text, _, _| this.metrics.range_duration = text.to_owned(),
        );
        let worst = self.field_number(
            cx,
            window,
            "met-worst",
            prefs.worst_frames as i64,
            InputCfg::default().mono(),
            |this, value, _| {
                this.metrics.prefs.worst_frames = value.clamp(1, MAX_WORST_FRAMES as i64) as usize;
            },
        );
        let options = div()
            .flex()
            .flex_wrap()
            .items_end()
            .gap(px(20.0))
            .child(field("Metrics", &t, metric_checks))
            .child(field("VMAF model", &t, model).w(px(190.0)))
            .child(field("Range start (s)", &t, range_start).w(px(110.0)))
            .child(field("Range length (s)", &t, range_duration).w(px(130.0)))
            .child(field("Worst frames", &t, worst).w(px(100.0)));

        // Results
        let rows = self.metrics.rows();
        let table: AnyElement = if rows.is_empty() {
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .h(px(120.0))
                .text_color(t.faint)
                .child(icon("metrics", 24.0, t.faint))
                .child(if running {
                    "Measuring..."
                } else {
                    "Scores appear here after a run."
                })
                .into_any_element()
        } else {
            let last = rows.len() - 1;
            let mut table_rows: Vec<AnyElement> =
                vec![header_row(&t, &TABLE_COLUMNS, &TABLE_LABELS).into_any_element()];
            for (index, row) in rows.into_iter().enumerate() {
                let name = div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(row.name),
                    )
                    .children(
                        row.error
                            .map(|error| div().text_size(px(11.0)).text_color(t.bad).child(error)),
                    );
                let cells = vec![
                    name.into_any_element(),
                    mono(row.frames).text_color(t.muted).into_any_element(),
                    mono(row.vmaf_mean).into_any_element(),
                    mono(row.vmaf_min).into_any_element(),
                    mono(row.vmaf_low).into_any_element(),
                    mono(row.psnr).into_any_element(),
                    mono(row.ssim).into_any_element(),
                    mono(row.xpsnr).into_any_element(),
                ];
                table_rows
                    .push(body_row(&t, &TABLE_COLUMNS, cells, index == last).into_any_element());
            }
            scroll_table("met-table", 700.0, table_rows).into_any_element()
        };
        let export_button = |id: &'static str, text: &'static str| {
            button(id, &t, BtnKind::Default, true)
                .child(icon("download", 12.0, t.text))
                .child(text)
                .when(!has_results, |this| this.opacity(0.5))
        };
        let exports = div()
            .flex()
            .flex_wrap()
            .gap(px(8.0))
            .child(
                export_button("met-csv-frames", "Export per-frame CSV").on_click(cx.listener(
                    move |this, _, _, cx| {
                        if has_results {
                            this.export_metric_frames(cx);
                        }
                    },
                )),
            )
            .child(
                export_button("met-csv-summary", "Export summary CSV").on_click(cx.listener(
                    move |this, _, _, cx| {
                        let Some(outcome) = this.metrics.outcome.as_ref().filter(|_| has_results)
                        else {
                            return;
                        };
                        let text = outcome.summary_csv();
                        this.export_text(
                            "Export summary CSV",
                            "kestrel-metrics-summary.csv".to_owned(),
                            text,
                            cx,
                        );
                    },
                )),
            )
            .child(
                export_button("met-worst", "Export worst frames (PNG)").on_click(cx.listener(
                    move |this, _, _, cx| {
                        if has_results {
                            this.export_worst_frames(cx);
                        }
                    },
                )),
            );

        // Command log
        let show_log = self.metrics.show_log;
        let log_text = self.metrics.command_log();
        let log_toggle = div()
            .id("met-log-toggle")
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_pointer()
            .text_size(px(12.0))
            .text_color(t.muted)
            .child(icon(
                if show_log {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                12.0,
                t.muted,
            ))
            .child("FFmpeg commands")
            .on_click(cx.listener(|this, _, _, cx| {
                this.metrics.show_log = !this.metrics.show_log;
                cx.notify();
            }));
        let log_body = show_log.then(|| {
            let copy = log_text.clone();
            let shown = if log_text.is_empty() {
                "Commands appear here after a run.".to_owned()
            } else {
                log_text.clone()
            };
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .id("met-log")
                        .p(px(10.0))
                        .max_h(px(220.0))
                        .overflow_y_scroll()
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(t.line)
                        .bg(t.bg)
                        .text_size(px(11.0))
                        .text_color(t.text)
                        .font_family(crate::widgets::MONO)
                        .child(shown),
                )
                .child(
                    div().flex().child(
                        button("met-log-copy", &t, BtnKind::Ghost, true)
                            .child(icon("copy", 12.0, t.muted))
                            .child("Copy")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                this.toast("Commands copied", TagKind::Ok, cx);
                            })),
                    ),
                )
        });
        let log = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(log_toggle)
            .children(log_body);

        page_scroll("metrics-page")
            .child(header)
            .children(progress)
            .children(failure)
            .child(card(&t).p(px(16.0)).child(files))
            .child(card(&t).p(px(16.0)).child(options))
            .child(card(&t).child(table))
            .child(exports)
            .child(log)
            .into_any_element()
    }
}

fn export_update(text: String, error: bool, commands: Vec<String>) -> MetricsUpdate {
    MetricsUpdate::Exported {
        text,
        error,
        commands,
    }
}
