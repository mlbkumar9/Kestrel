use crate::theme::Theme;
use gpui::{prelude::*, *};
pub fn progress(value: f32, theme: Theme) -> Div {
    let value = if value.is_finite() {
        value.clamp(0., 1.)
    } else {
        0.
    };
    div()
        .w_full()
        .h_2()
        .rounded_full()
        .overflow_hidden()
        .bg(theme.border)
        .child(div().h_full().w(relative(value)).bg(theme.accent))
}
