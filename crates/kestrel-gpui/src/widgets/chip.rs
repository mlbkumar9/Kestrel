use crate::theme::Theme;
use gpui::{prelude::*, *};
pub fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    count: Option<usize>,
    theme: Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded_full()
        .cursor_pointer()
        .bg(if selected { theme.selected } else { theme.card })
        .text_color(if selected { theme.accent } else { theme.text })
        .border_1()
        .border_color(if selected { theme.accent } else { theme.border })
        .child(label.into())
        .when_some(count, |d, n| {
            d.child(
                div()
                    .px_1()
                    .rounded_full()
                    .bg(theme.hover)
                    .child(n.to_string()),
            )
        })
}
