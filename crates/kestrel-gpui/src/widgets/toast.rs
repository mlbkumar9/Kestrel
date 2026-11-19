use crate::theme::Theme;
use gpui::{prelude::*, *};
use std::time::Duration;
#[derive(Clone, Copy, Default)]
pub enum ToastKind {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}
struct Entry {
    id: usize,
    message: SharedString,
    kind: ToastKind,
}
pub struct Toasts {
    entries: Vec<Entry>,
    next_id: usize,
    theme: Theme,
}
impl Toasts {
    pub fn new(theme: Theme) -> Self {
        Self {
            entries: vec![],
            next_id: 0,
            theme,
        }
    }
    pub fn push(
        &mut self,
        message: impl Into<SharedString>,
        kind: ToastKind,
        duration: Duration,
        cx: &mut Context<Self>,
    ) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            message: message.into(),
            kind,
        });
        cx.spawn(async move |entity, cx| {
            cx.background_executor().timer(duration).await;
            let _ = entity.update(cx, |s, cx| s.dismiss(id, cx));
        })
        .detach();
        cx.notify();
        id
    }
    pub fn dismiss(&mut self, id: usize, cx: &mut Context<Self>) {
        self.entries.retain(|e| e.id != id);
        cx.notify();
    }
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}
impl Render for Toasts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(self.entries.iter().map(|entry| {
                let id = entry.id;
                let color = match entry.kind {
                    ToastKind::Info => t.accent,
                    ToastKind::Success => t.success,
                    ToastKind::Warning => t.warning,
                    ToastKind::Error => t.danger,
                };
                div()
                    .id(id)
                    .flex()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .rounded_md()
                    .bg(t.card)
                    .text_color(t.text)
                    .border_1()
                    .border_color(color)
                    .child(entry.message.clone())
                    .child(
                        div()
                            .id(("dismiss", id))
                            .cursor_pointer()
                            .child("×")
                            .on_click(cx.listener(move |s, _, _, cx| s.dismiss(id, cx))),
                    )
            }))
    }
}
