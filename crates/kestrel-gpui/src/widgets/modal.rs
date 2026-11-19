use crate::theme::Theme;
use gpui::{prelude::*, *};
#[derive(Clone, Copy, Debug)]
pub struct ModalClosed;
pub struct Modal {
    title: SharedString,
    body: SharedString,
    actions: Vec<SharedString>,
    open: bool,
    focus: FocusHandle,
    theme: Theme,
}
#[derive(Clone, Copy, Debug)]
pub struct ModalAction(pub usize);
impl EventEmitter<ModalClosed> for Modal {}
impl EventEmitter<ModalAction> for Modal {}
impl Modal {
    pub fn new(
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            actions: vec![],
            open: false,
            focus: cx.focus_handle(),
            theme,
        }
    }
    pub fn actions(mut self, actions: Vec<SharedString>) -> Self {
        self.actions = actions;
        self
    }
    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        window.focus(&self.focus);
        cx.notify();
    }
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.emit(ModalClosed);
        cx.notify();
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}
impl Render for Modal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div().when(self.open, |d| {
            d.child(deferred(
                div()
                    .id("modal-overlay")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(t.overlay)
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
                        if e.keystroke.key == "escape" {
                            s.close(cx);
                        }
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .w(px(480.))
                            .p_6()
                            .rounded_lg()
                            .bg(t.card)
                            .text_color(t.text)
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(div().text_lg().child(self.title.clone()))
                            .child(self.body.clone())
                            .child(div().flex().justify_end().gap_2().children(
                                self.actions.iter().enumerate().map(|(i, label)| {
                                    super::button::Button::new(i, label.clone(), t).on_click(
                                        cx.listener(move |s, _, _, cx| {
                                            cx.emit(ModalAction(i));
                                            s.close(cx);
                                        }),
                                    )
                                }),
                            )),
                    ),
            ))
        })
    }
}
