//! Flex-based table rows with fixed and flexible columns.

use gpui::{AnyElement, Div, ElementId, FontWeight, Stateful, div, prelude::*, px};

use crate::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Col {
    pub width: Option<f32>,
    pub grow: f32,
    pub right: bool,
    /// Width a flexible column keeps while the others shrink.
    pub min: f32,
}

impl Col {
    pub const fn fixed(width: f32) -> Self {
        Self {
            width: Some(width),
            grow: 0.0,
            right: false,
            min: 0.0,
        }
    }

    pub const fn grow(grow: f32) -> Self {
        Self {
            width: None,
            grow,
            right: false,
            min: 0.0,
        }
    }

    pub const fn right(mut self) -> Self {
        self.right = true;
        self
    }

    pub const fn min(mut self, min: f32) -> Self {
        self.min = min;
        self
    }
}

fn cell(col: &Col, child: impl IntoElement) -> Div {
    let mut cell = div()
        .flex()
        .items_center()
        .min_w_0()
        .px(px(12.0))
        .when(col.right, |this| this.justify_end());
    match col.width {
        Some(width) => cell = cell.w(px(width)).flex_none(),
        None => {
            cell = cell.flex_basis(px(0.0)).min_w(px(col.min));
            cell.style().flex_grow = Some(col.grow);
            cell.style().flex_shrink = Some(1.0);
        }
    }
    cell.child(child)
}

/// Horizontally scrollable table body that keeps `min_w` when the card is narrower.
pub fn scroll_table(id: impl Into<ElementId>, min_w: f32, rows: Vec<AnyElement>) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .overflow_x_scroll()
        .child(div().min_w(px(min_w)).w_full().children(rows))
}

pub fn header_row(t: &Theme, cols: &[Col], labels: &[&str]) -> Div {
    div()
        .flex()
        .items_center()
        .w_full()
        .h(px(34.0))
        .border_b_1()
        .border_color(t.line)
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.faint)
        .children(
            cols.iter()
                .zip(labels)
                .map(|(col, text)| cell(col, text.to_uppercase()).whitespace_nowrap()),
        )
}

pub fn body_row(t: &Theme, cols: &[Col], cells: Vec<AnyElement>, last: bool) -> Div {
    let raised = t.raised;
    div()
        .flex()
        .items_center()
        .w_full()
        .min_h(px(44.0))
        .py(px(5.0))
        .when(!last, |this| this.border_b_1().border_color(t.line))
        .hover(move |s| s.bg(raised))
        .children(cols.iter().zip(cells).map(|(col, child)| cell(col, child)))
}
