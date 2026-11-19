//! Stateless building blocks: icons, buttons, tags, cards, labels, kbd hints, status dots.

use gpui::{Div, FontWeight, Rgba, SharedString, Stateful, Svg, div, prelude::*, px, svg};

use crate::theme::{RADIUS_CARD, RADIUS_CONTROL, Theme};

pub const MONO: &str = "JetBrains Mono";

pub fn icon(name: &str, size: f32, color: Rgba) -> Svg {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

pub fn mix(a: Rgba, b: Rgba, amount: f32) -> Rgba {
    Rgba {
        r: a.r + (b.r - a.r) * amount,
        g: a.g + (b.g - a.g) * amount,
        b: a.b + (b.b - a.b) * amount,
        a: a.a + (b.a - a.a) * amount,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BtnKind {
    Default,
    Primary,
    Ghost,
}

/// A button shell. Attach children and `.on_click`.
pub fn button(id: impl Into<SharedString>, t: &Theme, kind: BtnKind, small: bool) -> Stateful<Div> {
    let t = *t;
    let id: SharedString = id.into();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(if small { 6.0 } else { 8.0 }))
        .h(px(if small { 26.0 } else { 32.0 }))
        .px(px(if small { 9.0 } else { 12.0 }))
        .rounded(px(if small { 7.0 } else { RADIUS_CONTROL }))
        .text_size(px(if small { 12.0 } else { 13.0 }))
        .font_weight(if kind == BtnKind::Primary {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::MEDIUM
        })
        .whitespace_nowrap()
        .cursor_pointer()
        .border_1()
        .map(|this| match kind {
            BtnKind::Default => this
                .bg(t.raised)
                .border_color(t.line2)
                .text_color(t.text)
                .hover(move |s| s.bg(t.hover)),
            BtnKind::Primary => {
                let hover = mix(t.accent, gpui::white().into(), 0.08);
                this.bg(t.accent)
                    .border_color(t.accent)
                    .text_color(t.on_accent)
                    .hover(move |s| s.bg(hover).border_color(hover))
            }
            BtnKind::Ghost => this
                .border_color(gpui::transparent_black())
                .text_color(t.muted)
                .hover(move |s| s.bg(t.hover).text_color(t.text)),
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TagKind {
    Plain,
    Acc,
    Ok,
    Warn,
    Bad,
    Info,
}

pub fn tag(text: impl Into<SharedString>, kind: TagKind, t: &Theme) -> Div {
    let (bg, fg, border) = match kind {
        TagKind::Plain => (t.raised, t.muted, t.line),
        TagKind::Acc => (
            t.accent_soft,
            t.accent_text,
            gpui::transparent_black().into(),
        ),
        TagKind::Ok => (t.ok_soft, t.ok, gpui::transparent_black().into()),
        TagKind::Warn => (t.warn_soft, t.warn, gpui::transparent_black().into()),
        TagKind::Bad => (t.bad_soft, t.bad, gpui::transparent_black().into()),
        TagKind::Info => (t.info_soft, t.info, gpui::transparent_black().into()),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(20.0))
        .px(px(7.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(px(11.0))
        .font_family(MONO)
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .child(text.into())
}

/// Keyboard hint such as `Ctrl K`.
pub fn kbd(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .px(px(5.0))
        .h(px(18.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .text_color(t.faint)
        .text_size(px(11.0))
        .font_family(MONO)
        .whitespace_nowrap()
        .child(text.into())
}

/// Small coloured status dot.
pub fn dot(color: Rgba) -> Div {
    div().flex_none().size(px(7.0)).rounded_full().bg(color)
}

pub fn card(t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .bg(t.panel)
        .border_1()
        .border_color(t.line)
        .rounded(px(RADIUS_CARD))
}

/// Uppercase 11 px section label.
pub fn label(text: &str, t: &Theme) -> Div {
    div()
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.faint)
        .child(text.to_uppercase())
}

/// Field label above a control (12 px muted).
pub fn field_label(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .text_size(px(12.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.muted)
        .child(text.into())
}

pub fn field(label_text: impl Into<SharedString>, t: &Theme, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .min_w_0()
        .child(field_label(label_text, t))
        .child(control)
}

pub fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(MONO)
        .text_size(px(12.0))
        .child(text.into())
}

/// Stat card: label above a large value.
pub fn stat_card(label_text: &str, value: impl IntoElement, t: &Theme) -> Div {
    card(t)
        .px(px(16.0))
        .py(px(14.0))
        .gap(px(4.0))
        .min_w_0()
        .child(label(label_text, t))
        .child(value)
}

/// Large stat value text (22 px).
pub fn stat_value(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .text_size(px(22.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text)
        .child(text.into())
}

/// Section with the prototype's padding and bottom divider.
pub fn section(t: &Theme, last: bool) -> Div {
    div()
        .flex()
        .flex_col()
        .px(px(20.0))
        .py(px(18.0))
        .when(!last, |this| this.border_b_1().border_color(t.line))
}

pub fn section_title(text: &str, t: &Theme) -> Div {
    div()
        .text_size(px(13.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text)
        .child(text.to_owned())
}

pub fn page_title(title: &str, subtitle: &str, t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(18.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text)
                .child(title.to_owned()),
        )
        .child(div().text_color(t.muted).child(subtitle.to_owned()))
}

/// Horizontal progress bar. `ok` colours it green for finished jobs.
pub fn progress_bar(percent: f32, ok: bool, height: f32, t: &Theme) -> Div {
    div()
        .w_full()
        .h(px(height))
        .rounded_full()
        .bg(t.raised)
        .border_1()
        .border_color(t.line)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(gpui::relative((percent / 100.0).clamp(0.0, 1.0)))
                .rounded_full()
                .bg(if ok { t.ok } else { t.accent }),
        )
}

/// Placeholder thumbnail used in file lists.
pub fn thumb(t: &Theme) -> Div {
    let (from, to) = if t.light {
        (0xD9DCE3, 0xF1F2F5)
    } else {
        (0x2A2F3A, 0x14161B)
    };
    div()
        .flex_none()
        .w(px(44.0))
        .h(px(28.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(t.line2)
        .bg(gpui::linear_gradient(
            135.0,
            gpui::linear_color_stop(gpui::rgb(from), 0.0),
            gpui::linear_color_stop(gpui::rgb(to), 1.0),
        ))
}
