//! Library: drop zone, stats and the probed file table.

use gpui::{AnyElement, Context, FontWeight, Window, div, prelude::*, px};

use super::{grid, page_scroll};
use crate::app::{Kestrel, Pick};
use crate::ctl::InputCfg;
use crate::state::Page;
use crate::state::format::format_size;
use crate::widgets::{
    BtnKind, Col, TagKind, body_row, button, card, header_row, icon, mono, stat_card, stat_value,
    tag, thumb,
};

const COLS: [Col; 8] = [
    Col::fixed(44.0),
    Col::grow(2.2),
    Col::fixed(84.0),
    Col::grow(1.6),
    Col::grow(1.9),
    Col::grow(1.4),
    Col::fixed(96.0).right(),
    Col::fixed(96.0).right(),
];

impl Kestrel {
    pub fn render_library(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let stats = self.library.stats();
        let visible = self.library.visible();
        let all_selected = !visible.is_empty()
            && visible
                .iter()
                .all(|index| self.library.sources[*index].selected);

        let drop_zone = div()
            .flex()
            .items_center()
            .gap(px(16.0))
            .p(px(28.0))
            .rounded(px(14.0))
            .border_1()
            .border_dashed()
            .border_color(t.line2)
            .bg(t.panel)
            .text_color(t.muted)
            .child(icon("upload", 28.0, t.accent))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_color(t.text)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Drop files or folders anywhere"),
                    )
                    .child(div().text_size(px(12.0)).child(
                        "MKV, MP4, MOV, M4V, WebM, AVI, TS, M2TS, FLV, WMV and more. Folders are scanned recursively.",
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        button("add-files", &t, BtnKind::Default, true)
                            .child("Add files")
                            .on_click(cx.listener(|this, _, _, cx| this.pick(Pick::Files, cx))),
                    )
                    .child(
                        button("add-folder", &t, BtnKind::Default, true)
                            .child("Add folder")
                            .on_click(cx.listener(|this, _, _, cx| this.pick(Pick::Folder, cx))),
                    ),
            );

        let codec_tags = if stats.codecs.is_empty() {
            div().text_size(px(15.0)).text_color(t.faint).child("-")
        } else {
            div().flex().flex_wrap().gap(px(6.0)).mt(px(6.0)).children(
                stats.codecs.iter().take(4).map(|(codec, count)| {
                    tag(format!("{codec} \u{d7}{count}"), TagKind::Plain, &t)
                }),
            )
        };
        let stat_cards = grid(4)
            .gap(px(14.0))
            .child(stat_card(
                "Files",
                stat_value(stats.files.to_string(), &t),
                &t,
            ))
            .child(stat_card(
                "Total size",
                stat_value(format_size(stats.total_size), &t),
                &t,
            ))
            .child(stat_card("Video codecs", codec_tags, &t))
            .child(stat_card(
                "Est. output",
                stat_value(
                    if stats.files == 0 {
                        "-".to_owned()
                    } else {
                        format_size(stats.estimated_output)
                    },
                    &t,
                ),
                &t,
            ));

        let filter = self.field_text(
            cx,
            window,
            "lib-filter",
            &self.library.filter.clone(),
            InputCfg::default()
                .ph("Filter by name, codec or language")
                .compact(),
            |this, text, _, _| this.library.filter = text.to_owned(),
        );
        let toolbar = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .py(px(10.0))
            .border_b_1()
            .border_color(t.line)
            .child(div().w(px(320.0)).child(filter))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        button("remux-selected", &t, BtnKind::Default, true)
                            .child("Remux selected")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(first) = this.library.selected().first() {
                                    this.remux_file = *first;
                                }
                                this.go(Page::Remux, cx);
                            })),
                    )
                    .child(
                        button("transcode-selected", &t, BtnKind::Primary, true)
                            .child("Transcode selected")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(first) = this.library.selected().first().copied() {
                                    this.select_source(first, cx);
                                }
                                this.go(Page::Transcode, cx);
                            })),
                    ),
            );

        let header = header_row(
            &t,
            &COLS,
            &[
                "",
                "Name",
                "Container",
                "Video",
                "Audio",
                "Subtitles",
                "Duration",
                "Size",
            ],
        );
        let select_all = self
            .check_box(cx, "lib-select-all", all_selected, move |this, on, _| {
                this.library.set_all_selected(on);
            })
            .into_any_element();
        let header = header.child(
            div()
                .absolute()
                .left(px(12.0))
                .top(px(8.0))
                .child(select_all),
        );

        let rows: Vec<AnyElement> = visible
            .iter()
            .enumerate()
            .map(|(position, &index)| {
                let source = &self.library.sources[index];
                let selected = source.selected;
                let cells = vec![
                    self.check_box(
                        cx,
                        format!("lib-row-{index}"),
                        selected,
                        move |this, on, _| {
                            this.library.sources[index].selected = on;
                        },
                    )
                    .into_any_element(),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .min_w_0()
                        .child(thumb(&t))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(source.name.clone()),
                        )
                        .into_any_element(),
                    tag(source.ext.clone(), TagKind::Plain, &t).into_any_element(),
                    mono(source.video_label())
                        .text_color(t.muted)
                        .truncate()
                        .into_any_element(),
                    mono(source.audio_label())
                        .text_color(t.muted)
                        .truncate()
                        .into_any_element(),
                    mono(source.subs_label())
                        .text_color(t.muted)
                        .truncate()
                        .into_any_element(),
                    mono(source.duration_label())
                        .text_color(t.muted)
                        .into_any_element(),
                    mono(source.size_label()).into_any_element(),
                ];
                body_row(&t, &COLS, cells, position + 1 == visible.len()).into_any_element()
            })
            .collect();

        let empty = self.library.sources.is_empty();
        let table = card(&t)
            .overflow_hidden()
            .child(toolbar)
            .child(div().relative().child(header))
            .children(rows)
            .when(empty, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.0))
                        .py(px(40.0))
                        .child(icon("film", 26.0, t.faint))
                        .child(div().text_color(t.muted).child("No files yet"))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(t.faint)
                                .child("Drop media here, press Ctrl O, or use Add files."),
                        ),
                )
            });

        page_scroll("library-page")
            .gap(px(18.0))
            .child(drop_zone)
            .child(stat_cards)
            .child(table)
            .into_any_element()
    }
}
