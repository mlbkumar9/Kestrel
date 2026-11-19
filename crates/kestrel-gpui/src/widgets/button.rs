use crate::theme::Theme;
use gpui::{prelude::*, *};

#[derive(Clone, Copy, Default)]
pub enum ButtonVariant {
    Primary,
    #[default]
    Secondary,
    Ghost,
    Danger,
}
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    theme: Theme,
    variant: ButtonVariant,
    disabled: bool,
    icon: Option<SharedString>,
    on_click: Option<ClickHandler>,
}
impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: Theme) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            theme,
            variant: ButtonVariant::Secondary,
            disabled: false,
            icon: None,
            on_click: None,
        }
    }
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}
impl RenderOnce for Button {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let t = self.theme;
        let (bg, fg) = match self.variant {
            ButtonVariant::Primary => (t.accent, t.on_accent),
            ButtonVariant::Secondary => (t.hover, t.text),
            ButtonVariant::Ghost => (t.card, t.text),
            ButtonVariant::Danger => (t.danger, t.on_accent),
        };
        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(bg)
            .text_color(fg)
            .when(self.disabled, |d| d.opacity(0.45))
            .when(!self.disabled, |d| {
                d.cursor_pointer()
                    .hover(move |s| s.bg(t.border))
                    .when_some(self.on_click, |d, f| d.on_click(f))
            })
            .children(self.icon)
            .child(self.label)
    }
}
