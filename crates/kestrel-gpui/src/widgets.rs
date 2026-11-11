use gpui::{Div, SharedString, Stateful, div, prelude::*, px};

use crate::theme::Theme;

pub fn label(text: impl Into<SharedString>, theme: Theme) -> Div {
    div().text_color(theme.text).child(text.into())
}

pub fn muted(text: impl Into<SharedString>, theme: Theme) -> Div {
    label(text, theme).text_color(theme.muted_text).text_xs()
}

pub fn button(id: impl Into<gpui::ElementId>, theme: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_3()
        .rounded_lg()
        .cursor_pointer()
        .text_color(theme.muted_text)
        .hover(move |style| style.bg(theme.hover).text_color(theme.text))
}

pub fn toggle(enabled: bool, theme: Theme) -> Stateful<Div> {
    div()
        .id("theme-toggle")
        .flex()
        .items_center()
        .w(px(44.0))
        .h(px(24.0))
        .px_1()
        .rounded_full()
        .cursor_pointer()
        .bg(if enabled { theme.accent } else { theme.border })
        .when(enabled, |this| this.justify_end())
        .hover(move |style| style.bg(theme.accent_hover))
        .child(div().size_4().rounded_full().bg(theme.on_accent))
}

pub fn card(theme: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_6()
        .bg(theme.card)
        .border_1()
        .border_color(theme.border)
        .rounded_xl()
}

pub fn table_row(theme: Theme) -> Div {
    div()
        .flex()
        .items_center()
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(theme.border)
        .hover(move |style| style.bg(theme.hover))
}

pub fn table_header(theme: Theme) -> Div {
    table_row(theme).text_xs().text_color(theme.muted_text)
}

pub fn scrollable_list(id: &'static str) -> Stateful<Div> {
    div().id(id).flex_1().min_h_0().overflow_y_scroll()
}
