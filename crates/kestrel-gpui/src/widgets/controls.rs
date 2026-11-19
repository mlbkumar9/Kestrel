//! Interactive controls generic over the view that owns their state.

use std::rc::Rc;

use gpui::{
    App, Bounds, Context, DispatchPhase, Div, ElementId, FontWeight, Global, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString, Stateful, Window, canvas, div,
    point, prelude::*, px,
};

use super::basic::{MONO, icon};
use crate::theme::Theme;

pub fn checkbox<V: 'static>(
    cx: &mut Context<V>,
    id: impl Into<ElementId>,
    checked: bool,
    text: Option<SharedString>,
    t: &Theme,
    on_toggle: impl Fn(&mut V, bool, &mut Context<V>) + 'static,
) -> Stateful<Div> {
    let t = *t;
    let box_el = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(if checked { t.accent } else { t.line2 })
        .bg(if checked { t.accent } else { t.raised })
        .when(checked, |this| this.child(icon("check", 11.0, t.on_accent)));
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(9.0))
        .min_h(px(24.0))
        .cursor_pointer()
        .text_color(t.text)
        .child(box_el)
        .children(text)
        .on_click(cx.listener(move |view, _, _, cx| {
            on_toggle(view, !checked, cx);
            cx.notify();
        }))
}

pub fn segmented<V: 'static>(
    cx: &mut Context<V>,
    id: &str,
    options: impl IntoIterator<Item = impl Into<SharedString>>,
    selected: usize,
    t: &Theme,
    on_pick: impl Fn(&mut V, usize, &mut Context<V>) + 'static,
) -> Div {
    let t = *t;
    let on_pick = Rc::new(on_pick);
    div()
        .flex()
        .flex_none()
        .flex_wrap()
        .gap(px(2.0))
        .p(px(3.0))
        .rounded(px(9.0))
        .bg(t.raised)
        .border_1()
        .border_color(t.line)
        .children(options.into_iter().enumerate().map(|(index, text)| {
            let on_pick = on_pick.clone();
            let on = index == selected;
            div()
                .id(SharedString::from(format!("{id}-{index}")))
                .flex()
                .items_center()
                .h(px(26.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .border_1()
                .map(|this| {
                    if on {
                        this.bg(t.panel).border_color(t.line2).text_color(t.text)
                    } else {
                        this.border_color(gpui::transparent_black())
                            .text_color(t.muted)
                            .hover(move |s| s.text_color(t.text))
                    }
                })
                .child(text.into())
                .on_click(cx.listener(move |view, _, _, cx| {
                    on_pick(view, index, cx);
                    cx.notify();
                }))
        }))
}

/// Pill used for language and passthru toggles, with an optional count.
pub fn chip(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
    count: Option<usize>,
    on: bool,
    t: &Theme,
) -> Stateful<Div> {
    let t = *t;
    let id: SharedString = id.into();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .h(px(24.0))
        .px(px(9.0))
        .rounded_full()
        .text_size(px(12.0))
        .font_weight(FontWeight::MEDIUM)
        .border_1()
        .cursor_pointer()
        .map(|this| {
            if on {
                this.border_color(t.accent)
                    .bg(t.accent_soft)
                    .text_color(t.accent_text)
            } else {
                this.border_color(t.line2)
                    .bg(t.raised)
                    .text_color(t.muted)
                    .hover(move |s| s.text_color(t.text))
            }
        })
        .child(text.into())
        .children(count.map(|count| {
            div()
                .font_family(MONO)
                .text_size(px(11.0))
                .opacity(0.8)
                .child(count.to_string())
        }))
}

pub fn tab_bar<V: 'static>(
    cx: &mut Context<V>,
    labels: &[&'static str],
    selected: usize,
    t: &Theme,
    on_pick: impl Fn(&mut V, usize, &mut Context<V>) + 'static,
) -> Div {
    let t = *t;
    let on_pick = Rc::new(on_pick);
    div()
        .flex()
        .flex_wrap()
        .border_b_1()
        .border_color(t.line)
        .children(labels.iter().enumerate().map(|(index, text)| {
            let on_pick = on_pick.clone();
            let on = index == selected;
            div()
                .id(SharedString::from(format!("tab-{index}")))
                .flex()
                .items_center()
                .h(px(36.0))
                .mr(px(18.0))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .border_b_2()
                .map(|this| {
                    if on {
                        this.border_color(t.accent).text_color(t.text)
                    } else {
                        this.border_color(gpui::transparent_black())
                            .text_color(t.muted)
                            .hover(move |s| s.text_color(t.text))
                    }
                })
                .child(*text)
                .on_click(cx.listener(move |view, _, _, cx| {
                    on_pick(view, index, cx);
                    cx.notify();
                }))
        }))
}

/// Which slider currently follows the mouse.
#[derive(Default)]
struct SliderDrag(Option<SharedString>);

impl Global for SliderDrag {}

/// Horizontal slider reporting a 0..=1 fraction while the mouse is pressed on it.
pub fn slider<V: 'static>(
    cx: &mut Context<V>,
    id: impl Into<SharedString>,
    fraction: f32,
    t: &Theme,
    on_change: impl Fn(&mut V, f32, &mut Context<V>) + 'static,
) -> Div {
    let t = *t;
    let id: SharedString = id.into();
    let handler = Rc::new(cx.listener(move |view, value: &f32, _, cx| {
        on_change(view, *value, cx);
        cx.notify();
    }));
    let fraction = fraction.clamp(0.0, 1.0);
    div()
        .relative()
        .flex_1()
        .min_w(px(80.0))
        .h(px(20.0))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(
            div()
                .relative()
                .w_full()
                .h(px(4.0))
                .rounded_full()
                .bg(t.line2)
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(fraction))
                        .rounded_full()
                        .bg(t.accent),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(-5.0))
                        .left(gpui::relative(fraction))
                        .ml(px(-7.0))
                        .size(px(14.0))
                        .rounded_full()
                        .bg(t.text)
                        .border_2()
                        .border_color(t.accent),
                ),
        )
        .child(
            canvas(
                |bounds, _, _| bounds,
                move |_, bounds: Bounds<gpui::Pixels>, window: &mut Window, _cx: &mut App| {
                    let apply = {
                        let handler = handler.clone();
                        move |x: gpui::Pixels, window: &mut Window, cx: &mut App| {
                            let width = bounds.size.width.max(px(1.0));
                            let value = ((x - bounds.left()) / width).clamp(0.0, 1.0);
                            handler(&value, window, cx);
                        }
                    };
                    {
                        let id = id.clone();
                        let apply = apply.clone();
                        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble
                                && event.button == MouseButton::Left
                                && bounds.contains(&point(event.position.x, event.position.y))
                            {
                                cx.set_global(SliderDrag(Some(id.clone())));
                                apply(event.position.x, window, cx);
                            }
                        });
                    }
                    {
                        let id = id.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, _, window, cx| {
                            let active = cx
                                .try_global::<SliderDrag>()
                                .is_some_and(|drag| drag.0.as_ref() == Some(&id));
                            if active && event.pressed_button == Some(MouseButton::Left) {
                                apply(event.position.x, window, cx);
                            }
                        });
                    }
                    let id = id.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, _, _, cx| {
                        let active = cx
                            .try_global::<SliderDrag>()
                            .is_some_and(|drag| drag.0.as_ref() == Some(&id));
                        if active {
                            cx.set_global(SliderDrag(None));
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        )
}
