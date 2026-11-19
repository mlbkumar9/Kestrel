//! Remux: language filters, per-file track editor and the output inspector.

use gpui::{AnyElement, Context, FontWeight, Window, div, prelude::*, px};
use kestrel_core::compat;
use kestrel_core::formats::Container;
use kestrel_core::inputs::ExistingPolicy;
use kestrel_core::worker::RemuxEngine;

use super::{inspector, page_scroll};
use crate::app::{Kestrel, Pick};
use crate::ctl::InputCfg;
use crate::state::Page;
use crate::state::format::tool_version;
use crate::state::remux::{FileEdits, TrackKind, track_rows};
use crate::widgets::{
    BtnKind, Col, TagKind, body_row, button, card, chip, field, field_label, header_row, icon,
    mono, page_title, section, section_title, segmented, tag,
};

const COLS: [Col; 8] = [
    Col::fixed(64.0),
    Col::fixed(44.0),
    Col::fixed(96.0),
    Col::grow(1.2),
    Col::fixed(104.0),
    Col::grow(2.0),
    Col::fixed(76.0),
    Col::fixed(76.0),
];

const CONTAINERS: [Container; 4] = [
    Container::Mkv,
    Container::Mp4,
    Container::Mov,
    Container::WebM,
];
const POLICIES: [(&str, ExistingPolicy); 3] = [
    ("Rename", ExistingPolicy::Rename),
    ("Skip", ExistingPolicy::Skip),
    ("Overwrite", ExistingPolicy::Overwrite),
];

impl Kestrel {
    fn with_remux_edits(&mut self, change: impl FnOnce(&mut FileEdits)) {
        let Some(source) = self.library.sources.get(self.remux_file) else {
            return;
        };
        let Some(meta) = source.metadata().cloned() else {
            return;
        };
        let path = source.path.clone();
        change(self.remux.edits_for(&path, &meta));
    }

    fn language_chips(&self, cx: &mut Context<Self>, subtitles: bool) -> Vec<AnyElement> {
        let t = self.theme();
        let counts = self.library.language_counts(subtitles);
        let (selected, extras) = if subtitles {
            (&self.remux.sub_langs, &self.remux.extra_subs)
        } else {
            (&self.remux.audio_langs, &self.remux.extra_audio)
        };
        let prefix = if subtitles { "chip-s" } else { "chip-a" };
        let mut chips: Vec<AnyElement> = counts
            .iter()
            .map(|(code, count)| (code.clone(), Some(*count)))
            .chain(
                extras
                    .iter()
                    .filter(|code| !counts.iter().any(|(known, _)| known == *code))
                    .map(|code| (code.clone(), None)),
            )
            .map(|(code, count)| {
                let on = selected.contains(&code);
                let toggle = code.clone();
                chip(format!("{prefix}-{code}"), code, count, on, &t)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.remux.toggle_language(subtitles, &toggle);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        if chips.is_empty() {
            chips.push(
                div()
                    .text_size(px(12.0))
                    .text_color(t.faint)
                    .child("Add files in the Library to see their languages. No selection keeps every language.")
                    .into_any_element(),
            );
        }
        chips
    }

    pub fn render_remux(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let selected_files = self.library.selected();
        if self.remux_file >= self.library.sources.len() {
            self.remux_file = selected_files.first().copied().unwrap_or(0);
        }

        // Language cards.
        let audio_input = self.field_text(
            cx,
            window,
            "rmx-lang-audio",
            "",
            InputCfg::default().pill().ph("+ code"),
            |this, text, submit, cx| {
                if submit
                    && this.remux.add_custom_language(false, text)
                    && let Some(input) = this.inputs.get("rmx-lang-audio").cloned()
                {
                    input.update(cx, |input, cx| input.set_text("", cx));
                }
            },
        );
        let sub_input = self.field_text(
            cx,
            window,
            "rmx-lang-sub",
            "",
            InputCfg::default().pill().ph("+ code"),
            |this, text, submit, cx| {
                if submit
                    && this.remux.add_custom_language(true, text)
                    && let Some(input) = this.inputs.get("rmx-lang-sub").cloned()
                {
                    input.update(cx, |input, cx| input.set_text("", cx));
                }
            },
        );
        let audio_chips = self.language_chips(cx, false);
        let sub_chips = self.language_chips(cx, true);
        let include = self
            .check(
                cx,
                "rmx-include-subs",
                self.remux.include_subtitles,
                "Include subtitles",
                |this, on, _| this.remux.include_subtitles = on,
            )
            .text_size(px(12.0));
        let languages = card(&t)
            .child(
                section(&t, false)
                    .gap(px(14.0))
                    .child(section_title("Keep audio languages", &t))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.0))
                            .children(audio_chips)
                            .child(div().w(px(86.0)).child(audio_input)),
                    ),
            )
            .child(
                section(&t, true)
                    .gap(px(14.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(section_title("Keep subtitle languages", &t))
                            .child(div().flex_1().flex().justify_end().child(include)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.0))
                            .children(sub_chips)
                            .child(div().w(px(86.0)).child(sub_input)),
                    ),
            );

        // Tracks table for one file.
        let file = self.library.sources.get(self.remux_file).cloned();
        let meta = file.as_ref().and_then(|source| source.metadata().cloned());
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut edit_count = 0usize;
        if let (Some(file), Some(meta)) = (&file, &meta) {
            let edits = self.remux.edits_for(&file.path, meta).clone();
            edit_count = edits.change_count(meta);
            let track_rows = track_rows(meta);
            let added_count = edits.added.len();
            for (index, (row, edit)) in track_rows.iter().zip(&edits.tracks).enumerate() {
                let kind = match row.kind {
                    TrackKind::Video => TagKind::Info,
                    TrackKind::Audio => TagKind::Acc,
                    TrackKind::Subtitle => TagKind::Ok,
                };
                let language = self.field_text(
                    cx,
                    window,
                    format!("rmx-lang-{index}"),
                    &edit.language,
                    InputCfg::default().compact().mono(),
                    move |this, text, _, _| {
                        let text = text.to_owned();
                        this.with_remux_edits(|edits| edits.tracks[index].language = text);
                    },
                );
                let title = self.field_text(
                    cx,
                    window,
                    format!("rmx-title-{index}"),
                    &edit.title,
                    InputCfg::default().compact(),
                    move |this, text, _, _| {
                        let text = text.to_owned();
                        this.with_remux_edits(|edits| edits.tracks[index].title = text);
                    },
                );
                let cells = vec![
                    self.check_box(
                        cx,
                        format!("rmx-keep-{index}"),
                        edit.keep,
                        move |this, on, _| {
                            this.with_remux_edits(|edits| edits.tracks[index].keep = on);
                        },
                    )
                    .into_any_element(),
                    mono(index.to_string())
                        .text_color(t.faint)
                        .into_any_element(),
                    tag(row.kind.label(), kind, &t).into_any_element(),
                    mono(row.codec.clone()).truncate().into_any_element(),
                    div().w_full().child(language).into_any_element(),
                    div().w_full().child(title).into_any_element(),
                    self.check_box(
                        cx,
                        format!("rmx-def-{index}"),
                        edit.default,
                        move |this, on, _| {
                            this.with_remux_edits(|edits| edits.tracks[index].default = on);
                        },
                    )
                    .into_any_element(),
                    self.check_box(
                        cx,
                        format!("rmx-forced-{index}"),
                        edit.forced,
                        move |this, on, _| {
                            this.with_remux_edits(|edits| edits.tracks[index].forced = on);
                        },
                    )
                    .into_any_element(),
                ];
                rows.push(
                    body_row(
                        &t,
                        &COLS,
                        cells,
                        added_count == 0 && index + 1 == track_rows.len(),
                    )
                    .into_any_element(),
                );
            }
            let base = track_rows.len();
            for (added_index, added) in edits.added.iter().enumerate() {
                let number = base + added_index;
                let language = self.field_text(
                    cx,
                    window,
                    format!("rmx-add-lang-{added_index}"),
                    &added.language,
                    InputCfg::default().compact().mono(),
                    move |this, text, _, _| {
                        let text = text.to_owned();
                        this.with_remux_edits(|edits| {
                            if let Some(file) = edits.added.get_mut(added_index) {
                                file.language = text;
                            }
                        });
                    },
                );
                let title = self.field_text(
                    cx,
                    window,
                    format!("rmx-add-title-{added_index}"),
                    &added.title,
                    InputCfg::default().compact(),
                    move |this, text, _, _| {
                        let text = text.to_owned();
                        this.with_remux_edits(|edits| {
                            if let Some(file) = edits.added.get_mut(added_index) {
                                file.title = text;
                            }
                        });
                    },
                );
                let kind = match added.kind {
                    crate::state::remux::AddedKind::Subtitle => TagKind::Ok,
                    crate::state::remux::AddedKind::Audio => TagKind::Acc,
                };
                let cells = vec![
                    button(
                        format!("rmx-add-remove-{added_index}"),
                        &t,
                        BtnKind::Ghost,
                        true,
                    )
                    .tooltip(crate::widgets::overlay::tooltip("Remove this added file"))
                    .child(icon("x", 12.0, t.muted))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.with_remux_edits(|edits| {
                            if added_index < edits.added.len() {
                                edits.added.remove(added_index);
                            }
                        });
                        cx.notify();
                    }))
                    .into_any_element(),
                    mono(number.to_string())
                        .text_color(t.faint)
                        .into_any_element(),
                    tag(added.kind.label(), kind, &t).into_any_element(),
                    div()
                        .min_w_0()
                        .child(mono(added.codec_label()).truncate())
                        .into_any_element(),
                    div().w_full().child(language).into_any_element(),
                    div().w_full().child(title).into_any_element(),
                    self.check_box(
                        cx,
                        format!("rmx-add-def-{added_index}"),
                        added.default,
                        move |this, on, _| {
                            this.with_remux_edits(|edits| {
                                if let Some(file) = edits.added.get_mut(added_index) {
                                    file.default = on;
                                }
                            });
                        },
                    )
                    .into_any_element(),
                    self.check_box(
                        cx,
                        format!("rmx-add-forced-{added_index}"),
                        added.forced,
                        move |this, on, _| {
                            this.with_remux_edits(|edits| {
                                if let Some(file) = edits.added.get_mut(added_index) {
                                    file.forced = on;
                                }
                            });
                        },
                    )
                    .into_any_element(),
                ];
                rows.push(
                    body_row(&t, &COLS, cells, added_index + 1 == added_count).into_any_element(),
                );
            }
        }

        let file_picker = if selected_files.len() > 1 {
            let names: Vec<String> = selected_files
                .iter()
                .map(|index| self.library.sources[*index].name.clone())
                .collect();
            let position = selected_files
                .iter()
                .position(|index| *index == self.remux_file)
                .unwrap_or(0);
            let targets = selected_files.clone();
            Some(div().w(px(260.0)).child(self.select(
                cx,
                "rmx-file",
                names,
                position,
                true,
                move |this, pick, _| {
                    if let Some(index) = targets.get(pick) {
                        this.remux_file = *index;
                    }
                },
            )))
        } else {
            None
        };
        let tracks_header = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(14.0))
            .py(px(12.0))
            .border_b_1()
            .border_color(t.line)
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Tracks"))
            .when(edit_count > 0, |this| {
                this.child(tag(format!("{edit_count} edits"), TagKind::Acc, &t))
            })
            .child(match &file {
                Some(file) => mono(file.name.clone()).text_color(t.faint).truncate(),
                None => mono("no file selected").text_color(t.faint),
            })
            .children(file_picker)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        button("add-sub-file", &t, BtnKind::Default, true)
                            .child(icon("plus", 13.0, t.text))
                            .child("Add subtitle file")
                            .on_click(cx.listener(|this, _, _, cx| this.pick(Pick::Subtitle, cx))),
                    )
                    .child(
                        button("add-audio-file", &t, BtnKind::Default, true)
                            .child("Add audio file")
                            .on_click(cx.listener(|this, _, _, cx| this.pick(Pick::Audio, cx))),
                    ),
            );
        let tracks_empty = rows.is_empty();
        let tracks = card(&t)
            .overflow_hidden()
            .child(tracks_header)
            .child(header_row(
                &t,
                &COLS,
                &[
                    "Keep", "#", "Type", "Codec", "Language", "Title", "Default", "Forced",
                ],
            ))
            .children(rows)
            .when(tracks_empty, |this| {
                this.child(
                    div()
                        .py(px(28.0))
                        .flex()
                        .justify_center()
                        .text_color(t.faint)
                        .child(if file.is_some() {
                            "Reading tracks..."
                        } else {
                            "Select a file in the Library to edit its tracks."
                        }),
                )
            });

        // Inspector.
        let container_index = CONTAINERS
            .iter()
            .position(|container| *container == self.remux.container)
            .unwrap_or(0);
        let engine_labels = [
            ("MKVToolNix", self.tools.mkvmerge.as_ref()),
            ("FFmpeg", self.tools.ffmpeg.as_ref()),
        ]
        .map(
            |(name, tool)| match tool.and_then(|tool| tool.version.as_deref()) {
                Some(version) => format!("{name} {}", tool_version(version)),
                None => name.to_owned(),
            },
        );
        let engine_index = usize::from(self.remux.engine == RemuxEngine::Ffmpeg);
        let destination = self.field_text(
            cx,
            window,
            "rmx-dest",
            &self.remux.destination.clone(),
            InputCfg::default().mono().ph("Next to the source"),
            |this, text, _, _| this.remux.destination = text.to_owned(),
        );
        let collision_index = POLICIES
            .iter()
            .position(|(_, policy)| *policy == self.remux.collision)
            .unwrap_or(0);
        let warnings = meta
            .as_ref()
            .map(|meta| compat::job_warnings(meta, &self.remux.job_settings()))
            .unwrap_or_default();
        let warning_card =
            card(&t)
                .p(px(12.0))
                .bg(t.raised)
                .gap(px(8.0))
                .children(if warnings.is_empty() {
                    vec![
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .text_size(px(12.0))
                            .child(tag(self.remux.container.label(), TagKind::Ok, &t))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_color(t.muted)
                                    .child("All kept tracks can be stored in this container."),
                            )
                            .into_any_element(),
                    ]
                } else {
                    warnings
                        .iter()
                        .map(|warning| {
                            div()
                                .flex()
                                .items_start()
                                .gap(px(8.0))
                                .text_size(px(12.0))
                                .child(tag(self.remux.container.label(), TagKind::Warn, &t))
                                .child(div().flex_1().text_color(t.muted).child(warning.clone()))
                                .into_any_element()
                        })
                        .collect()
                });
        let count = selected_files.len();
        let side = inspector(300.0, &t)
            .child(field(
                "Output container",
                &t,
                segmented(
                    cx,
                    "rmx-container",
                    CONTAINERS.map(Container::label),
                    container_index,
                    &t,
                    |this, index, _| this.remux.container = CONTAINERS[index],
                ),
            ))
            .child(field(
                "Engine",
                &t,
                segmented(
                    cx,
                    "rmx-engine",
                    engine_labels,
                    engine_index,
                    &t,
                    |this, index, _| {
                        this.remux.engine = if index == 0 {
                            RemuxEngine::MkvToolNix
                        } else {
                            RemuxEngine::Ffmpeg
                        };
                    },
                ),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(field_label("Destination", &t))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.0))
                            .child(div().flex_1().min_w_0().child(destination))
                            .child(
                                button("rmx-browse", &t, BtnKind::Default, true)
                                    .h(px(32.0))
                                    .child("Browse")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.pick(Pick::RemuxDestination, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(t.faint)
                            .child("Folder inputs mirror their subfolders."),
                    ),
            )
            .child(field(
                "If the output exists",
                &t,
                self.select(
                    cx,
                    "rmx-collision",
                    POLICIES.map(|(name, _)| name),
                    collision_index,
                    false,
                    |this, index, _| this.remux.collision = POLICIES[index].1,
                ),
            ))
            .child(self.check(
                cx,
                "rmx-dry",
                self.remux.dry_run,
                "Dry run (log commands only)",
                |this, on, _| this.remux.dry_run = on,
            ))
            .child(warning_card)
            .child(
                div()
                    .flex_1()
                    .justify_end()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        button("remux-go", &t, BtnKind::Primary, false)
                            .w_full()
                            .child(if count == 1 {
                                "Remux 1 file".to_owned()
                            } else {
                                format!("Remux {count} files")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.queue_remux(cx) > 0 {
                                    this.go(Page::Queue, cx);
                                    this.start_queue(cx);
                                }
                            })),
                    )
                    .child(
                        button("remux-queue", &t, BtnKind::Default, false)
                            .w_full()
                            .child("Add to queue")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.queue_remux(cx);
                            })),
                    ),
            );

        div()
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                page_scroll("remux-page")
                    .child(page_title(
                        "Remux without re-encoding",
                        "Keep, drop, retag and add tracks. Add external subtitles. Change container. Lossless and fast.",
                        &t,
                    ))
                    .child(languages)
                    .child(tracks),
            )
            .child(side)
            .into_any_element()
    }
}
