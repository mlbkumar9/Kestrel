//! Settings: tools, hardware encoders, behaviour and appearance.

use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use kestrel_core::tools::Tool;
use kestrel_core::transcode::{HwCodec, HwVendor, VideoEncoder};

use super::{grid, page_scroll};
use crate::app::Kestrel;
use crate::ctl::InputCfg;
use crate::shell::vendor_label;
use crate::state::format::tool_version;
use crate::state::{PARALLEL_JOBS, PRIORITIES};
use crate::theme::Appearance;
use crate::widgets::controls::segmented;
use crate::widgets::{BtnKind, TagKind, button, card, field, section, section_title, tag};

const VENDORS: [HwVendor; 3] = [HwVendor::Nvenc, HwVendor::Qsv, HwVendor::Amf];

fn codec_name(codec: HwCodec) -> &'static str {
    match codec {
        HwCodec::H264 => "H.264",
        HwCodec::Hevc => "HEVC",
        HwCodec::Av1 => "AV1",
    }
}

impl Kestrel {
    fn tool_row(
        &mut self,
        cx: &mut Context<Self>,
        window: &mut Window,
        key: &'static str,
        name: &'static str,
        tool: Option<Tool>,
    ) -> gpui::Div {
        let t = self.theme();
        let path = tool
            .as_ref()
            .map(|tool| tool.path.display().to_string())
            .unwrap_or_default();
        let input = self.field_text(
            cx,
            window,
            key,
            &path,
            InputCfg::default().mono().disabled(true).ph("Not found"),
            |_, _, _, _| {},
        );
        let badge = match tool.as_ref().and_then(|tool| tool.version.as_deref()) {
            Some(version) => tag(tool_version(version), TagKind::Ok, &t),
            None if tool.is_some() => tag("unknown", TagKind::Warn, &t),
            None => tag("missing", TagKind::Bad, &t),
        };
        field(
            name,
            &t,
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(div().flex_1().min_w_0().child(input))
                .child(badge),
        )
    }

    pub fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let ffmpeg = self.tool_row(
            cx,
            window,
            "set-ffmpeg",
            "FFmpeg",
            self.tools.ffmpeg.clone(),
        );
        let ffprobe = self.tool_row(
            cx,
            window,
            "set-ffprobe",
            "FFprobe",
            self.tools.ffprobe.clone(),
        );
        let mkvmerge = self.tool_row(
            cx,
            window,
            "set-mkvmerge",
            "mkvmerge",
            self.tools.mkvmerge.clone(),
        );
        let tools = section(&t, false)
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(section_title("Tools", &t))
                    .child(
                        div().flex_1().flex().justify_end().child(
                            button("set-redetect-tools", &t, BtnKind::Default, true)
                                .child("Re-detect tools")
                                .on_click(cx.listener(|this, _, _, cx| this.redetect_tools(cx))),
                        ),
                    ),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(ffmpeg)
                    .child(ffprobe)
                    .child(mkvmerge),
            );

        let mut hardware_tags: Vec<AnyElement> = Vec::new();
        match &self.encoders {
            None => hardware_tags.push(tag("Detecting...", TagKind::Plain, &t).into_any_element()),
            Some(set) => {
                for encoder in &set.hardware {
                    if let VideoEncoder::Hardware { vendor, codec } = encoder {
                        hardware_tags.push(
                            tag(
                                format!("{} {}", vendor_label(*vendor), codec_name(*codec)),
                                TagKind::Ok,
                                &t,
                            )
                            .into_any_element(),
                        );
                    }
                }
                for vendor in VENDORS {
                    let found = set.hardware.iter().any(|encoder| {
                        matches!(encoder, VideoEncoder::Hardware { vendor: have, .. } if *have == vendor)
                    });
                    if !found {
                        hardware_tags.push(
                            tag(
                                format!("{} not found", vendor_label(vendor)),
                                TagKind::Plain,
                                &t,
                            )
                            .into_any_element(),
                        );
                    }
                }
            }
        }
        let detection_error = self.encoder_error.clone().map(|message| {
            div()
                .mt(px(12.0))
                .text_size(px(12.0))
                .text_color(t.bad)
                .child(message)
        });
        let hardware = section(&t, false)
            .child(section_title("Hardware encoders", &t))
            .children(detection_error)
            .child(
                div()
                    .mt(px(12.0))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.0))
                    .children(hardware_tags)
                    .child(
                        div().flex_1().flex().justify_end().child(
                            button("set-redetect-enc", &t, BtnKind::Default, true)
                                .child("Re-detect")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.encoders = None;
                                    this.detect_encoders(cx);
                                    cx.notify();
                                })),
                        ),
                    ),
            );

        let behaviour = section(&t, false)
            .child(section_title("Behaviour", &t))
            .child(
                grid(2)
                    .mt(px(12.0))
                    .child(field(
                        "Parallel jobs",
                        &t,
                        self.select(
                            cx,
                            "set-parallel",
                            PARALLEL_JOBS,
                            self.settings.parallel_jobs,
                            false,
                            |this, index, _| this.settings.parallel_jobs = index,
                        ),
                    ))
                    .child(field(
                        "Process priority",
                        &t,
                        self.select(
                            cx,
                            "set-priority",
                            PRIORITIES,
                            self.settings.priority,
                            false,
                            |this, index, _| this.settings.priority = index,
                        ),
                    ))
                    .child(self.check(
                        cx,
                        "set-notify",
                        self.settings.notify_when_done,
                        "Notify when the queue finishes",
                        |this, on, _| this.settings.notify_when_done = on,
                    ))
                    .child(self.check(
                        cx,
                        "set-dates",
                        self.settings.keep_source_dates,
                        "Keep the source file's dates",
                        |this, on, _| this.settings.keep_source_dates = on,
                    )),
            );

        let current = Appearance::ALL
            .iter()
            .position(|appearance| *appearance == self.appearance)
            .unwrap_or(0);
        let appearance = section(&t, true)
            .child(section_title("Appearance", &t))
            .child(div().mt(px(12.0)).child(segmented(
                cx,
                "set-appearance",
                Appearance::ALL.map(Appearance::label),
                current,
                &t,
                |this, index, cx| this.set_appearance(Appearance::ALL[index], cx),
            )));

        page_scroll("settings-page")
            .child(
                card(&t)
                    .max_w(px(760.0))
                    .w_full()
                    .child(tools)
                    .child(hardware)
                    .child(behaviour)
                    .child(appearance),
            )
            .into_any_element()
    }
}
