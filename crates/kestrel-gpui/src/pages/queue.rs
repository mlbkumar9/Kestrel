//! Queue: the running job and the table of every job.

use gpui::{
    AnyElement, BoxShadow, Context, FontWeight, MouseButton, SharedString, Window, deferred, div,
    point, prelude::*, px,
};

use super::page_scroll;
use crate::app::Kestrel;
use crate::state::format::format_clock;
use crate::state::queue::{JobStatus, WHEN_DONE};
use crate::widgets::{
    BtnKind, Col, TagKind, body_row, button, card, header_row, icon, mono, progress_bar, tag,
};

const COLS: [Col; 6] = [
    Col::grow(2.2),
    Col::fixed(100.0),
    Col::grow(1.6),
    Col::fixed(220.0),
    Col::fixed(104.0),
    Col::fixed(52.0),
];

fn status_kind(status: &JobStatus) -> TagKind {
    match status {
        JobStatus::Queued | JobStatus::Skipped => TagKind::Plain,
        JobStatus::Encoding => TagKind::Acc,
        JobStatus::Done => TagKind::Ok,
        JobStatus::Failed(_) => TagKind::Bad,
        JobStatus::Cancelled => TagKind::Warn,
    }
}

impl Kestrel {
    /// Menu for one queue row: stop, retry, remove and reveal the output.
    fn job_menu(&self, cx: &mut Context<Self>, id: u64, status: &JobStatus) -> AnyElement {
        let t = self.theme();
        let key = SharedString::from(format!("q-menu-{id}"));
        let open = self.open_menu.as_ref() == Some(&key);
        let toggle = key.clone();
        let trigger = button(
            SharedString::from(format!("q-more-{id}")),
            &t,
            BtnKind::Ghost,
            true,
        )
        .child(icon("more", 14.0, t.muted))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                this.open_menu = if this.open_menu.as_ref() == Some(&toggle) {
                    None
                } else {
                    Some(toggle.clone())
                };
                cx.stop_propagation();
                cx.notify();
            }),
        );
        let can_stop = matches!(status, JobStatus::Queued | JobStatus::Encoding);
        let can_retry = matches!(
            status,
            JobStatus::Failed(_) | JobStatus::Cancelled | JobStatus::Skipped
        );
        let can_remove = !matches!(status, JobStatus::Encoding);
        let entries: [(&str, &str, bool, u8); 4] = [
            ("pause", "Stop", can_stop, 0),
            ("refresh", "Retry", can_retry, 1),
            ("trash", "Remove", can_remove, 2),
            ("folder", "Reveal output", true, 3),
        ];
        let menu = open.then(|| {
            let items = entries.into_iter().map(|(glyph, text, enabled, action)| {
                div()
                    .id(SharedString::from(format!("q-item-{id}-{action}")))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .h(px(30.0))
                    .px(px(10.0))
                    .rounded(px(6.0))
                    .text_size(px(13.0))
                    .whitespace_nowrap()
                    .when(enabled, |this| {
                        this.cursor_pointer()
                            .text_color(t.text)
                            .hover(move |s| s.bg(t.raised))
                    })
                    .when(!enabled, |this| this.text_color(t.faint))
                    .child(icon(glyph, 13.0, if enabled { t.muted } else { t.faint }))
                    .child(text)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if !enabled {
                                return;
                            }
                            this.open_menu = None;
                            this.job_action(id, action, cx);
                            cx.notify();
                        }),
                    )
            });
            deferred(
                div()
                    .absolute()
                    .top(px(30.0))
                    .right_0()
                    .w(px(168.0))
                    .flex()
                    .flex_col()
                    .p(px(4.0))
                    .rounded(px(10.0))
                    .bg(t.panel)
                    .border_1()
                    .border_color(t.line2)
                    .shadow(vec![BoxShadow {
                        color: t.shadow.into(),
                        offset: point(px(0.0), px(12.0)),
                        blur_radius: px(32.0),
                        spread_radius: px(0.0),
                    }])
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(items),
            )
            .with_priority(10)
        });
        div()
            .relative()
            .child(trigger)
            .children(menu)
            .into_any_element()
    }

    fn job_action(&mut self, id: u64, action: u8, cx: &mut Context<Self>) {
        match action {
            0 => {
                let encoding = self
                    .queue
                    .jobs
                    .iter()
                    .any(|job| job.id == id && job.status == JobStatus::Encoding);
                if encoding {
                    self.stop_queue(cx);
                } else if let Some(job) = self.queue.find_mut(id) {
                    job.status = JobStatus::Cancelled;
                }
            }
            1 => self.queue.retry(id),
            2 => self.queue.remove(id),
            _ => {
                if let Some(job) = self.queue.jobs.iter().find(|job| job.id == id) {
                    let path = if job.output.exists() {
                        job.output.clone()
                    } else {
                        job.output
                            .parent()
                            .map_or_else(|| job.input.clone(), std::path::Path::to_path_buf)
                    };
                    cx.reveal_path(&path);
                }
            }
        }
    }

    pub fn render_queue(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let running = self.queue.running;
        let paused = self.queue.paused;
        let current = self.queue.current().cloned();

        let hero = match current {
            Some(job) => {
                let position = self
                    .queue
                    .position()
                    .map_or(String::new(), |(index, total)| {
                        format!(" {index} of {total}")
                    });
                let live = &self.queue.live;
                let mut stats: Vec<AnyElement> = Vec::new();
                if let Some(fps) = live.fps {
                    stats.push(mono(format!("{fps:.1} fps")).into_any_element());
                }
                if let Some(speed) = live.speed {
                    stats.push(mono(format!("{speed:.1}\u{d7} realtime")).into_any_element());
                }
                if let Some(eta) = live.eta_seconds {
                    stats.push(mono(format!("ETA {}", format_clock(eta))).into_any_element());
                }
                stats.push(
                    mono(format!(
                        "elapsed {}",
                        format_clock(self.queue.elapsed_seconds())
                    ))
                    .into_any_element(),
                );
                let left = div()
                    .flex_1()
                    .min_w(px(260.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("Encoding{position}")),
                            )
                            .child(tag(job.summary.clone(), TagKind::Acc, &t))
                            .child(
                                mono(format!("{}%", job.percent))
                                    .flex_1()
                                    .flex()
                                    .justify_end()
                                    .text_color(t.muted),
                            ),
                    )
                    .child(div().mt(px(10.0)).child(progress_bar(
                        job.percent as f32,
                        false,
                        8.0,
                        &t,
                    )))
                    .child(
                        div()
                            .mt(px(10.0))
                            .flex()
                            .flex_wrap()
                            .gap(px(18.0))
                            .text_color(t.muted)
                            .children(stats),
                    )
                    .child(
                        div()
                            .mt(px(6.0))
                            .text_size(px(12.0))
                            .text_color(t.faint)
                            .truncate()
                            .child(job.name.clone()),
                    );
                Some(left)
            }
            None => None,
        };
        let when_done = self.select(
            cx,
            "q-when-done",
            WHEN_DONE.map(|text| format!("When done: {text}")),
            self.queue.when_done,
            false,
            |this, index, _| this.queue.when_done = index,
        );
        let controls = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                button("q-pause", &t, BtnKind::Default, false)
                    .child(icon(
                        if paused || !running { "play" } else { "pause" },
                        13.0,
                        t.text,
                    ))
                    .child(if !running {
                        "Start"
                    } else if paused {
                        "Resume"
                    } else {
                        "Pause"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
            )
            .child(
                button("q-stop", &t, BtnKind::Default, false)
                    .child(icon("stop", 13.0, t.text))
                    .child("Stop")
                    .on_click(cx.listener(|this, _, _, cx| this.stop_queue(cx))),
            )
            .child(div().w(px(200.0)).child(when_done));
        let hero_card = card(&t)
            .p(px(18.0))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(22.0))
            .child(match hero {
                Some(body) => body,
                None => div()
                    .flex_1()
                    .min_w(px(260.0))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Nothing is encoding"),
                    )
                    .child(div().mt(px(4.0)).text_color(t.muted).child(
                        if self.queue.active_count() > 0 {
                            format!(
                                "{} job(s) waiting. Press Start to run them.",
                                self.queue.active_count()
                            )
                        } else {
                            "Queue a remux or an encode from their pages.".to_owned()
                        },
                    )),
            })
            .child(controls);

        let total = self.queue.jobs.len();
        let rows: Vec<AnyElement> = self
            .queue
            .jobs
            .iter()
            .enumerate()
            .map(|(position, job)| {
                let finished = job.status == JobStatus::Done;
                let cells = vec![
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(job.name.clone()),
                        )
                        .child(
                            mono(job.output.display().to_string())
                                .text_size(px(11.0))
                                .text_color(t.faint)
                                .truncate(),
                        )
                        .into_any_element(),
                    tag(job.kind.label(), TagKind::Plain, &t).into_any_element(),
                    mono(job.summary.clone())
                        .text_color(t.muted)
                        .truncate()
                        .into_any_element(),
                    div()
                        .w_full()
                        .child(progress_bar(
                            if finished { 100.0 } else { job.percent as f32 },
                            finished,
                            6.0,
                            &t,
                        ))
                        .into_any_element(),
                    tag(job.status.label(), status_kind(&job.status), &t).into_any_element(),
                    self.job_menu(cx, job.id, &job.status),
                ];
                body_row(&t, &COLS, cells, position + 1 == total).into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let table = card(&t)
            .child(header_row(
                &t,
                &COLS,
                &["Job", "Type", "Settings", "Progress", "Status", ""],
            ))
            .children(rows)
            .when(empty, |this| {
                this.child(
                    div()
                        .py(px(36.0))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.0))
                        .child(icon("queue", 26.0, t.faint))
                        .child(div().text_color(t.muted).child("The queue is empty")),
                )
            });
        let clear = (self.queue.jobs.iter().any(|job| !job.status.is_active())).then(|| {
            div().flex().justify_end().child(
                button("q-clear", &t, BtnKind::Ghost, true)
                    .child("Clear finished")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.queue.clear_finished();
                        cx.notify();
                    })),
            )
        });
        page_scroll("queue-page")
            .child(hero_card)
            .child(table)
            .children(clear)
            .into_any_element()
    }
}
