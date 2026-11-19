//! Transcode: source list, eight settings tabs and the encode inspector.

use gpui::{
    AnyElement, Context, Div, FontWeight, SharedString, Stateful, Window, div, prelude::*, px,
};
use kestrel_core::formats::Container;

use super::{grid, inspector, summary_row};
use crate::app::{Kestrel, Pick};
use crate::ctl::InputCfg;
use crate::state::Page;
use crate::state::format::format_size;
use crate::state::probe::{chapters_to_csv, timestamp};
use crate::state::transcode::*;
use crate::widgets::controls::{chip, segmented, slider, tab_bar};
use crate::widgets::{
    BtnKind, Col, TagKind, body_row, button, card, field, header_row, icon, kbd, mono,
    scroll_table, section, section_title, tag, thumb,
};

const AUDIO_COLS: [Col; 8] = [
    Col::grow(1.7),
    Col::grow(1.5),
    Col::grow(1.4),
    Col::fixed(112.0),
    Col::fixed(88.0),
    Col::fixed(78.0),
    Col::fixed(78.0),
    Col::fixed(48.0),
];
const SUB_COLS: [Col; 7] = [
    Col::grow(2.0),
    Col::fixed(90.0),
    Col::fixed(96.0),
    Col::fixed(80.0),
    Col::fixed(80.0),
    Col::fixed(110.0),
    Col::fixed(48.0),
];
const CHAPTER_COLS: [Col; 4] = [
    Col::fixed(56.0),
    Col::fixed(130.0),
    Col::fixed(130.0),
    Col::grow(1.0),
];

fn minutes_label(minutes: u64) -> String {
    if minutes >= 60 {
        format!("{} h {:02} min", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}

impl Kestrel {
    // ----- field helpers -----

    fn fsel(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        label: &'static str,
        options: &[&'static str],
        selected: usize,
        on: impl Fn(&mut Kestrel, usize) + 'static,
    ) -> Div {
        let t = self.theme();
        field(
            label,
            &t,
            self.select(
                cx,
                id,
                options.iter().copied(),
                selected,
                false,
                move |this, index, _| on(this, index),
            ),
        )
    }

    fn fnum(
        &mut self,
        cx: &mut Context<Self>,
        window: &mut Window,
        id: &'static str,
        label: &'static str,
        value: i64,
        on: impl Fn(&mut Kestrel, i64) + 'static,
    ) -> Div {
        let t = self.theme();
        let input = self.field_number(
            cx,
            window,
            id,
            value,
            InputCfg::default(),
            move |this, number, _| on(this, number),
        );
        field(label, &t, input)
    }

    #[allow(clippy::too_many_arguments)]
    fn ftext(
        &mut self,
        cx: &mut Context<Self>,
        window: &mut Window,
        id: &'static str,
        label: &'static str,
        value: &str,
        cfg: InputCfg,
        on: impl Fn(&mut Kestrel, &str) + 'static,
    ) -> Div {
        let t = self.theme();
        let input = self.field_text(cx, window, id, value, cfg, move |this, text, _, _| {
            on(this, text);
        });
        field(label, &t, input)
    }

    fn fcheck(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        text: &'static str,
        checked: bool,
        on: impl Fn(&mut Kestrel, bool) + 'static,
    ) -> Stateful<Div> {
        self.check(cx, id, checked, text, move |this, value, _| on(this, value))
    }

    fn seg(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        options: &[&'static str],
        selected: usize,
        on: impl Fn(&mut Kestrel, usize) + 'static,
    ) -> Div {
        let t = self.theme();
        segmented(
            cx,
            id,
            options.iter().copied(),
            selected,
            &t,
            move |this, index, _| on(this, index),
        )
    }

    fn pair_select(
        &self,
        cx: &mut Context<Self>,
        ids: [&'static str; 2],
        label: &'static str,
        first: (&[&'static str], usize),
        second: (&[&'static str], usize),
        on: impl Fn(&mut Kestrel, usize, bool) + Clone + 'static,
    ) -> Div {
        let t = self.theme();
        let on_first = on.clone();
        field(
            label,
            &t,
            div()
                .flex()
                .gap(px(8.0))
                .child(div().flex_1().min_w_0().child(self.select(
                    cx,
                    ids[0],
                    first.0.iter().copied(),
                    first.1,
                    false,
                    move |this, index, _| on_first(this, index, false),
                )))
                .child(div().flex_1().min_w_0().child(self.select(
                    cx,
                    ids[1],
                    second.0.iter().copied(),
                    second.1,
                    false,
                    move |this, index, _| on(this, index, true),
                ))),
        )
    }

    fn current_meta(&self) -> Option<kestrel_core::mkv::MediaMetadata> {
        self.library
            .current_source()
            .and_then(|source| source.metadata().cloned())
    }

    fn add_audio_track(&mut self, all: bool, cx: &mut Context<Self>) {
        let Some(meta) = self.current_meta() else {
            self.toast("Add a source first", TagKind::Warn, cx);
            return;
        };
        if all {
            self.form.audio_tracks = (0..meta.audio_tracks.len())
                .filter_map(|index| self.form.source_audio_track(&meta, index))
                .collect();
            return;
        }
        let next = (0..meta.audio_tracks.len()).find_map(|index| {
            self.form.source_audio_track(&meta, index).filter(|row| {
                !self
                    .form
                    .audio_tracks
                    .iter()
                    .any(|have| have.source == row.source)
            })
        });
        match next {
            Some(row) => self.form.audio_tracks.push(row),
            None => self.toast(
                "Every source audio track is already listed",
                TagKind::Info,
                cx,
            ),
        }
    }

    fn add_sub_track(&mut self, cx: &mut Context<Self>) {
        let Some(meta) = self.current_meta() else {
            self.toast("Add a source first", TagKind::Warn, cx);
            return;
        };
        let next = (0..meta.subtitle_tracks.len()).find_map(|index| {
            self.form.source_sub_track(&meta, index).filter(|row| {
                !self
                    .form
                    .sub_tracks
                    .iter()
                    .any(|have| have.source == row.source)
            })
        });
        match next {
            Some(row) => self.form.sub_tracks.push(row),
            None => self.toast(
                "Every source subtitle track is already listed",
                TagKind::Info,
                cx,
            ),
        }
    }

    // ----- page -----

    pub fn render_transcode(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let sources = self.render_transcode_sources(cx);
        let centre = self.render_transcode_centre(window, cx);
        let side = self.render_transcode_inspector(window, cx);
        let _ = t;
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(sources)
            .child(centre)
            .child(side)
            .into_any_element()
    }

    fn render_transcode_sources(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let current = self.library.current;
        let items: Vec<AnyElement> = self
            .library
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let on = index == current;
                div()
                    .id(SharedString::from(format!("tr-src-{index}")))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .p(px(8.0))
                    .rounded(px(8.0))
                    .cursor_pointer()
                    .border_1()
                    .map(|this| {
                        if on {
                            this.bg(t.accent_soft).border_color(t.accent)
                        } else {
                            this.border_color(gpui::transparent_black())
                                .hover(move |s| s.bg(t.hover))
                        }
                    })
                    .child(thumb(&t))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(source.name.clone()),
                            )
                            .child(
                                mono(source.short_meta())
                                    .text_size(px(11.0))
                                    .text_color(t.faint)
                                    .truncate(),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_source(index, cx)))
                    .into_any_element()
            })
            .collect();
        let empty = items.is_empty();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(244.0))
            .h_full()
            .bg(t.panel)
            .border_r_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .px(px(12.0))
                    .pt(px(12.0))
                    .pb(px(8.0))
                    .child(crate::widgets::label("Sources", &t))
                    .child(div().flex_1().flex().justify_end().child(tag(
                        self.library.sources.len().to_string(),
                        TagKind::Plain,
                        &t,
                    ))),
            )
            .child(
                div()
                    .id("tr-sources")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(8.0))
                    .pb(px(8.0))
                    .gap(px(2.0))
                    .children(items)
                    .when(empty, |this| {
                        this.child(
                            div()
                                .p(px(10.0))
                                .text_size(px(12.0))
                                .text_color(t.faint)
                                .child("Add files in the Library to encode them."),
                        )
                    }),
            )
    }

    fn render_transcode_centre(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let source = self.library.current_source();
        let title = source.map_or_else(|| "No source".to_owned(), |source| source.name.clone());
        let mut tags: Vec<AnyElement> = Vec::new();
        if let Some(source) = source {
            tags.push(tag(source.ext.clone(), TagKind::Plain, &t).into_any_element());
            if let Some(res) = source.resolution() {
                tags.push(tag(res, TagKind::Plain, &t).into_any_element());
            }
            if let Some(codec) = source.video_codec() {
                tags.push(tag(codec, TagKind::Plain, &t).into_any_element());
            }
            if let Some(fps) = source.fps_label() {
                tags.push(tag(format!("{fps} fps"), TagKind::Plain, &t).into_any_element());
            }
            if source.metadata().is_some() {
                let hdr = source.hdr_label();
                tags.push(
                    tag(
                        hdr,
                        if hdr == "HDR" {
                            TagKind::Warn
                        } else {
                            TagKind::Plain
                        },
                        &t,
                    )
                    .into_any_element(),
                );
            }
            tags.push(tag(source.duration_label(), TagKind::Plain, &t).into_any_element());
            tags.push(tag(source.size_label(), TagKind::Plain, &t).into_any_element());
        }
        let preset_names: Vec<String> = self
            .presets
            .iter()
            .map(|preset| preset.name.clone())
            .collect();
        let preset_select = self.select(
            cx,
            "tr-preset",
            preset_names,
            self.selected_preset_index(),
            false,
            |this, index, cx| this.load_preset(index, cx),
        );
        let header = div()
            .flex_none()
            .px(px(22.0))
            .pt(px(16.0))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap(px(12.0))
                    .child(
                        div()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(6.0))
                                    .mt(px(6.0))
                                    .children(tags),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_end()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(t.muted)
                                    .child("Preset"),
                            )
                            .child(div().w(px(220.0)).child(preset_select))
                            .child(
                                button("save-preset", &t, BtnKind::Default, true)
                                    .child("Save as...")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_save_dialog(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div().mt(px(12.0)).child(tab_bar(
                    cx,
                    &Tab::ALL.map(Tab::label),
                    Tab::ALL
                        .iter()
                        .position(|tab| *tab == self.tab)
                        .unwrap_or(0),
                    &t,
                    |this, index, _| this.tab = Tab::ALL[index],
                )),
            );
        let body = match self.tab {
            Tab::Summary => self.tab_summary(cx),
            Tab::Dimensions => self.tab_dimensions(window, cx),
            Tab::Filters => self.tab_filters(cx),
            Tab::Video => self.tab_video(window, cx),
            Tab::Audio => self.tab_audio(window, cx),
            Tab::Subtitles => self.tab_subtitles(window, cx),
            Tab::Chapters => self.tab_chapters(window, cx),
            Tab::Tags => self.tab_tags(window, cx),
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(header)
            .child(
                div()
                    .id("tr-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
    }

    // ----- tabs -----

    fn tab_summary(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let form = &self.form;
        let container_index = CONTAINERS
            .iter()
            .position(|container| *container == form.container)
            .unwrap_or(0);
        let containers = self.seg(
            cx,
            "tr-container",
            &CONTAINERS.map(Container::label),
            container_index,
            |this, index| this.form.set_container(CONTAINERS[index]),
        );
        let (web, align, passthru, attach) = (
            form.web_optimized,
            form.align_av,
            form.passthru_metadata,
            form.keep_attachments,
        );
        let video = form.video_summary();
        let audio = form.audio_summary();
        let subs = form.subtitle_summary();
        let preview_length = form.preview_length;
        let format = div()
            .child(section_title("Format", &t))
            .child(div().mt(px(10.0)).child(containers))
            .child(
                grid(2)
                    .mt(px(16.0))
                    .child(self.fcheck(
                        cx,
                        "tr-web",
                        "Web optimized (fast start)",
                        web,
                        |this, on| {
                            this.form.web_optimized = on;
                        },
                    ))
                    .child(
                        self.fcheck(cx, "tr-align", "Align A/V start", align, |this, on| {
                            this.form.align_av = on;
                        }),
                    )
                    .child(self.fcheck(
                        cx,
                        "tr-passthru-meta",
                        "Passthru common metadata",
                        passthru,
                        |this, on| this.form.passthru_metadata = on,
                    ))
                    .child(self.fcheck(
                        cx,
                        "tr-attach",
                        "Keep attachments (fonts, cover art)",
                        attach,
                        |this, on| this.form.keep_attachments = on,
                    )),
            );
        let (from, to) = if t.light {
            (0xD9DCE3, 0xF1F2F5)
        } else {
            (0x1D2230, 0x0D0F14)
        };
        let preview = card(&t)
            .mt(px(14.0))
            .p(px(14.0))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(14.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(220.0))
                    .child(crate::widgets::label("Preview", &t))
                    .child(
                        div()
                            .mt(px(8.0))
                            .h(px(120.0))
                            .rounded(px(8.0))
                            .flex()
                            .items_end()
                            .p(px(8.0))
                            .text_size(px(11.0))
                            .text_color(t.muted)
                            .bg(gpui::linear_gradient(
                                120.0,
                                gpui::linear_color_stop(gpui::rgb(from), 0.0),
                                gpui::linear_color_stop(gpui::rgb(to), 1.0),
                            ))
                            .font_family(crate::widgets::MONO)
                            .child("Source / encoded split preview"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .w(px(160.0))
                    .child(self.fsel(
                        cx,
                        "tr-preview-len",
                        "Preview length",
                        &PREVIEW_LENGTHS,
                        preview_length,
                        |this, index| this.form.preview_length = index,
                    ))
                    .child(
                        button("tr-preview-go", &t, BtnKind::Default, true)
                            .child("Encode preview")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toast(
                                    "Preview encoding arrives with the 4.1 engine",
                                    TagKind::Info,
                                    cx,
                                );
                            })),
                    ),
            );
        let output = div()
            .child(section_title("Output", &t))
            .child(
                grid(3)
                    .mt(px(10.0))
                    .child(crate::widgets::stat_card(
                        "Video",
                        mono(video).text_size(px(12.0)),
                        &t,
                    ))
                    .child(crate::widgets::stat_card(
                        "Audio",
                        mono(audio).text_size(px(12.0)),
                        &t,
                    ))
                    .child(crate::widgets::stat_card(
                        "Subtitles",
                        mono(subs).text_size(px(12.0)),
                        &t,
                    )),
            )
            .child(preview);
        div()
            .child(section(&t, false).child(format))
            .child(section(&t, true).child(output))
    }

    fn tab_dimensions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let form = self.form.clone();
        let dims = self.library.current_source().and_then(|source| {
            let video = source.metadata()?.video.as_ref()?;
            Some((video.width?, video.height?))
        });
        let custom_crop = form.crop_mode == 3;
        let crop_fields: Vec<AnyElement> =
            [("Top", 0usize), ("Bottom", 1), ("Left", 2), ("Right", 3)]
                .into_iter()
                .map(|(name, slot)| {
                    let id = [
                        "tr-crop-top",
                        "tr-crop-bottom",
                        "tr-crop-left",
                        "tr-crop-right",
                    ][slot];
                    let input = self.field_number(
                        cx,
                        window,
                        id,
                        i64::from(form.crop[slot]),
                        InputCfg::default().disabled(!custom_crop),
                        move |this, number, _| this.form.crop[slot] = number.max(0) as u32,
                    );
                    field(name, &t, input).into_any_element()
                })
                .collect();
        let orientation = div()
            .child(section_title("Orientation and cropping", &t))
            .child(
                grid(4)
                    .mt(px(10.0))
                    .child(
                        self.fsel(cx, "tr-flip", "Flip", &FLIPS, form.flip, |this, i| {
                            this.form.flip = i;
                        }),
                    )
                    .child(self.fsel(
                        cx,
                        "tr-rot",
                        "Rotation",
                        &ROTATIONS,
                        form.rotation,
                        |this, i| this.form.rotation = i,
                    ))
                    .child(
                        field(
                            "Cropping",
                            &t,
                            self.seg(
                                cx,
                                "tr-crop-mode",
                                &CROP_MODES,
                                form.crop_mode,
                                |this, i| {
                                    this.form.crop_mode = i;
                                },
                            ),
                        )
                        .col_span(4),
                    )
                    .children(crop_fields),
            );
        let pixel_aspect = self.ftext(
            cx,
            window,
            "tr-par",
            "Pixel aspect",
            &form.pixel_aspect,
            InputCfg::default().mono(),
            |this, text| this.form.pixel_aspect = text.to_owned(),
        );
        let scaled_w = self.fnum(
            cx,
            window,
            "tr-scaled-w",
            "Scaled width",
            i64::from(form.scaled_width),
            |this, n| this.form.scaled_width = n.max(0) as u32,
        );
        let scaled_h = self.fnum(
            cx,
            window,
            "tr-scaled-h",
            "Scaled height",
            i64::from(form.scaled_height),
            |this, n| this.form.scaled_height = n.max(0) as u32,
        );
        let resolution = div()
            .child(section_title("Resolution and scaling", &t))
            .child(
                grid(4)
                    .mt(px(10.0))
                    .child(self.fsel(
                        cx,
                        "tr-res-limit",
                        "Resolution limit",
                        &RES_LIMITS,
                        form.res_limit,
                        |this, i| this.form.res_limit = i,
                    ))
                    .child(self.fsel(
                        cx,
                        "tr-anamorphic",
                        "Anamorphic",
                        &ANAMORPHIC,
                        form.anamorphic,
                        |this, i| this.form.anamorphic = i,
                    ))
                    .child(pixel_aspect)
                    .child(self.fsel(
                        cx,
                        "tr-modulus",
                        "Modulus",
                        &MODULUS,
                        form.modulus,
                        |this, i| {
                            this.form.modulus = i;
                        },
                    ))
                    .child(scaled_w)
                    .child(scaled_h)
                    .child(div().flex().items_end().child(self.fcheck(
                        cx,
                        "tr-optimal",
                        "Optimal size",
                        form.optimal_size,
                        |this, on| this.form.optimal_size = on,
                    )))
                    .child(div().flex().items_end().child(self.fcheck(
                        cx,
                        "tr-upscale",
                        "Allow upscaling",
                        form.allow_upscale,
                        |this, on| this.form.allow_upscale = on,
                    ))),
            );
        let borders = div().child(section_title("Borders", &t)).child(
            grid(4)
                .mt(px(10.0))
                .child(self.fsel(
                    cx,
                    "tr-border-fill",
                    "Fill",
                    &BORDER_FILLS,
                    form.border_fill,
                    |this, i| this.form.border_fill = i,
                ))
                .child(self.fsel(
                    cx,
                    "tr-border-colour",
                    "Colour",
                    &BORDER_COLORS,
                    form.border_color,
                    |this, i| this.form.border_color = i,
                ))
                .child(
                    field(
                        "Final dimensions",
                        &t,
                        div()
                            .w_full()
                            .min_h(px(32.0))
                            .py(px(6.0))
                            .text_size(px(12.0))
                            .text_color(t.muted)
                            .font_family(crate::widgets::MONO)
                            .child(form.final_dimensions(dims)),
                    )
                    .col_span(2),
                ),
        );
        div()
            .child(section(&t, false).child(orientation))
            .child(section(&t, false).child(resolution))
            .child(section(&t, true).child(borders))
    }

    fn tab_filters(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let form = self.form.clone();
        let filters = grid(3)
            .child(self.fsel(
                cx,
                "tr-detelecine",
                "Detelecine",
                &DETELECINE,
                form.detelecine,
                |this, i| {
                    this.form.detelecine = i;
                },
            ))
            .child(self.fsel(
                cx,
                "tr-interlace",
                "Interlace detection",
                &INTERLACE_DETECT,
                form.interlace_detect,
                |this, i| this.form.interlace_detect = i,
            ))
            .child(self.pair_select(
                cx,
                ["tr-deint", "tr-deint-preset"],
                "Deinterlace",
                (&DEINTERLACE, form.deinterlace),
                (&DEINTERLACE_PRESETS, form.deinterlace_preset),
                |this, i, second| {
                    if second {
                        this.form.deinterlace_preset = i;
                    } else {
                        this.form.deinterlace = i;
                    }
                },
            ))
            .child(self.pair_select(
                cx,
                ["tr-denoise", "tr-denoise-preset"],
                "Denoise",
                (&DENOISE, form.denoise),
                (&DENOISE_PRESETS, form.denoise_preset),
                |this, i, second| {
                    if second {
                        this.form.denoise_preset = i;
                    } else {
                        this.form.denoise = i;
                    }
                },
            ))
            .child(self.fsel(
                cx,
                "tr-denoise-tune",
                "Denoise tune",
                &DENOISE_TUNES,
                form.denoise_tune,
                |this, i| this.form.denoise_tune = i,
            ))
            .child(self.fsel(
                cx,
                "tr-chroma",
                "Chroma smooth",
                &CHROMA_SMOOTH,
                form.chroma_smooth,
                |this, i| this.form.chroma_smooth = i,
            ))
            .child(self.pair_select(
                cx,
                ["tr-sharpen", "tr-sharpen-preset"],
                "Sharpen",
                (&SHARPEN, form.sharpen),
                (&SHARPEN_PRESETS, form.sharpen_preset),
                |this, i, second| {
                    if second {
                        this.form.sharpen_preset = i;
                    } else {
                        this.form.sharpen = i;
                    }
                },
            ))
            .child(self.fsel(
                cx,
                "tr-deblock",
                "Deblock",
                &DEBLOCK,
                form.deblock,
                |this, i| {
                    this.form.deblock = i;
                },
            ))
            .child(self.fsel(
                cx,
                "tr-colourspace",
                "Colourspace",
                &COLOURSPACES,
                form.colourspace,
                |this, i| this.form.colourspace = i,
            ));
        let hdr = div().child(section_title("Colour and HDR", &t)).child(
            grid(3)
                .mt(px(10.0))
                .child(self.fsel(
                    cx,
                    "tr-hdr",
                    "HDR handling",
                    &HDR_MODES,
                    form.hdr,
                    |this, i| {
                        this.form.hdr = i;
                    },
                ))
                .child(self.fsel(
                    cx,
                    "tr-tonemap",
                    "Tone-mapping operator",
                    &TONEMAPS,
                    form.tonemap,
                    |this, i| this.form.tonemap = i,
                ))
                .child(div().flex().items_end().child(self.fcheck(
                    cx,
                    "tr-gray",
                    "Grayscale",
                    form.grayscale,
                    |this, on| this.form.grayscale = on,
                ))),
        );
        div()
            .child(section(&t, false).child(filters))
            .child(section(&t, true).child(hdr))
    }

    fn tab_video(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let form = self.form.clone();
        let quality_tag = |text: String| {
            tag(text, TagKind::Acc, &t)
                .h(px(26.0))
                .min_w(px(70.0))
                .justify_center()
                .text_size(px(13.0))
        };
        let side_label = |text: &'static str, right: bool| {
            div()
                .w(px(96.0))
                .flex_none()
                .whitespace_nowrap()
                .text_size(px(12.0))
                .text_color(t.faint)
                .when(right, |this| this.text_right())
                .child(text)
        };
        let top = grid(3)
            .child(self.fsel(
                cx,
                "tr-encoder",
                "Video encoder",
                &ENCODERS,
                form.encoder,
                |this, i| this.form.encoder = i,
            ))
            .child(self.fsel(
                cx,
                "tr-fps",
                "Frame rate (FPS)",
                &FRAME_RATES,
                form.frame_rate,
                |this, i| this.form.frame_rate = i,
            ))
            .child(
                field(
                    "Frame rate mode",
                    &t,
                    self.seg(cx, "tr-fps-mode", &FPS_MODES, form.fps_mode, |this, i| {
                        this.form.fps_mode = i;
                    }),
                )
                .col_span(3),
            );
        let rate_seg = self.seg(
            cx,
            "tr-rate-mode",
            &RATE_MODES,
            form.rate_mode,
            |this, i| {
                this.form.rate_mode = i;
            },
        );
        let quality_body = if form.is_constant_quality() {
            div()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.0))
                        .child(side_label("Higher quality", false))
                        .child(slider(
                            cx,
                            "tr-rf",
                            f32::from(form.rf) / 51.0,
                            &t,
                            |this, fraction, _| this.form.rf = (fraction * 51.0).round() as u8,
                        ))
                        .child(side_label("Smaller file", true))
                        .child(quality_tag(format!("{} {}", form.quality_label(), form.rf))),
                )
                .child(
                    div()
                        .mt(px(8.0))
                        .text_size(px(12.0))
                        .text_color(t.faint)
                        .child(form.quality_hint()),
                )
        } else {
            let bitrate = self.fnum(
                cx,
                window,
                "tr-bitrate",
                "Average bitrate (kbit/s)",
                i64::from(form.bitrate_kbps),
                |this, n| this.form.bitrate_kbps = n.max(0) as u32,
            );
            div().child(
                grid(3)
                    .child(bitrate)
                    .child(div().flex().items_end().child(self.fcheck(
                        cx,
                        "tr-multipass",
                        "Multi-pass encoding",
                        form.multipass,
                        |this, on| this.form.multipass = on,
                    )))
                    .child(div().flex().items_end().child(self.fcheck(
                        cx,
                        "tr-turbo",
                        "Turbo analysis pass",
                        form.turbo,
                        |this, on| this.form.turbo = on,
                    ))),
            )
        };
        let quality = div()
            .child(
                div()
                    .flex()
                    .items_center()
                    .mb(px(14.0))
                    .child(section_title("Quality", &t))
                    .child(div().flex_1().flex().justify_end().child(rate_seg)),
            )
            .child(quality_body);
        let advanced = self.ftext(
            cx,
            window,
            "tr-advanced",
            "Advanced options",
            &form.advanced,
            InputCfg::default().mono().ph("key=value:key=value"),
            |this, text| this.form.advanced = text.to_owned(),
        );
        let options = div()
            .child(section_title("Encoder options", &t))
            .child(
                div()
                    .mt(px(10.0))
                    .mb(px(16.0))
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .child(side_label("Faster", false))
                    .child(slider(
                        cx,
                        "tr-speed",
                        f32::from(form.speed) / 9.0,
                        &t,
                        |this, fraction, _| this.form.speed = (fraction * 9.0).round() as u8,
                    ))
                    .child(side_label("Smaller", true))
                    .child(quality_tag(form.speed_label())),
            )
            .child(
                grid(4)
                    .child(self.fsel(
                        cx,
                        "tr-tune",
                        "Tune",
                        &VIDEO_TUNES,
                        form.video_tune,
                        |this, i| {
                            this.form.video_tune = i;
                        },
                    ))
                    .child(self.fsel(
                        cx,
                        "tr-profile",
                        "Profile",
                        &VIDEO_PROFILES,
                        form.video_profile,
                        |this, i| this.form.video_profile = i,
                    ))
                    .child(self.fsel(
                        cx,
                        "tr-level",
                        "Level",
                        &VIDEO_LEVELS,
                        form.video_level,
                        |this, i| {
                            this.form.video_level = i;
                        },
                    ))
                    .child(div().flex().items_end().child(self.fcheck(
                        cx,
                        "tr-fast-decode",
                        "Fast decode",
                        form.fast_decode,
                        |this, on| this.form.fast_decode = on,
                    ))),
            )
            .child(div().mt(px(14.0)).child(advanced));
        div()
            .child(section(&t, false).child(top))
            .child(section(&t, false).child(quality))
            .child(section(&t, true).child(options))
    }

    fn tab_audio(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let form = self.form.clone();
        let bitrates: Vec<String> = AUDIO_BITRATES.iter().map(u32::to_string).collect();
        let rows: Vec<AnyElement> = form
            .audio_tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                let gain = self.field_number(
                    cx,
                    window,
                    format!("tr-gain-{index}"),
                    i64::from(track.gain),
                    InputCfg::default().compact(),
                    move |this, n, _| {
                        if let Some(row) = this.form.audio_tracks.get_mut(index) {
                            row.gain = n as i32;
                        }
                    },
                );
                let drc = self.field_number(
                    cx,
                    window,
                    format!("tr-drc-{index}"),
                    i64::from(track.drc),
                    InputCfg::default().compact(),
                    move |this, n, _| {
                        if let Some(row) = this.form.audio_tracks.get_mut(index) {
                            row.drc = n as i32;
                        }
                    },
                );
                let cells = vec![
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(track.source.clone()),
                        )
                        .child(
                            mono(track.source_meta.clone())
                                .text_size(px(11.0))
                                .text_color(t.faint)
                                .truncate(),
                        )
                        .into_any_element(),
                    self.select(
                        cx,
                        format!("tr-aenc-{index}"),
                        AUDIO_ENCODERS,
                        track.encoder,
                        true,
                        move |this, i, _| {
                            if let Some(row) = this.form.audio_tracks.get_mut(index) {
                                row.encoder = i;
                            }
                        },
                    )
                    .into_any_element(),
                    self.select(
                        cx,
                        format!("tr-amix-{index}"),
                        MIXDOWNS,
                        track.mixdown,
                        true,
                        move |this, i, _| {
                            if let Some(row) = this.form.audio_tracks.get_mut(index) {
                                row.mixdown = i;
                            }
                        },
                    )
                    .into_any_element(),
                    self.select(
                        cx,
                        format!("tr-arate-{index}"),
                        SAMPLE_RATES,
                        track.sample_rate,
                        true,
                        move |this, i, _| {
                            if let Some(row) = this.form.audio_tracks.get_mut(index) {
                                row.sample_rate = i;
                            }
                        },
                    )
                    .into_any_element(),
                    self.select(
                        cx,
                        format!("tr-abr-{index}"),
                        bitrates.clone(),
                        track.bitrate,
                        true,
                        move |this, i, _| {
                            if let Some(row) = this.form.audio_tracks.get_mut(index) {
                                row.bitrate = i;
                            }
                        },
                    )
                    .into_any_element(),
                    div().w_full().child(gain).into_any_element(),
                    div().w_full().child(drc).into_any_element(),
                    button(format!("tr-aremove-{index}"), &t, BtnKind::Ghost, true)
                        .child(icon("x", 12.0, t.muted))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if index < this.form.audio_tracks.len() {
                                this.form.audio_tracks.remove(index);
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                ];
                body_row(&t, &AUDIO_COLS, cells, index + 1 == form.audio_tracks.len())
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let tracks = div()
            .child(
                div()
                    .flex()
                    .items_center()
                    .mb(px(12.0))
                    .child(section_title("Tracks", &t))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_end()
                            .flex()
                            .gap(px(6.0))
                            .child(
                                button("tr-add-audio", &t, BtnKind::Default, true)
                                    .child("Add track")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.add_audio_track(false, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("tr-add-all-audio", &t, BtnKind::Default, true)
                                    .child("Add all")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.add_audio_track(true, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("tr-clear-audio", &t, BtnKind::Ghost, true)
                                    .child("Clear")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.form.audio_tracks.clear();
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                card(&t)
                    .child(scroll_table(
                        "tr-audio-scroll",
                        900.0,
                        std::iter::once(
                            header_row(
                                &t,
                                &AUDIO_COLS,
                                &[
                                    "Source",
                                    "Encoder",
                                    "Mixdown",
                                    "Sample rate",
                                    "Bitrate",
                                    "Gain",
                                    "DRC",
                                    "",
                                ],
                            )
                            .into_any_element(),
                        )
                        .chain(rows)
                        .collect(),
                    ))
                    .when(empty, |this| {
                        this.child(
                            div()
                                .py(px(22.0))
                                .flex()
                                .justify_center()
                                .text_color(t.faint)
                                .child("No audio tracks. Use Add track or Add all."),
                        )
                    }),
            );
        let languages = self.ftext(
            cx,
            window,
            "tr-languages",
            "Languages",
            &form.languages,
            InputCfg::default().mono(),
            |this, text| this.form.languages = text.to_owned(),
        );
        let passthru_chips: Vec<AnyElement> = PASSTHRU_FORMATS
            .iter()
            .enumerate()
            .map(|(index, name)| {
                chip(
                    SharedString::from(format!("tr-pt-{index}")),
                    *name,
                    None,
                    form.passthru_allowed[index],
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.form.passthru_allowed[index] = !this.form.passthru_allowed[index];
                    cx.notify();
                }))
                .into_any_element()
            })
            .collect();
        let selection = div()
            .child(section_title("Selection behaviour", &t))
            .child(
                grid(3)
                    .mt(px(10.0))
                    .child(self.fsel(
                        cx,
                        "tr-asel",
                        "Track selection",
                        &AUDIO_SELECTION,
                        form.audio_selection,
                        |this, i| this.form.audio_selection = i,
                    ))
                    .child(languages)
                    .child(self.fsel(
                        cx,
                        "tr-afallback",
                        "Passthru fallback",
                        &PASSTHRU_FALLBACKS,
                        form.passthru_fallback,
                        |this, i| this.form.passthru_fallback = i,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.0))
                    .mt(px(14.0))
                    .child(
                        div()
                            .mr(px(6.0))
                            .text_size(px(12.0))
                            .text_color(t.muted)
                            .child("Allow passthru:"),
                    )
                    .children(passthru_chips),
            );
        div()
            .child(section(&t, false).child(tracks))
            .child(section(&t, true).child(selection))
    }

    fn tab_subtitles(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let form = self.form.clone();
        let rows: Vec<AnyElement> = form
            .sub_tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                let offset = self.field_number(
                    cx,
                    window,
                    format!("tr-soff-{index}"),
                    i64::from(track.offset_ms),
                    InputCfg::default().compact(),
                    move |this, n, _| {
                        if let Some(row) = this.form.sub_tracks.get_mut(index) {
                            row.offset_ms = n as i32;
                        }
                    },
                );
                let cells = vec![
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(track.source.clone()),
                        )
                        .child(tag(
                            track.kind.clone(),
                            if track.external.is_some() {
                                TagKind::Acc
                            } else {
                                TagKind::Plain
                            },
                            &t,
                        ))
                        .into_any_element(),
                    mono(track.language.clone()).into_any_element(),
                    self.check_box(
                        cx,
                        format!("tr-sforced-{index}"),
                        track.forced_only,
                        move |this, on, _| {
                            if let Some(row) = this.form.sub_tracks.get_mut(index) {
                                row.forced_only = on;
                            }
                        },
                    )
                    .into_any_element(),
                    self.check_box(
                        cx,
                        format!("tr-sburn-{index}"),
                        track.burn,
                        move |this, on, _| {
                            if let Some(row) = this.form.sub_tracks.get_mut(index) {
                                row.burn = on;
                            }
                        },
                    )
                    .into_any_element(),
                    self.check_box(
                        cx,
                        format!("tr-sdef-{index}"),
                        track.default,
                        move |this, on, _| {
                            if let Some(row) = this.form.sub_tracks.get_mut(index) {
                                row.default = on;
                            }
                        },
                    )
                    .into_any_element(),
                    div().w_full().child(offset).into_any_element(),
                    button(format!("tr-sremove-{index}"), &t, BtnKind::Ghost, true)
                        .child(icon("x", 12.0, t.muted))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if index < this.form.sub_tracks.len() {
                                this.form.sub_tracks.remove(index);
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                ];
                body_row(&t, &SUB_COLS, cells, index + 1 == form.sub_tracks.len())
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let tracks = div()
            .child(
                div()
                    .flex()
                    .items_center()
                    .mb(px(12.0))
                    .child(section_title("Tracks", &t))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_end()
                            .flex()
                            .gap(px(6.0))
                            .child(
                                button("tr-add-sub", &t, BtnKind::Default, true)
                                    .child("Add track")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.add_sub_track(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("tr-import-sub", &t, BtnKind::Primary, true)
                                    .child("Import SRT / ASS / VTT...")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.pick(Pick::ImportSubtitle, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                card(&t)
                    .child(scroll_table(
                        "tr-sub-scroll",
                        720.0,
                        std::iter::once(
                            header_row(
                                &t,
                                &SUB_COLS,
                                &[
                                    "Source",
                                    "Language",
                                    "Forced only",
                                    "Burn in",
                                    "Default",
                                    "Offset (ms)",
                                    "",
                                ],
                            )
                            .into_any_element(),
                        )
                        .chain(rows)
                        .collect(),
                    ))
                    .when(empty, |this| {
                        this.child(
                            div()
                                .py(px(22.0))
                                .flex()
                                .justify_center()
                                .text_color(t.faint)
                                .child("No subtitle tracks."),
                        )
                    }),
            );
        let selection = div()
            .child(section_title("Selection behaviour", &t))
            .child(
                grid(3)
                    .mt(px(10.0))
                    .child(self.fsel(
                        cx,
                        "tr-ssel",
                        "Track selection",
                        &SUB_SELECTION,
                        form.sub_selection,
                        |this, i| this.form.sub_selection = i,
                    ))
                    .child(self.fsel(
                        cx,
                        "tr-sburn-mode",
                        "Burn-in behaviour",
                        &SUB_BURN,
                        form.sub_burn,
                        |this, i| this.form.sub_burn = i,
                    ))
                    .child(self.fsel(
                        cx,
                        "tr-scharset",
                        "Import character set",
                        &CHARSETS,
                        form.charset,
                        |this, i| this.form.charset = i,
                    )),
            )
            .child(
                grid(2)
                    .mt(px(14.0))
                    .child(self.fcheck(
                        cx,
                        "tr-foreign",
                        "Add foreign audio scan",
                        form.foreign_scan,
                        |this, on| this.form.foreign_scan = on,
                    ))
                    .child(self.fcheck(
                        cx,
                        "tr-cc",
                        "Add closed captions when available",
                        form.closed_captions,
                        |this, on| this.form.closed_captions = on,
                    )),
            );
        div()
            .child(section(&t, false).child(tracks))
            .child(section(&t, true).child(selection))
    }

    fn tab_chapters(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let chapters = self.form.chapters.clone();
        let create = self.form.create_chapters;
        let rows: Vec<AnyElement> = chapters
            .iter()
            .enumerate()
            .map(|(index, chapter)| {
                let name = self.field_text(
                    cx,
                    window,
                    format!("tr-chap-{index}"),
                    &chapter.name,
                    InputCfg::default().compact(),
                    move |this, text, _, _| {
                        if let Some(row) = this.form.chapters.get_mut(index) {
                            row.name = text.to_owned();
                        }
                    },
                );
                let cells = vec![
                    mono((index + 1).to_string())
                        .text_color(t.faint)
                        .into_any_element(),
                    mono(timestamp(chapter.start)).into_any_element(),
                    mono(timestamp(chapter.duration))
                        .text_color(t.muted)
                        .into_any_element(),
                    div().w_full().child(name).into_any_element(),
                ];
                body_row(&t, &CHAPTER_COLS, cells, index + 1 == chapters.len()).into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let csv = chapters_to_csv(&chapters);
        let stem = self
            .library
            .current_source()
            .map(|source| source.stem())
            .unwrap_or_else(|| "chapters".to_owned());
        let body = div()
            .child(
                div()
                    .flex()
                    .items_center()
                    .mb(px(12.0))
                    .child(self.fcheck(
                        cx,
                        "tr-create-chapters",
                        "Create chapter markers",
                        create,
                        |this, on| this.form.create_chapters = on,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_end()
                            .flex()
                            .gap(px(6.0))
                            .child(
                                button("tr-chap-import", &t, BtnKind::Default, true)
                                    .child("Import CSV / XML")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.pick(Pick::Chapters, cx);
                                    })),
                            )
                            .child(
                                button("tr-chap-export", &t, BtnKind::Default, true)
                                    .child("Export")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.export_text(
                                            "Export chapters",
                                            format!("{stem}-chapters.csv"),
                                            csv.clone(),
                                            cx,
                                        );
                                    })),
                            ),
                    ),
            )
            .child(
                card(&t)
                    .child(header_row(
                        &t,
                        &CHAPTER_COLS,
                        &["#", "Start", "Duration", "Name"],
                    ))
                    .children(rows)
                    .when(empty, |this| {
                        this.child(
                            div()
                                .py(px(22.0))
                                .flex()
                                .justify_center()
                                .text_color(t.faint)
                                .child("This source has no chapters. Import a list to add some."),
                        )
                    }),
            );
        div().child(section(&t, true).child(body))
    }

    fn tab_tags(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let form = self.form.clone();
        let title = self.ftext(
            cx,
            window,
            "tr-tag-title",
            "Title",
            &form.tag_title,
            InputCfg::default(),
            |this, text| {
                this.form.tag_title = text.to_owned();
            },
        );
        let actors = self.ftext(
            cx,
            window,
            "tr-tag-actors",
            "Actors",
            &form.tag_actors,
            InputCfg::default(),
            |this, text| {
                this.form.tag_actors = text.to_owned();
            },
        );
        let director = self.ftext(
            cx,
            window,
            "tr-tag-director",
            "Director",
            &form.tag_director,
            InputCfg::default(),
            |this, text| {
                this.form.tag_director = text.to_owned();
            },
        );
        let date = self.ftext(
            cx,
            window,
            "tr-tag-date",
            "Release date",
            &form.tag_date,
            InputCfg::default().ph("YYYY-MM-DD"),
            |this, text| this.form.tag_date = text.to_owned(),
        );
        let genre = self.ftext(
            cx,
            window,
            "tr-tag-genre",
            "Genre",
            &form.tag_genre,
            InputCfg::default(),
            |this, text| {
                this.form.tag_genre = text.to_owned();
            },
        );
        let comment = self.ftext(
            cx,
            window,
            "tr-tag-comment",
            "Comment",
            &form.tag_comment,
            InputCfg::default(),
            |this, text| {
                this.form.tag_comment = text.to_owned();
            },
        );
        let description = self.ftext(
            cx,
            window,
            "tr-tag-description",
            "Description",
            &form.tag_description,
            InputCfg::default(),
            |this, text| this.form.tag_description = text.to_owned(),
        );
        div().child(
            section(&t, true)
                .child(
                    grid(2)
                        .child(title)
                        .child(actors)
                        .child(director)
                        .child(date)
                        .child(genre)
                        .child(comment),
                )
                .child(div().mt(px(14.0)).child(description)),
        )
    }

    // ----- inspector -----

    fn render_transcode_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = self.theme();
        let form = self.form.clone();
        let source = self.library.current_source();
        let (source_size, duration) = source.map_or((0, None), |source| {
            (
                source.size,
                source.metadata().and_then(|meta| meta.duration),
            )
        });
        let dims = source.and_then(|source| {
            let video = source.metadata()?.video.as_ref()?;
            Some((video.width?, video.height?))
        });
        let has_source = source.is_some();
        let (estimate, note) = match self.measured {
            Some(bytes) => (
                format_size(bytes),
                format!("measured on a short sample of {}", format_size(source_size)),
            ),
            None if has_source => (
                format!(
                    "~{}",
                    format_size(heuristic_size(&form, source_size, duration))
                ),
                format!(
                    "estimate from {} \u{b7} about {} on this machine",
                    format_size(source_size),
                    minutes_label(heuristic_minutes(&form, duration))
                ),
            ),
            None => (
                "-".to_owned(),
                "Add a source to estimate the size".to_owned(),
            ),
        };
        let filters = form.filter_summary();
        let dimension_text = form
            .final_dimensions(dims)
            .rsplit("-> ")
            .next()
            .unwrap_or_default()
            .to_owned();
        let summary = card(&t)
            .p(px(12.0))
            .gap(px(8.0))
            .bg(t.raised)
            .child(summary_row("Video", form.video_summary(), &t))
            .child(summary_row("Size", dimension_text, &t))
            .child(summary_row("Filters", filters, &t))
            .child(summary_row(
                "Container",
                form.container.label().to_owned(),
                &t,
            ));
        let save_as = self.ftext(
            cx,
            window,
            "tr-save-as",
            "Save as",
            &form.save_as,
            InputCfg::default().mono(),
            |this, text| this.form.save_as = text.to_owned(),
        );
        let folder_input = self.field_text(
            cx,
            window,
            "tr-folder",
            &form.folder,
            InputCfg::default().mono(),
            |this, text, _, _| this.form.folder = text.to_owned(),
        );
        let folder = field(
            "Folder",
            &t,
            div()
                .flex()
                .gap(px(8.0))
                .child(div().flex_1().min_w_0().child(folder_input))
                .child(
                    button("tr-browse", &t, BtnKind::Default, true)
                        .h(px(32.0))
                        .child("Browse")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pick(Pick::TranscodeFolder, cx);
                        })),
                ),
        );
        let apply_label = format!(
            "Apply to all {} selected sources",
            self.library.selected().len()
        );
        let apply = self.check(
            cx,
            "tr-apply-all",
            form.apply_to_all,
            apply_label,
            |this, on, _| this.form.apply_to_all = on,
        );
        let show_command = self.show_command;
        let command = show_command.then(|| self.ffmpeg_preview());
        let command_block = div()
            .flex()
            .flex_col()
            .text_size(px(12.0))
            .text_color(t.muted)
            .child(
                div()
                    .id("tr-command-toggle")
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .child(icon(
                        if show_command {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        12.0,
                        t.muted,
                    ))
                    .child("FFmpeg command")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_command = !this.show_command;
                        cx.notify();
                    })),
            )
            .children(command.map(|text| {
                let copy = text.clone();
                div()
                    .mt(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(
                        div()
                            .p(px(10.0))
                            .max_h(px(150.0))
                            .overflow_hidden()
                            .rounded(px(8.0))
                            .border_1()
                            .border_color(t.line)
                            .bg(t.bg)
                            .text_size(px(11.0))
                            .text_color(t.text)
                            .font_family(crate::widgets::MONO)
                            .child(text),
                    )
                    .child(
                        div().flex().child(
                            button("tr-copy-command", &t, BtnKind::Ghost, true)
                                .child(icon("copy", 12.0, t.muted))
                                .child("Copy")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        copy.clone(),
                                    ));
                                    this.toast("Command copied", TagKind::Ok, cx);
                                })),
                        ),
                    )
            }));
        inspector(280.0, &t)
            .child(
                div()
                    .child(crate::widgets::label("Estimate", &t))
                    .child(
                        div()
                            .mt(px(4.0))
                            .text_size(px(26.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(estimate),
                    )
                    .child(div().text_size(px(12.0)).text_color(t.muted).child(note))
                    .child(
                        div().flex().mt(px(6.0)).child(
                            button("tr-measure", &t, BtnKind::Default, true)
                                .child("Measure on a sample")
                                .on_click(cx.listener(|this, _, _, cx| this.measure_estimate(cx))),
                        ),
                    ),
            )
            .child(summary)
            .child(save_as)
            .child(folder)
            .child(apply)
            .child(command_block)
            .child(
                div()
                    .flex_1()
                    .justify_end()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        button("tr-start", &t, BtnKind::Primary, false)
                            .w_full()
                            .child("Start encode")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.queue_transcode(cx) > 0 {
                                    this.go(Page::Queue, cx);
                                    this.start_queue(cx);
                                }
                            })),
                    )
                    .child(
                        button("tr-queue", &t, BtnKind::Default, false)
                            .w_full()
                            .child("Add to queue")
                            .child(kbd("Ctrl Enter", &t))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.queue_transcode(cx);
                            })),
                    ),
            )
    }
}
