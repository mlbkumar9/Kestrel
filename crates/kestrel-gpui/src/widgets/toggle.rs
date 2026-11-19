use crate::theme::Theme;
use gpui::{prelude::*, *};
pub fn toggle(id: impl Into<ElementId>, enabled: bool, theme: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .w(px(44.))
        .h(px(24.))
        .px_1()
        .rounded_full()
        .cursor_pointer()
        .bg(if enabled { theme.accent } else { theme.border })
        .when(enabled, |d| d.justify_end())
        .child(div().size_4().rounded_full().bg(theme.on_accent))
}
