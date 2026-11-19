use crate::theme::Theme;
use gpui::{prelude::*, *};
#[derive(Clone)]
pub struct Column {
    flexible: bool,
    pub title: SharedString,
    pub width: Pixels,
}
impl Column {
    pub fn new(title: impl Into<SharedString>, width: f32) -> Self {
        Self {
            flexible: false,
            title: title.into(),
            width: px(width.max(0.)),
        }
    }
    pub fn flexible(mut self) -> Self {
        self.flexible = true;
        self
    }
}
#[derive(Clone, Copy, Debug)]
pub struct RowSelected(pub usize);
pub struct Table {
    selected_rows: std::collections::BTreeSet<usize>,
    progress_column: Option<usize>,
    columns: Vec<Column>,
    rows: Vec<Vec<SharedString>>,
    selected: Option<usize>,
    theme: Theme,
}
impl EventEmitter<RowSelected> for Table {}
impl Table {
    pub fn new(columns: Vec<Column>, rows: Vec<Vec<SharedString>>, theme: Theme) -> Self {
        Self {
            selected_rows: Default::default(),
            progress_column: None,
            columns,
            rows,
            selected: None,
            theme,
        }
    }
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = (index < self.rows.len()).then_some(index);
        self
    }
    pub fn selection(&self) -> Option<usize> {
        self.selected
    }
    pub fn progress_column(mut self, index: usize) -> Self {
        self.progress_column = Some(index);
        self
    }
    pub fn set_selected_rows(
        &mut self,
        rows: std::collections::BTreeSet<usize>,
        cx: &mut Context<Self>,
    ) {
        self.selected_rows = rows;
        cx.notify();
    }
    pub fn set_rows(&mut self, rows: Vec<Vec<SharedString>>, cx: &mut Context<Self>) {
        self.rows = rows;
        self.selected = None;
        cx.notify();
    }
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}
impl Render for Table {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .id("table")
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .text_color(t.text)
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .h_8()
                    .bg(t.card)
                    .text_color(t.muted_text)
                    .children(self.columns.iter().map(|c| {
                        div()
                            .when(c.flexible, |d| d.flex_1().min_w_0())
                            .when(!c.flexible, |d| d.w(c.width).flex_shrink_0())
                            .flex()
                            .items_center()
                            .h_full()
                            .px_3()
                            .py_2()
                            .overflow_hidden()
                            .child(c.title.clone())
                    })),
            )
            .child(
                uniform_list(
                    "rows",
                    self.rows.len(),
                    cx.processor(|s, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|i| {
                                let t = s.theme;
                                div()
                                    .id(i)
                                    .flex()
                                    .w_full()
                                    .h_8()
                                    .cursor_pointer()
                                    .border_b_1()
                                    .border_color(t.border)
                                    .bg(if s.selected_rows.contains(&i) || s.selected == Some(i) {
                                        t.selected
                                    } else {
                                        t.background
                                    })
                                    .hover(move |d| d.bg(t.hover))
                                    .children(s.columns.iter().enumerate().map(|(j, c)| {
                                        let text = s.rows[i].get(j).cloned().unwrap_or_default();
                                        div()
                                            .when(c.flexible, |d| d.flex_1().min_w_0())
                                            .when(!c.flexible, |d| d.w(c.width).flex_shrink_0())
                                            .flex()
                                            .items_center()
                                            .h_full()
                                            .px_3()
                                            .py_0()
                                            .overflow_hidden()
                                            .when(s.progress_column != Some(j), |d| {
                                                d.child(text.clone())
                                            })
                                            .when(s.progress_column == Some(j), |d| {
                                                d.flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(text.clone())
                                                    .child(
                                                        super::progress::progress(
                                                            text.trim_end_matches('%')
                                                                .parse::<f32>()
                                                                .unwrap_or(0.)
                                                                / 100.,
                                                            t,
                                                        )
                                                        .flex_1(),
                                                    )
                                            })
                                    }))
                                    .on_click(cx.listener(move |s, _, _, cx| {
                                        s.selected = Some(i);
                                        cx.emit(RowSelected(i));
                                        cx.notify();
                                    }))
                            })
                            .collect()
                    }),
                )
                .flex_1()
                .min_h_0(),
            )
    }
}
