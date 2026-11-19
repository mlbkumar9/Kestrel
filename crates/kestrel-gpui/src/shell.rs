//! Window chrome: sidebar, header, status footer, banners and toasts.

use gpui::{
    AnyElement, Context, Div, FontWeight, Rgba, Stateful, Window, WindowControlArea, div,
    prelude::*, px,
};
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
const HEADER_HEIGHT: f32 = 52.0;
/// How long the queue pill shows "Done" after a clean run.
pub const DONE_PILL_SECONDS: u64 = 10;
const BAR_BUTTON: f32 = 30.0;
const CAPTION_BUTTON_WIDTH: f32 = 46.0;
/// macOS keeps its native traffic lights, drawn over the left of the sidebar header.
const MAC_TRAFFIC_LIGHTS: bool = cfg!(target_os = "macos");

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
            .pt(px(12.0))
            .pb(px(10.0))
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

    /// Minimise, maximise/restore and close buttons for the client-drawn title bar.
    /// On Windows the hit-test areas let the OS handle clicks and snap layouts; elsewhere
    /// the buttons call the window methods directly.
    fn caption_buttons(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let maximized = window.is_maximized();
        let button = |id: &'static str, area: WindowControlArea, glyph: &'static str| {
            let close = area == WindowControlArea::Close;
            let hover_bg = if close { hex(0xE81123) } else { t.hover };
            let hover_fg = if close { hex(0xFFFFFF) } else { t.text };
            div()
                .id(id)
                .window_control_area(area)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(CAPTION_BUTTON_WIDTH))
                .h(px(HEADER_HEIGHT))
                .group(id)
                .hover(move |s| s.bg(hover_bg))
                .child(
                    gpui::svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(px(10.0))
                        .flex_none()
                        .text_color(t.muted)
                        .group_hover(id, move |s| s.text_color(hover_fg)),
                )
        };
        let min = button("caption-min", WindowControlArea::Min, "win-min");
        let max = button(
            "caption-max",
            WindowControlArea::Max,
            if maximized { "win-restore" } else { "win-max" },
        );
        let close = button("caption-close", WindowControlArea::Close, "win-close");
        let (min, max, close) = if cfg!(windows) {
            (min, max, close)
        } else {
            (
                min.on_click(cx.listener(|_, _, window, _| window.minimize_window())),
                max.on_click(cx.listener(|_, _, window, _| window.zoom_window())),
                close.on_click(cx.listener(|_, _, window, _| window.remove_window())),
            )
        };
        div()
            .flex()
            .flex_none()
            .h_full()
            .child(min)
            .child(max)
            .child(close)
            .into_any_element()
    }

    /// Icon-only button for the top bar. A disabled one is dimmed and ignores clicks.
    fn bar_icon_button(
        &self,
        id: &'static str,
        glyph: &'static str,
        tip: &'static str,
        enabled: bool,
    ) -> Stateful<Div> {
        let t = self.theme();
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(BAR_BUTTON))
            .rounded(px(7.0))
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(move |s| s.bg(t.hover))
                    .tooltip(tooltip(tip))
            })
            .child(icon(glyph, 15.0, if enabled { t.muted } else { t.faint }))
            .when(!enabled, |this| this.opacity(0.45))
    }

    /// Live queue status: progress while running or paused, then "Done" or "N failed".
    /// `compact` drops the file name and ETA so the bar fits a narrow window.
    fn queue_pill(&self, compact: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = self.theme();
        let pill = div()
            .flex()
            .flex_shrink()
            .min_w_0()
            .items_center()
            .h(px(BAR_BUTTON))
            .rounded(px(8.0))
            .border_1()
            .border_color(t.line2)
            .bg(t.raised)
            .overflow_hidden();
        let body = |id: &'static str| {
            div()
                .id(id)
                .flex()
                .flex_shrink()
                .min_w_0()
                .overflow_hidden()
                .items_center()
                .h_full()
                .gap(px(8.0))
                .px(px(10.0))
                .text_size(px(12.0))
                .cursor_pointer()
                .hover(move |s| s.bg(t.hover))
        };
        let mono = |text: String, color: Rgba| {
            div()
                .flex_none()
                .font_family(crate::widgets::MONO)
                .text_size(px(11.0))
                .text_color(color)
                .child(text)
        };
        if self.queue.running {
            let paused = self.queue.paused;
            let percent = self.queue.current().map_or(0, |job| job.percent.min(100));
            let name = self
                .queue
                .current()
                .map_or_else(|| "Preparing".to_owned(), |job| job.name.clone());
            let color = if paused { t.warn } else { t.accent };
            let eta = if paused {
                "Paused".to_owned()
            } else {
                self.queue
                    .live
                    .eta_seconds
                    .map_or_else(String::new, |eta| format!("ETA {}", format_clock(eta)))
            };
            let bar = div()
                .flex_none()
                .w(px(40.0))
                .h(px(4.0))
                .rounded_full()
                .bg(t.line2)
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(color)
                        .w(gpui::relative(f32::from(percent as u16) / 100.0)),
                );
            return Some(
                pill.child(
                    body("queue-pill")
                        .tooltip(tooltip("Open the queue (Ctrl+4)"))
                        .child(bar)
                        .when(!compact, |this| {
                            this.child(
                                div()
                                    .flex_shrink()
                                    .min_w_0()
                                    .max_w(px(120.0))
                                    .truncate()
                                    .text_color(t.text)
                                    .child(name),
                            )
                        })
                        .child(mono(format!("{percent}%"), t.muted))
                        .when(!compact && !eta.is_empty(), |this| {
                            this.child(mono(eta, t.faint))
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.go(Page::Queue, cx))),
                )
                .child(
                    div()
                        .id("queue-pill-pause")
                        .flex()
                        .items_center()
                        .justify_center()
                        .h_full()
                        .w(px(30.0))
                        .border_l_1()
                        .border_color(t.line2)
                        .cursor_pointer()
                        .hover(move |s| s.bg(t.hover))
                        .tooltip(tooltip(if paused {
                            "Resume the queue"
                        } else {
                            "Pause the queue"
                        }))
                        .child(icon(if paused { "play" } else { "pause" }, 12.0, t.text))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
                )
                .into_any_element(),
            );
        }
        let (finished, summary) = self.queue_done?;
        if summary.failed > 0 {
            return Some(
                pill.border_color(t.bad)
                    .bg(t.bad_soft)
                    .child(
                        body("queue-pill-failed")
                            .tooltip(tooltip("Show the failures in Activity"))
                            .child(icon("alert", 13.0, t.bad))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.bad)
                                    .child(format!("{} failed", summary.failed)),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.queue_done = None;
                                this.go(Page::Activity, cx);
                            })),
                    )
                    .into_any_element(),
            );
        }
        if finished.elapsed().as_secs() >= DONE_PILL_SECONDS {
            return None;
        }
        let noun = if summary.done == 1 { "file" } else { "files" };
        Some(
            pill.child(
                body("queue-pill-done")
                    .tooltip(tooltip("Open the queue (Ctrl+4)"))
                    .child(icon("check", 13.0, t.ok))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ok)
                            .child(format!("Done \u{b7} {} {noun}", summary.done)),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.go(Page::Queue, cx))),
            )
            .into_any_element(),
        )
    }

    pub fn render_header(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let captions = (!MAC_TRAFFIC_LIGHTS).then(|| self.caption_buttons(window, cx));
        // Drag regions are siblings of the controls, never their parents: the platform hit
        // test returns the first control area under the pointer, so a drag parent would
        // swallow every button.
        let drag = |id: &'static str| {
            div()
                .id(id)
                .window_control_area(WindowControlArea::Drag)
                .h_full()
        };
        let mark = gpui::img(gpui::ImageSource::Resource(gpui::Resource::Embedded(
            if t.light {
                crate::assets::LOGO_LIGHT
            } else {
                crate::assets::LOGO
            }
            .into(),
        )))
        .flex_none()
        .size(px(20.0))
        .object_fit(gpui::ObjectFit::Contain);
        let can_back = self.nav.can_back();
        let can_forward = self.nav.can_forward();
        let back = self
            .bar_icon_button("nav-back", "arrow-left", "Back (Alt+Left)", can_back)
            .when(can_back, |this| {
                this.on_click(cx.listener(|this, _, _, cx| this.go_back(cx)))
            });
        let forward = self
            .bar_icon_button(
                "nav-forward",
                "arrow-right",
                "Forward (Alt+Right)",
                can_forward,
            )
            .when(can_forward, |this| {
                this.on_click(cx.listener(|this, _, _, cx| this.go_forward(cx)))
            });
        let sidebar_toggle = self
            .bar_icon_button(
                "toggle-sidebar",
                "panel-left",
                if self.collapsed {
                    "Show sidebar (Ctrl+B)"
                } else {
                    "Hide sidebar (Ctrl+B)"
                },
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)));
        let breadcrumb = drag("header-title")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .pl(px(10.0))
            .pr(px(8.0))
            .child(mark)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(14.0))
                    .child("Kestrel"),
            )
            .child(tag("4.0", TagKind::Plain, &t))
            .child(div().text_color(t.faint).child("/"))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.page.label()),
            );
        let search = div()
            .id("open-palette")
            .flex()
            .flex_shrink()
            .items_center()
            .gap(px(8.0))
            .w(px(360.0))
            .min_w(px(120.0))
            .h(px(BAR_BUTTON))
            .px(px(10.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(t.line2)
            .bg(t.raised)
            .cursor_pointer()
            .hover(move |s| s.bg(t.hover))
            .tooltip(tooltip("Search files, presets and actions (Ctrl+K)"))
            .child(icon("search", 14.0, t.muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(t.muted)
                    .child("Search files, presets and actions"),
            )
            .child(kbd("Ctrl K", &t))
            .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx)));
        let unread = if self.page == Page::Activity {
            0
        } else {
            self.activity.unread
        };
        let bell = self
            .bar_icon_button("open-activity", "bell", "Activity (Ctrl+5)", true)
            .relative()
            .when(unread > 0, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(2.0))
                        .right(px(1.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .min_w(px(15.0))
                        .h(px(15.0))
                        .px(px(3.0))
                        .rounded_full()
                        .bg(t.bad)
                        .text_color(gpui::white())
                        .text_size(px(9.0))
                        .font_weight(FontWeight::BOLD)
                        .child(if unread > 9 {
                            "9+".to_owned()
                        } else {
                            unread.to_string()
                        }),
                )
            })
            .on_click(cx.listener(|this, _, _, cx| this.go(Page::Activity, cx)));
        let theme_toggle = self
            .bar_icon_button(
                "toggle-theme",
                if t.light { "moon" } else { "sun" },
                if t.light {
                    "Switch to the dark theme"
                } else {
                    "Switch to the light theme"
                },
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_theme(cx)));
        let running = self.queue.running;
        let paused = self.queue.paused;
        let start = button("start-queue", &t, BtnKind::Primary, true)
            .tooltip(tooltip(if running {
                if paused {
                    "Resume the queue"
                } else {
                    "Pause the queue"
                }
            } else {
                "Start the queue (Ctrl+4 shows it)"
            }))
            .child(icon(
                if running && !paused { "pause" } else { "play" },
                12.0,
                t.on_accent,
            ))
            .child(match (running, paused) {
                (true, false) => "Pause",
                (true, true) => "Resume",
                _ => "Start queue",
            })
            .on_click(cx.listener(|this, _, _, cx| {
                if this.queue.running {
                    this.toggle_pause(cx);
                } else {
                    this.go(Page::Queue, cx);
                    this.start_queue(cx);
                }
            }));
        let add_media = button("add-media", &t, BtnKind::Default, true)
            .tooltip(tooltip("Add media (Ctrl+O)"))
            .child(icon("plus", 13.0, t.text))
            .child("Add media")
            .on_click(cx.listener(|this, _, _, cx| {
                this.go(Page::Library, cx);
                this.pick(Pick::Files, cx);
            }));
        let compact = window.viewport_size().width < px(1280.0);
        let pill = self.queue_pill(compact, cx);
        div()
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .h(px(HEADER_HEIGHT))
            .bg(t.panel)
            .border_b_1()
            .border_color(t.line)
            .child(drag("header-lead").w(px(if MAC_TRAFFIC_LIGHTS { 78.0 } else { 10.0 })))
            .child(sidebar_toggle)
            .child(div().w(px(2.0)))
            .child(back)
            .child(forward)
            .child(breadcrumb)
            .child(drag("header-drag-left").flex_1().min_w(px(8.0)))
            .child(search)
            .child(drag("header-drag-right").flex_1().min_w(px(8.0)))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .pr(px(10.0))
                    .children(pill)
                    .child(bell)
                    .child(theme_toggle)
                    .child(add_media)
                    .child(start),
            )
            .children(captions)
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
