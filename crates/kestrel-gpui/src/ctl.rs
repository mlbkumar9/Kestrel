//! Controls that need the root view: text fields kept in a registry and dropdown selects.

use std::rc::Rc;

use gpui::{
    BoxShadow, Context, Div, Entity, MouseButton, SharedString, Window, deferred, div, point,
    prelude::*, px,
};

use crate::app::Kestrel;
use crate::theme::{RADIUS_CONTROL, Theme};
use crate::widgets::input::{TextEvent, TextInput};
use crate::widgets::{checkbox, icon};

#[derive(Clone, Copy)]
pub struct InputCfg {
    pub placeholder: &'static str,
    pub mono: bool,
    pub numeric: bool,
    pub disabled: bool,
    pub height: f32,
    pub font_size: f32,
    pub pill: bool,
    pub middle_ellipsis: bool,
}

impl Default for InputCfg {
    fn default() -> Self {
        Self {
            placeholder: "",
            mono: false,
            numeric: false,
            disabled: false,
            height: 32.0,
            font_size: 13.0,
            pill: false,
            middle_ellipsis: false,
        }
    }
}

impl InputCfg {
    pub fn ph(mut self, placeholder: &'static str) -> Self {
        self.placeholder = placeholder;
        self
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn numeric(mut self) -> Self {
        self.numeric = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Shows a long path as `start...end` while the field is not focused.
    pub fn middle_ellipsis(mut self) -> Self {
        self.middle_ellipsis = true;
        self
    }

    /// Fully rounded 24 px input used for the language code boxes.
    pub fn pill(mut self) -> Self {
        self.pill = true;
        self.height = 24.0;
        self.font_size = 12.0;
        self
    }

    /// Table-row input: 28 px high, 12 px text.
    pub fn compact(mut self) -> Self {
        self.height = 28.0;
        self.font_size = 12.0;
        self
    }
}

impl Kestrel {
    /// A text field whose entity lives in the registry under `key`. `on` receives the new
    /// text and whether Enter was pressed. The field shows `value` whenever it is not focused.
    pub fn field_text(
        &mut self,
        cx: &mut Context<Self>,
        window: &mut Window,
        key: impl Into<SharedString>,
        value: &str,
        cfg: InputCfg,
        on: impl Fn(&mut Kestrel, &str, bool, &mut Context<Kestrel>) + 'static,
    ) -> Entity<TextInput> {
        let key = key.into();
        let entity = match self.inputs.get(&key) {
            Some(entity) => entity.clone(),
            None => {
                let entity = cx.new(TextInput::new);
                cx.subscribe_in(
                    &entity,
                    window,
                    move |this, input, event: &TextEvent, _, cx| {
                        let text = input.read(cx).text().to_owned();
                        on(this, &text, matches!(event, TextEvent::Submit), cx);
                        cx.notify();
                    },
                )
                .detach();
                self.inputs.insert(key, entity.clone());
                entity
            }
        };
        entity.update(cx, |input, _| {
            input.mono = cfg.mono;
            input.numeric = cfg.numeric;
            input.disabled = cfg.disabled;
            input.middle_ellipsis = cfg.middle_ellipsis;
            input.height = cfg.height;
            input.font_size = cfg.font_size;
            input.radius = if cfg.pill { 12.0 } else { RADIUS_CONTROL };
            input.placeholder(cfg.placeholder);
        });
        let focused = entity.read(cx).is_focused(window);
        if !focused && entity.read(cx).text() != value {
            entity.update(cx, |input, cx| input.set_text(value, cx));
        }
        entity
    }

    /// Numeric field editing an integer.
    pub fn field_number(
        &mut self,
        cx: &mut Context<Self>,
        window: &mut Window,
        key: impl Into<SharedString>,
        value: i64,
        cfg: InputCfg,
        on: impl Fn(&mut Kestrel, i64, &mut Context<Kestrel>) + 'static,
    ) -> Entity<TextInput> {
        self.field_text(
            cx,
            window,
            key,
            &value.to_string(),
            cfg.numeric(),
            move |this, text, _, cx| {
                if let Ok(number) = text.trim().parse::<i64>() {
                    on(this, number, cx);
                } else if text.trim().is_empty() || text.trim() == "-" {
                    on(this, 0, cx);
                }
            },
        )
    }

    pub fn check(
        &self,
        cx: &mut Context<Self>,
        id: impl Into<SharedString>,
        checked: bool,
        text: impl Into<SharedString>,
        on: impl Fn(&mut Kestrel, bool, &mut Context<Kestrel>) + 'static,
    ) -> gpui::Stateful<Div> {
        let t = self.theme();
        let id: SharedString = id.into();
        checkbox(cx, id, checked, Some(text.into()), &t, on)
    }

    /// Checkbox with no label, for table cells.
    pub fn check_box(
        &self,
        cx: &mut Context<Self>,
        id: impl Into<SharedString>,
        checked: bool,
        on: impl Fn(&mut Kestrel, bool, &mut Context<Kestrel>) + 'static,
    ) -> gpui::Stateful<Div> {
        let t = self.theme();
        let id: SharedString = id.into();
        checkbox(cx, id, checked, None, &t, on).min_h(px(18.0))
    }

    /// Dropdown select. The menu is drawn deferred so it sits above later siblings.
    pub fn select(
        &self,
        cx: &mut Context<Self>,
        id: impl Into<SharedString>,
        options: impl IntoIterator<Item = impl Into<SharedString>>,
        selected: usize,
        compact: bool,
        on_pick: impl Fn(&mut Kestrel, usize, &mut Context<Kestrel>) + 'static,
    ) -> Div {
        let t: Theme = self.theme();
        let id: SharedString = id.into();
        let open = self.open_menu.as_ref() == Some(&id);
        let options: Vec<SharedString> = options.into_iter().map(Into::into).collect();
        let current = options.get(selected).cloned().unwrap_or_default();
        let height = if compact { 28.0 } else { 32.0 };
        let on_pick = Rc::new(on_pick);
        let toggle_id = id.clone();
        let ring = BoxShadow {
            color: t.accent_soft.into(),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(3.0),
        };
        let trigger = div()
            .id(id.clone())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .w_full()
            .h(px(height))
            .px(px(if compact { 8.0 } else { 10.0 }))
            .rounded(px(RADIUS_CONTROL))
            .border_1()
            .bg(t.raised)
            .border_color(if open { t.accent } else { t.line2 })
            .when(open, |this| this.shadow(vec![ring]))
            .text_size(px(if compact { 12.0 } else { 13.0 }))
            .text_color(t.text)
            .cursor_pointer()
            .child(div().flex_1().min_w_0().truncate().child(current))
            .child(icon("chevron-down", 14.0, t.muted))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.open_menu = if this.open_menu.as_ref() == Some(&toggle_id) {
                        None
                    } else {
                        Some(toggle_id.clone())
                    };
                    cx.stop_propagation();
                    cx.notify();
                }),
            );
        let menu = open.then(|| {
            let items = options.iter().enumerate().map(|(index, text)| {
                let on_pick = on_pick.clone();
                let on = index == selected;
                div()
                    .id(SharedString::from(format!("{id}-opt-{index}")))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .h(px(30.0))
                    .px(px(10.0))
                    .rounded(px(6.0))
                    .text_size(px(13.0))
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .map(|this| {
                        if on {
                            this.bg(t.accent_soft).text_color(t.accent_text)
                        } else {
                            this.text_color(t.text).hover(move |s| s.bg(t.raised))
                        }
                    })
                    .child(text.clone())
                    .when(on, |this| this.child(icon("check", 12.0, t.accent_text)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            on_pick(this, index, cx);
                            this.open_menu = None;
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
            });
            deferred(
                div()
                    .id(SharedString::from(format!("{id}-menu")))
                    .absolute()
                    .top(px(height + 4.0))
                    .left_0()
                    .min_w_full()
                    .max_h(px(264.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .p(px(4.0))
                    .rounded(px(10.0))
                    .bg(t.panel)
                    .border_1()
                    .border_color(t.line2)
                    .shadow(vec![BoxShadow {
                        color: t.shadow.into(),
                        offset: point(px(0.0), px(12.0)),
                        blur_radius: px(32.0),
                        spread_radius: px(0.0),
                    }])
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(items),
            )
            .with_priority(10)
        });
        div()
            .relative()
            .w_full()
            .min_w_0()
            .child(trigger)
            .children(menu)
    }
}
