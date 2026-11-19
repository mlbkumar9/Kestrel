use crate::theme::Theme;
use gpui::{prelude::*, *};
pub struct Tooltip {
    text: SharedString,
    theme: Theme,
}
impl Tooltip {
    pub fn new(text: impl Into<SharedString>, theme: Theme) -> Self {
        Self {
            text: text.into(),
            theme,
        }
    }
    pub fn build(self, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }
}
impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(self.theme.card)
            .text_color(self.theme.text)
            .border_1()
            .border_color(self.theme.border)
            .text_xs()
            .child(self.text.clone())
    }
}
