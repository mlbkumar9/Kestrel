//! Tooltip view and overlay panels (modal scrim, toast).

use gpui::{
    AnyView, App, BoxShadow, Div, IntoElement, Render, SharedString, Window, div, point,
    prelude::*, px,
};

use crate::theme::{RADIUS_OVERLAY, Theme};

pub struct Tooltip {
    text: SharedString,
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = *cx.global::<Theme>();
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(t.raised)
            .border_1()
            .border_color(t.line2)
            .text_size(px(12.0))
            .text_color(t.text)
            .child(self.text.clone())
    }
}

/// Builder for `.tooltip(...)`.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text: SharedString = text.into();
    move |_, cx| {
        let text = text.clone();
        cx.new(|_| Tooltip { text }).into()
    }
}

/// Elevated panel with the overlay radius and shadow.
pub fn overlay_panel(t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .bg(t.panel)
        .border_1()
        .border_color(t.line2)
        .rounded(px(RADIUS_OVERLAY))
        .overflow_hidden()
        .shadow(vec![BoxShadow {
            color: t.shadow.into(),
            offset: point(px(0.0), px(20.0)),
            blur_radius: px(60.0),
            spread_radius: px(0.0),
        }])
}

/// Dimmed full-window backdrop.
pub fn scrim(t: &Theme) -> Div {
    div().absolute().inset_0().bg(t.scrim)
}
