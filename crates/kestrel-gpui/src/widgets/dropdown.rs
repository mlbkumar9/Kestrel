use crate::theme::Theme;
use gpui::{prelude::*, *};
#[derive(Clone, Debug)]
pub struct SelectionChanged(pub usize);
pub struct Select {
    options: Vec<SharedString>,
    selected: Option<usize>,
    placeholder: SharedString,
    open: bool,
    focus: FocusHandle,
    theme: Theme,
}
impl EventEmitter<SelectionChanged> for Select {}
impl Select {
    pub fn new(options: Vec<SharedString>, theme: Theme, cx: &mut Context<Self>) -> Self {
        Self {
            options,
            selected: None,
            placeholder: "Select".into(),
            open: false,
            focus: cx.focus_handle(),
            theme,
        }
    }
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = (index < self.options.len()).then_some(index);
        self
    }
    pub fn selection(&self) -> Option<usize> {
        self.selected
    }
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}
impl Render for Select {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .id("select")
            .relative()
            .track_focus(&self.focus)
            .text_color(t.text)
            .on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    s.open = false;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id("trigger")
                    .px_3()
                    .py_2()
                    .bg(t.card)
                    .border_1()
                    .border_color(t.border)
                    .rounded_md()
                    .cursor_pointer()
                    .child(
                        self.selected
                            .and_then(|i| self.options.get(i))
                            .cloned()
                            .unwrap_or(self.placeholder.clone()),
                    )
                    .on_click(cx.listener(|s, _, w, cx| {
                        s.open = !s.open;
                        w.focus(&s.focus);
                        cx.notify();
                    })),
            )
            .when(self.open, |d| {
                d.child(deferred(
                    div()
                        .id("options")
                        .occlude()
                        .absolute()
                        .top_full()
                        .left_0()
                        .w_full()
                        .max_h(px(240.))
                        .overflow_y_scroll()
                        .bg(t.card)
                        .border_1()
                        .border_color(t.border)
                        .rounded_md()
                        .on_mouse_down_out(cx.listener(|s, _, _, cx| {
                            s.open = false;
                            cx.notify();
                        }))
                        .children(self.options.iter().enumerate().map(|(i, label)| {
                            div()
                                .id(i)
                                .px_3()
                                .py_2()
                                .cursor_pointer()
                                .hover(move |s| s.bg(t.hover))
                                .child(label.clone())
                                .on_click(cx.listener(move |s, _, _, cx| {
                                    s.selected = Some(i);
                                    s.open = false;
                                    cx.emit(SelectionChanged(i));
                                    cx.notify();
                                }))
                        })),
                ))
            })
    }
}
