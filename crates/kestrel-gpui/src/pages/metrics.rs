//! Metrics: placeholder for the quality metrics planned for 4.1.

use gpui::{AnyElement, Context, FontWeight, Window, div, prelude::*, px};

use super::{grid, page_scroll};
use crate::app::Kestrel;
use crate::ctl::InputCfg;
use crate::theme::RADIUS_CONTROL;
use crate::widgets::{TagKind, card, field, icon, stat_card, stat_value, tag};

const MODELS: [&str; 3] = ["vmaf_v0.6.1 (1080p)", "vmaf_4k_v0.6.1", "vmaf_v0.6.1neg"];

impl Kestrel {
    pub fn render_metrics(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let reference = self.field_text(
            cx,
            window,
            "met-ref",
            "",
            InputCfg::default().mono().disabled(true).ph("Source file"),
            |_, _, _, _| {},
        );
        let distorted = self.field_text(
            cx,
            window,
            "met-dis",
            "",
            InputCfg::default().mono().disabled(true).ph("Encoded file"),
            |_, _, _, _| {},
        );
        let model = div().opacity(0.5).child(field(
            "VMAF model",
            &t,
            div()
                .flex()
                .items_center()
                .justify_between()
                .h(px(32.0))
                .px(px(10.0))
                .rounded(px(RADIUS_CONTROL))
                .border_1()
                .border_color(t.line2)
                .bg(t.raised)
                .child(MODELS[0])
                .child(icon("chevron-down", 14.0, t.muted)),
        ));
        let empty_stat =
            |name: &'static str| stat_card(name, stat_value("-", &t).text_color(t.faint), &t);
        page_scroll("metrics-page")
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Quality metrics"),
                            )
                            .child(div().text_color(t.muted).child(
                                "VMAF, PSNR and SSIM of an encode against its source, per frame. Planned for Kestrel 4.1.",
                            )),
                    )
                    .child(
                        div()
                            .flex_1().flex().justify_end()
                            .child(tag("Coming in 4.1", TagKind::Info, &t)),
                    ),
            )
            .child(
                grid(3)
                    .child(field("Reference (source)", &t, reference))
                    .child(field("Distorted (encode)", &t, distorted))
                    .child(model),
            )
            .child(
                grid(4)
                    .gap(px(14.0))
                    .child(empty_stat("VMAF mean"))
                    .child(empty_stat("VMAF 1% low"))
                    .child(empty_stat("PSNR"))
                    .child(empty_stat("SSIM")),
            )
            .child(
                card(&t)
                    .h(px(260.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.faint)
                    .child("Per-frame VMAF chart appears here after a run."),
            )
            .into_any_element()
    }
}
