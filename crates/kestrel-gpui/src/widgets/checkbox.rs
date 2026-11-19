use crate::theme::Theme;
use gpui::{prelude::*, *};
pub fn checkbox(
    id: impl Into<ElementId>,
    checked: bool,
    label: impl Into<SharedString>,
    theme: Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .text_color(theme.text)
        .child(
            div()
                .size_4()
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .border_1()
                .border_color(theme.border)
                .bg(if checked { theme.accent } else { theme.card })
                .text_color(theme.on_accent)
                .child(if checked { "✓" } else { "" }),
        )
        .child(label.into())
}
