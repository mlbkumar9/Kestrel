//! Activity: the job log with the full command lines.

use gpui::{AnyElement, ClipboardItem, Context, Window, div, prelude::*, px};

use crate::app::Kestrel;
use crate::state::activity::Level;
use crate::widgets::{BtnKind, TagKind, button, card, icon};

impl Kestrel {
    pub fn render_activity(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let jobs = self.activity.jobs();
        let mut names = vec!["All jobs".to_owned()];
        names.extend(jobs.iter().cloned());
        let selected = self
            .activity
            .filter
            .as_ref()
            .and_then(|name| jobs.iter().position(|job| job == name))
            .map_or(0, |position| position + 1);
        let choices = names.clone();
        let filter = self.select(
            cx,
            "act-filter",
            names,
            selected,
            false,
            move |this, index, _| {
                this.activity.filter = index
                    .checked_sub(1)
                    .and_then(|i| choices.get(i + 1).cloned());
            },
        );
        let text = self.activity.as_text();
        let export_text = text.clone();
        let toolbar = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().w(px(260.0)).child(filter))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        button("act-copy", &t, BtnKind::Default, true)
                            .child(icon("copy", 12.0, t.text))
                            .child("Copy")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                this.toast("Log copied", TagKind::Ok, cx);
                            })),
                    )
                    .child(
                        button("act-export", &t, BtnKind::Default, true)
                            .child(icon("download", 12.0, t.text))
                            .child("Export...")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.export_text(
                                    "Export log",
                                    "kestrel-activity.log".to_owned(),
                                    export_text.clone(),
                                    cx,
                                );
                            })),
                    )
                    .child(
                        button("act-clear", &t, BtnKind::Ghost, true)
                            .child("Clear")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.activity.clear();
                                cx.notify();
                            })),
                    ),
            );
        let lines = self.activity.visible();
        let empty = lines.is_empty();
        let rows: Vec<AnyElement> = lines
            .into_iter()
            .map(|line| {
                let colour = match line.level {
                    Level::Info => t.info,
                    Level::Cmd => t.accent_text,
                    Level::Done => t.ok,
                    Level::Warn => t.warn,
                    Level::Error => t.bad,
                };
                div()
                    .flex()
                    .items_start()
                    .gap(px(14.0))
                    .py(px(2.0))
                    .font_family(crate::widgets::MONO)
                    .text_size(px(12.0))
                    .child(
                        div()
                            .flex_none()
                            .text_color(t.faint)
                            .child(line.time.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(48.0))
                            .text_color(colour)
                            .child(line.level.label().trim_end().to_owned()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(t.text)
                            .child(line.message.clone()),
                    )
                    .into_any_element()
            })
            .collect();
        let log = card(&t)
            .id("activity-log")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(14.0))
            .px(px(16.0))
            .children(rows)
            .when(empty, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(6.0))
                        .size_full()
                        .child(icon("activity", 26.0, t.faint))
                        .child(
                            div()
                                .text_color(t.faint)
                                .child("Nothing has run yet. Commands and results appear here."),
                        ),
                )
            });
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(px(24.0))
            .py(px(22.0))
            .gap(px(12.0))
            .child(toolbar)
            .child(log)
            .into_any_element()
    }
}
