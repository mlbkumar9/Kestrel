//! One module per page. Each adds a `render_*` method to the root view.

mod activity;
mod library;
mod metrics;
mod queue;
mod remux;
mod settings;
mod transcode;

use gpui::{Div, Stateful, div, prelude::*, px};

use crate::theme::Theme;

/// Scrolling page body with the prototype padding.
pub fn page_scroll(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .px(px(24.0))
        .py(px(22.0))
        .gap(px(16.0))
}

/// Equal-width grid with the prototype gaps.
pub fn grid(columns: u16) -> Div {
    div()
        .grid()
        .grid_cols(columns)
        .gap_x(px(18.0))
        .gap_y(px(14.0))
        .w_full()
}

/// Fixed-width side panel used for inspectors.
pub fn inspector(width: f32, t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(width))
        .h_full()
        .gap(px(16.0))
        .p(px(18.0))
        .bg(t.panel)
        .border_l_1()
        .border_color(t.line)
}

/// Label/value row used in summary cards.
pub fn summary_row(label: &str, value: String, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .text_size(px(12.0))
        .child(div().text_color(t.muted).child(label.to_owned()))
        .child(
            div()
                .flex_1()
                .text_right()
                .min_w_0()
                .truncate()
                .font_family(crate::widgets::MONO)
                .child(value),
        )
}
