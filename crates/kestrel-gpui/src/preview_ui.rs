//! Transcode preview: starting, cancelling and playing the segment encode, and the card that
//! shows its progress, numbers and source-versus-encoded stills.

use std::path::Path;
use std::sync::Arc;

use gpui::{Context, Div, FontWeight, ObjectFit, div, prelude::*, px};
use kestrel_core::preview::{self, PreviewError, PreviewOutcome, PreviewRequest};

use crate::app::{Kestrel, UiEvent};
use crate::state::format::format_clock;
use crate::state::preview::{POSITIONS, PreviewPhase};
use crate::state::transcode::PREVIEW_LENGTHS;
use crate::widgets::controls::{segmented, slider};
use crate::widgets::{BtnKind, TagKind, button, card, field, label, mono, progress_bar};

/// What a preview worker reports back.
pub struct PreviewEvent {
    pub run: u64,
    pub update: PreviewUpdate,
}

pub enum PreviewUpdate {
    Progress(u32),
    /// The last event of a run, whatever happened.
    Done(Result<PreviewOutcome, PreviewError>),
}

impl Kestrel {
    /// Encodes the chosen segment of the current source with the form's settings, on a worker
    /// thread.
    pub fn start_preview(&mut self, cx: &mut Context<Self>) {
        if self.preview.is_running() {
            return;
        }
        let Some(ffmpeg) = self.tools.ffmpeg.as_ref().map(|tool| tool.path.clone()) else {
            self.toast("FFmpeg was not found", TagKind::Warn, cx);
            return;
        };
        let Some(source) = self.library.current_source() else {
            self.toast("Add a source to preview", TagKind::Warn, cx);
            return;
        };
        let Some(metadata) = source.metadata().cloned() else {
            self.toast("The source is still being read", TagKind::Info, cx);
            return;
        };
        let input = source.path.clone();
        let Some(encoders) = self.encoders.clone() else {
            self.toast(
                "Detecting encoders, try again in a moment",
                TagKind::Info,
                cx,
            );
            return;
        };
        let Some(segment) = self
            .preview
            .segment(metadata.duration, self.form.preview_length)
        else {
            self.toast(
                "The source has no known duration to preview",
                TagKind::Warn,
                cx,
            );
            return;
        };
        let request = PreviewRequest {
            ffmpeg,
            input: input.clone(),
            metadata,
            settings: self.form.to_job_settings(),
            encoders,
            segment,
        };
        let (run, control) = self.preview.begin(input);
        let sender = self.begin_background(cx);
        std::thread::spawn(move || {
            let mut last = u32::MAX;
            let result = preview::run_preview(&request, &control, &mut |percent| {
                if percent != last {
                    last = percent;
                    let _ = sender.send(UiEvent::Preview(Box::new(PreviewEvent {
                        run,
                        update: PreviewUpdate::Progress(percent),
                    })));
                }
            });
            let _ = sender.send(UiEvent::Preview(Box::new(PreviewEvent {
                run,
                update: PreviewUpdate::Done(result),
            })));
        });
        cx.notify();
    }

    pub fn cancel_preview(&mut self, cx: &mut Context<Self>) {
        self.preview.cancel();
        cx.notify();
    }

    pub fn play_preview(&mut self, cx: &mut Context<Self>) {
        let PreviewPhase::Done(outcome) = &self.preview.phase else {
            return;
        };
        if let Err(error) = preview::open_with_system(&outcome.file) {
            self.toast(error, TagKind::Bad, cx);
        }
    }

    pub fn on_preview_event(&mut self, event: PreviewEvent, cx: &mut Context<Self>) {
        match event.update {
            PreviewUpdate::Progress(percent) => self.preview.progress(event.run, percent),
            PreviewUpdate::Done(result) => {
                if let Some(message) = self.preview.finish(event.run, result) {
                    self.toast(message, TagKind::Bad, cx);
                }
            }
        }
        cx.notify();
    }

    /// Drops the preview when the current source is not the one it was made from.
    pub fn sync_preview_source(&mut self) {
        let current = self
            .library
            .current_source()
            .map(|source| source.path.clone());
        self.preview.keep_only_for(current.as_deref());
    }

    pub fn render_preview_card(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let length_index = self.form.preview_length;
        let source = self.library.current_source();
        let duration = source.and_then(|source| source.metadata().and_then(|meta| meta.duration));
        let current = source.map(|source| source.path.clone());
        let position = self.preview.position as f32;
        let segment = self.preview.segment(duration, length_index);
        let belongs = self.preview.source.is_none() || self.preview.source == current;

        let length = field(
            "Length",
            &t,
            self.select(
                cx,
                "tr-preview-len",
                PREVIEW_LENGTHS,
                length_index,
                false,
                |this, index, _| this.form.preview_length = index,
            ),
        )
        .w(px(110.0));
        let quick = segmented(
            cx,
            "tr-preview-pos",
            POSITIONS.map(|(text, _)| text),
            self.preview.position_index().unwrap_or(usize::MAX),
            &t,
            |this, index, _| this.preview.position = POSITIONS[index].1,
        );
        let start = field(
            "Starts at",
            &t,
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(quick)
                .child(div().w(px(150.0)).flex().child(slider(
                    cx,
                    "tr-preview-start",
                    position,
                    &t,
                    |this, value, _| this.preview.position = f64::from(value),
                ))),
        );
        let range = segment.map_or_else(
            || "-".to_owned(),
            |segment| {
                format!(
                    "{} to {}",
                    format_clock(segment.start_seconds),
                    format_clock(segment.start_seconds + segment.length_seconds)
                )
            },
        );
        let action = if let PreviewPhase::Running { percent } = self.preview.phase {
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .w(px(150.0))
                        .child(progress_bar(percent as f32, false, 6.0, &t)),
                )
                .child(mono(format!("{percent}%")).text_color(t.muted))
                .child(
                    button("tr-preview-cancel", &t, BtnKind::Default, true)
                        .child("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_preview(cx))),
                )
        } else {
            div().flex().items_center().child(
                button("tr-preview-go", &t, BtnKind::Default, true)
                    .child("Encode preview")
                    .on_click(cx.listener(|this, _, _, cx| this.start_preview(cx))),
            )
        };

        let mut body = card(&t)
            .mt(px(14.0))
            .p(px(14.0))
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(10.0))
                    .child(label("Preview", &t))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(t.muted)
                            .child("Encodes a short stretch with the settings above"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap(px(16.0))
                    .child(length)
                    .child(start)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(t.muted)
                                    .child("Segment"),
                            )
                            .child(mono(range)),
                    )
                    .child(action),
            );

        if !belongs {
            return body;
        }
        match &self.preview.phase {
            PreviewPhase::Failed(error) => {
                body = body.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(t.bad)
                        .child(format!("The preview failed: {error}")),
                );
            }
            PreviewPhase::Done(outcome) => {
                if let Some(summary) = self.preview.summary() {
                    let stat = |name: &str, value: String| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(label(name, &t))
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(value),
                            )
                    };
                    body = body.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_end()
                            .gap(px(28.0))
                            .child(stat("Preview size", summary.size))
                            .child(stat("Bitrate", summary.bitrate))
                            .child(stat("Full file, extrapolated", summary.full_size))
                            .child(
                                button("tr-preview-play", &t, BtnKind::Primary, true)
                                    .child("Play preview")
                                    .on_click(cx.listener(|this, _, _, cx| this.play_preview(cx))),
                            ),
                    );
                }
                body = body.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(14.0))
                        .child(still(
                            "Source",
                            outcome.source_still.as_deref(),
                            &outcome.still_error,
                            &t,
                        ))
                        .child(still(
                            "Encoded",
                            outcome.encoded_still.as_deref(),
                            &outcome.still_error,
                            &t,
                        )),
                );
            }
            PreviewPhase::Idle | PreviewPhase::Running { .. } => {}
        }
        body
    }
}

const STILL_WIDTH: f32 = 250.0;
const STILL_HEIGHT: f32 = 141.0;

/// One comparison still: caption above, the image (or why there is none) below.
fn still(
    caption: &str,
    path: Option<&Path>,
    error: &Option<String>,
    t: &crate::theme::Theme,
) -> Div {
    let frame = div()
        .w(px(STILL_WIDTH))
        .h(px(STILL_HEIGHT))
        .rounded(px(8.0))
        .border_1()
        .border_color(t.line2)
        .bg(t.bg)
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center();
    let frame = match path {
        Some(path) => frame.child(
            gpui::img(gpui::ImageSource::Resource(gpui::Resource::Path(
                Arc::from(path),
            )))
            .size_full()
            .object_fit(ObjectFit::Contain),
        ),
        None => frame.child(
            div()
                .px(px(10.0))
                .text_size(px(12.0))
                .text_color(t.faint)
                .child(error.clone().unwrap_or_else(|| "No still".to_owned())),
        ),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(t.muted)
                .child(format!("{caption}, segment midpoint")),
        )
        .child(frame)
}
