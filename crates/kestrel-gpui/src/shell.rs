//! Window chrome: sidebar, header, status footer, banners and toasts.

use gpui::{AnyElement, Context, Div, FontWeight, Rgba, Stateful, Window, div, prelude::*, px};
use kestrel_core::tools::Tool;
use kestrel_core::transcode::{HwVendor, VideoEncoder};

use crate::app::{Kestrel, Pick};
use crate::state::Page;
use crate::state::format::{format_clock, tool_version};
use crate::theme::{PRESET_DOTS, RADIUS_CARD, hex};
use crate::widgets::overlay::tooltip;
use crate::widgets::{BtnKind, TagKind, button, card, dot, icon, kbd, label, tag};

const SIDEBAR_WIDTH: f32 = 236.0;
const SIDEBAR_COLLAPSED: f32 = 60.0;

fn page_icon(page: Page) -> &'static str {
    match page {
        Page::Library => "library",
        Page::Remux => "remux",
        Page::Transcode => "transcode",
        Page::Queue => "queue",
        Page::Activity => "activity",
        Page::Metrics => "metrics",
        Page::Settings => "settings",
    }
}

pub fn vendor_label(vendor: HwVendor) -> &'static str {
    match vendor {
        HwVendor::Nvenc => "NVENC",
        HwVendor::Qsv => "QSV",
        HwVendor::Amf => "AMF",
        HwVendor::VideoToolbox => "VideoToolbox",
        HwVendor::Vaapi => "VAAPI",
    }
}

impl Kestrel {
    /// Hardware encoder vendors found by FFmpeg, in detection order without repeats.
    pub fn gpu_vendors(&self) -> Vec<&'static str> {
        let mut vendors = Vec::new();
        if let Some(set) = &self.encoders {
            for encoder in &set.hardware {
                if let VideoEncoder::Hardware { vendor, .. } = encoder {
                    let name = vendor_label(*vendor);
                    if !vendors.contains(&name) {
                        vendors.push(name);
                    }
                }
            }
        }
        vendors
    }

    fn nav_item(
        &self,
        cx: &mut Context<Self>,
        page: Page,
        badge: Option<(String, bool)>,
    ) -> Stateful<Div> {
        let t = self.theme();
        let on = self.page == page;
        let collapsed = self.collapsed;
        div()
            .id(page.label())
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(34.0))
            .w_full()
            .px(px(10.0))
            .rounded(px(8.0))
            .border_1()
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .when(collapsed, |this| this.justify_center().px(px(0.0)))
            .map(|this| {
                if on {
                    this.bg(t.raised).border_color(t.line).text_color(t.text)
                } else {
                    this.border_color(gpui::transparent_black())
                        .text_color(t.muted)
                        .hover(move |s| s.bg(t.hover).text_color(t.text))
                }
            })
            .child(icon(
                page_icon(page),
                16.0,
                if on { t.text } else { t.muted },
            ))
            .when(!collapsed, |this| {
                this.child(page.label())
                    .children(badge.map(|(text, accent)| {
                        div().flex_1().flex().justify_end().child(
                            div()
                                .px(px(7.0))
                                .py(px(1.0))
                                .rounded_full()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .map(|this| {
                                    if accent {
                                        this.bg(t.accent_soft).text_color(t.accent_text)
                                    } else {
                                        this.bg(t.raised)
                                            .text_color(t.faint)
                                            .border_1()
                                            .border_color(t.line)
                                    }
                                })
                                .child(text),
                        )
                    }))
            })
            .when(collapsed, |this| this.tooltip(tooltip(page.label())))
            .on_click(cx.listener(move |this, _, _, cx| this.go(page, cx)))
    }

    pub fn render_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let collapsed = self.collapsed;
        let queue_badge = self.queue.active_count();
        let library_count = self.library.sources.len();

        let mark = gpui::img(gpui::ImageSource::Resource(gpui::Resource::Embedded(
            if t.light {
                crate::assets::LOGO_LIGHT
            } else {
                crate::assets::LOGO
            }
            .into(),
        )))
        .flex_none()
        .size(px(22.0))
        .object_fit(gpui::ObjectFit::Contain);
        let logo_row =
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(8.0))
                .pt(px(2.0))
                .pb(px(14.0))
                .when(collapsed, |this| this.justify_center().px(px(0.0)))
                .child(mark)
                .when(!collapsed, |this| {
                    this.child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Kestrel"),
                    )
                    .child(div().flex_1().flex().justify_end().child(tag(
                        "4.0",
                        TagKind::Plain,
                        &t,
                    )))
                });

        let search = button("open-palette", &t, BtnKind::Default, false)
            .w_full()
            .justify_start()
            .mb(px(14.0))
            .when(collapsed, |this| this.justify_center().px(px(0.0)))
            .child(icon("search", 14.0, t.muted))
            .when(!collapsed, |this| {
                this.child(div().text_color(t.muted).child("Search or run..."))
                    .child(div().flex_1().flex().justify_end().child(kbd("Ctrl K", &t)))
            })
            .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx)));

        let nav = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(self.nav_item(
                cx,
                Page::Library,
                (library_count > 0).then(|| (library_count.to_string(), false)),
            ))
            .child(self.nav_item(cx, Page::Remux, None))
            .child(self.nav_item(cx, Page::Transcode, None))
            .child(self.nav_item(
                cx,
                Page::Queue,
                (queue_badge > 0).then(|| (queue_badge.to_string(), true)),
            ))
            .child(self.nav_item(cx, Page::Activity, None))
            .child(self.nav_item(cx, Page::Metrics, Some(("4.1".to_owned(), false))));

        let presets =
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .children(self.presets.iter().enumerate().map(|(index, preset)| {
                    let color = hex(PRESET_DOTS[index % PRESET_DOTS.len()]);
                    div()
                        .id(("side-preset", index))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .h(px(30.0))
                        .px(px(10.0))
                        .rounded(px(8.0))
                        .text_color(t.muted)
                        .cursor_pointer()
                        .hover(move |s| s.bg(t.hover).text_color(t.text))
                        .when(collapsed, |this| {
                            this.justify_center()
                                .px(px(0.0))
                                .tooltip(tooltip(preset.name.clone()))
                        })
                        .child(dot(color))
                        .when(!collapsed, |this| {
                            this.child(div().min_w_0().truncate().child(preset.name.clone()))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.load_preset_and_open(index, cx);
                        }))
                        .on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                this.open_preset_menu(index, event.position, cx);
                            }),
                        )
                }));

        let tool_row = |name: &'static str, tool: &Option<Tool>| {
            let (color, text) = match tool {
                Some(tool) => (
                    t.ok,
                    tool.version
                        .as_deref()
                        .map_or_else(|| "found".to_owned(), tool_version),
                ),
                None => (t.bad, "missing".to_owned()),
            };
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(px(12.0))
                .child(dot(color))
                .child(name)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .justify_end()
                        .font_family(crate::widgets::MONO)
                        .text_color(t.faint)
                        .child(text),
                )
        };
        let gpu = self.gpu_vendors();
        let (gpu_color, gpu_text) = match (&self.encoders, gpu.is_empty()) {
            (None, _) => (t.faint, "detecting".to_owned()),
            (Some(_), true) => (t.faint, "none".to_owned()),
            (Some(_), false) => (t.info, gpu.join(", ")),
        };
        let status_card = card(&t)
            .px(px(12.0))
            .py(px(10.0))
            .gap(px(6.0))
            .child(tool_row("FFmpeg", &self.tools.ffmpeg))
            .child(tool_row("MKVToolNix", &self.tools.mkvmerge))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(px(12.0))
                    .child(dot(gpu_color))
                    .child("GPU")
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_end()
                            .font_family(crate::widgets::MONO)
                            .text_color(t.faint)
                            .child(gpu_text),
                    ),
            );
        let collapse_button = button("collapse", &t, BtnKind::Ghost, true)
            .tooltip(tooltip(if collapsed {
                "Expand sidebar"
            } else {
                "Collapse sidebar"
            }))
            .child(icon(
                if collapsed {
                    "chevron-right"
                } else {
                    "chevron-left"
                },
                14.0,
                t.muted,
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.collapsed = !this.collapsed;
                cx.notify();
            }));
        let bottom = div()
            .mt_auto()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .when(!collapsed, |this| this.child(status_card))
            .child(
                div()
                    .flex()
                    .when(collapsed, |this| this.flex_col())
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex_1()
                            .w_full()
                            .child(self.nav_item(cx, Page::Settings, None)),
                    )
                    .child(collapse_button),
            );

        div()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .w(px(if collapsed {
                SIDEBAR_COLLAPSED
            } else {
                SIDEBAR_WIDTH
            }))
            .bg(t.panel)
            .border_r_1()
            .border_color(t.line)
            .px(px(10.0))
            .pt(px(14.0))
            .pb(px(10.0))
            .child(logo_row)
            .child(search)
            .child(nav)
            .when(!collapsed, |this| {
                this.child(
                    div()
                        .px(px(10.0))
                        .pt(px(20.0))
                        .pb(px(8.0))
                        .child(label("Presets", &t)),
                )
            })
            .when(collapsed, |this| this.child(div().h(px(14.0))))
            .child(
                div()
                    .id("sidebar-presets")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(presets),
            )
            .child(bottom)
            .into_any_element()
    }

    pub fn render_header(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.0))
            .h(px(52.0))
            .px(px(18.0))
            .border_b_1()
            .border_color(t.line)
            .child(div().text_color(t.faint).child("Kestrel"))
            .child(div().text_color(t.faint).child("/"))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.page.label()),
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
                        button("toggle-theme", &t, BtnKind::Ghost, true)
                            .tooltip(tooltip("Toggle light and dark theme"))
                            .child(icon(if t.light { "moon" } else { "sun" }, 14.0, t.muted))
                            .child(if t.light { "Dark" } else { "Light" })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_theme(cx))),
                    )
                    .child(
                        button("add-media", &t, BtnKind::Default, true)
                            .child(icon("plus", 13.0, t.text))
                            .child("Add media")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.go(Page::Library, cx);
                                this.pick(Pick::Files, cx);
                            })),
                    )
                    .child(
                        button("start-queue", &t, BtnKind::Primary, true)
                            .child(icon("play", 12.0, t.on_accent))
                            .child("Start queue")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.go(Page::Queue, cx);
                                this.start_queue(cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    pub fn render_footer(&self) -> AnyElement {
        let t = self.theme();
        let mono = |text: String| div().child(text);
        let mut left: Vec<AnyElement> = Vec::new();
        if let Some(job) = self.queue.current() {
            let position = self
                .queue
                .position()
                .map_or(String::new(), |(index, total)| format!(" {index}/{total}"));
            left.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(dot(t.accent))
                    .child(format!("Encoding{position} - {}%", job.percent))
                    .into_any_element(),
            );
            if let Some(fps) = self.queue.live.fps {
                left.push(mono(format!("{fps:.1} fps")).into_any_element());
            }
            if let Some(eta) = self.queue.live.eta_seconds {
                left.push(mono(format!("ETA {}", format_clock(eta))).into_any_element());
            }
        } else {
            left.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(dot(t.faint))
                    .child(if self.queue.active_count() > 0 {
                        format!("{} job(s) waiting", self.queue.active_count())
                    } else {
                        "Idle".to_owned()
                    })
                    .into_any_element(),
            );
        }
        let gpu = self.gpu_vendors();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(16.0))
            .h(px(28.0))
            .px(px(14.0))
            .border_t_1()
            .border_color(t.line)
            .bg(t.panel)
            .font_family(crate::widgets::MONO)
            .text_size(px(11.0))
            .text_color(t.faint)
            .children(left)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .child(format!("{} source(s)", self.library.sources.len())),
            )
            .when(!gpu.is_empty(), |this| {
                this.child(div().child(gpu.join(" / ")))
            })
            .into_any_element()
    }

    pub fn render_banners(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = self.theme();
        let mut banners = Vec::new();
        let missing: Vec<&str> = [
            ("FFmpeg", self.tools.ffmpeg.is_none()),
            ("FFprobe", self.tools.ffprobe.is_none()),
            ("MKVToolNix", self.tools.mkvmerge.is_none()),
        ]
        .into_iter()
        .filter(|(_, missing)| *missing)
        .map(|(name, _)| name)
        .collect();
        if !missing.is_empty() {
            let critical = self.tools.ffmpeg.is_none() || self.tools.ffprobe.is_none();
            banners.push(banner(
                &t,
                if critical { t.bad } else { t.warn },
                if critical { t.bad_soft } else { t.warn_soft },
                format!(
                    "{} not found. {}",
                    missing.join(", "),
                    if critical {
                        "Probing and encoding need FFmpeg and FFprobe. Install them or place them next to Kestrel."
                    } else {
                        "Remux with the MKVToolNix engine is unavailable; the FFmpeg engine still works."
                    }
                ),
                vec![
                    button("banner-redetect", &t, BtnKind::Default, true)
                        .child("Re-detect")
                        .on_click(cx.listener(|this, _, _, cx| this.redetect_tools(cx)))
                        .into_any_element(),
                ],
            ));
        }
        if let Some(install) = self.legacy.clone().filter(|_| !self.legacy_dismissed) {
            banners.push(banner(
                &t,
                t.info,
                t.info_soft,
                format!(
                    "A previous LangMux {} installation was found. Remove it to avoid two copies of the app.",
                    install.display_version
                ),
                vec![
                    button("legacy-remove", &t, BtnKind::Default, true)
                        .child(if self.legacy_busy() {
                            "Removing..."
                        } else {
                            "Remove old version"
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.remove_legacy(cx)))
                        .into_any_element(),
                    button("legacy-dismiss", &t, BtnKind::Ghost, true)
                        .child("Dismiss")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.legacy_dismissed = true;
                            cx.notify();
                        }))
                        .into_any_element(),
                ],
            ));
        }
        banners
    }

    pub fn render_toasts(&self) -> AnyElement {
        let t = self.theme();
        div()
            .absolute()
            .right(px(18.0))
            .bottom(px(44.0))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.0))
            .children(self.toasts.iter().map(|toast| {
                let color: Rgba = match toast.kind {
                    TagKind::Ok => t.ok,
                    TagKind::Warn => t.warn,
                    TagKind::Bad => t.bad,
                    TagKind::Info => t.info,
                    _ => t.accent,
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(10.0))
                    .max_w(px(420.0))
                    .rounded(px(10.0))
                    .bg(t.panel)
                    .border_1()
                    .border_color(t.line2)
                    .shadow(vec![gpui::BoxShadow {
                        color: t.shadow.into(),
                        offset: gpui::point(px(0.0), px(8.0)),
                        blur_radius: px(24.0),
                        spread_radius: px(0.0),
                    }])
                    .child(dot(color))
                    .child(div().text_color(t.text).child(toast.text.clone()))
            }))
            .into_any_element()
    }
}

fn banner(
    t: &crate::theme::Theme,
    accent: Rgba,
    soft: Rgba,
    text: String,
    actions: Vec<AnyElement>,
) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.0))
        .px(px(18.0))
        .py(px(8.0))
        .bg(soft)
        .border_b_1()
        .border_color(t.line)
        .child(icon("alert", 15.0, accent))
        .child(div().flex_1().min_w_0().text_color(t.text).child(text))
        .children(actions)
        .into_any_element()
}

pub fn _card_radius() -> f32 {
    RADIUS_CARD
}
