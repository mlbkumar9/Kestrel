use crate::theme::Theme;
use gpui::{prelude::*, *};
pub fn tabs(
    id: impl Into<ElementId>,
    labels: impl IntoIterator<Item = SharedString>,
    selected: usize,
    theme: Theme,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let callback = std::rc::Rc::new(on_select);
    div()
        .id(id)
        .flex()
        .gap_2()
        .border_b_1()
        .border_color(theme.border)
        .children(labels.into_iter().enumerate().map(|(i, label)| {
            let callback = callback.clone();
            div()
                .id(i)
                .px_3()
                .py_2()
                .cursor_pointer()
                .text_color(if i == selected {
                    theme.accent
                } else {
                    theme.muted_text
                })
                .when(i == selected, |d| d.border_b_2().border_color(theme.accent))
                .child(label)
                .on_click(move |_, w, c| callback(i, w, c))
        }))
}
