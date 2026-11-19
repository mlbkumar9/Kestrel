use crate::theme::Theme;
use gpui::{prelude::*, *};
#[derive(Clone, Copy, Debug)]
pub struct ValueChanged(pub f32);
pub struct Slider {
    pub value: f32,
    min: f32,
    max: f32,
    step: Option<f32>,
    theme: Theme,
    bounds: Bounds<Pixels>,
    dragging: bool,
}
impl EventEmitter<ValueChanged> for Slider {}
pub fn snap(value: f32, min: f32, max: f32, step: Option<f32>) -> f32 {
    if !min.is_finite() || !max.is_finite() || max <= min {
        return min;
    }
    let value = if value.is_finite() {
        value.clamp(min, max)
    } else {
        min
    };
    let value = match step {
        Some(s) if s.is_finite() && s > 0. => min + ((value - min) / s).round() * s,
        _ => value,
    };
    value.clamp(min, max)
}
impl Slider {
    pub fn new(value: f32, range: std::ops::RangeInclusive<f32>, theme: Theme) -> Self {
        let (min, max) = range.into_inner();
        Self {
            value: snap(value, min, max, None),
            min,
            max,
            step: None,
            theme,
            bounds: Bounds::default(),
            dragging: false,
        }
    }
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self.value = snap(self.value, self.min, self.max, self.step);
        self
    }
    pub fn set_value(&mut self, value: f32, cx: &mut Context<Self>) {
        self.value = snap(value, self.min, self.max, self.step);
        cx.notify();
    }
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
    fn update_position(&mut self, x: Pixels, cx: &mut Context<Self>) {
        if self.bounds.size.width <= px(0.) {
            return;
        }
        let ratio = (x - self.bounds.left()) / self.bounds.size.width;
        let value = snap(
            self.min + ratio * (self.max - self.min),
            self.min,
            self.max,
            self.step,
        );
        if self.value != value {
            self.value = value;
            cx.emit(ValueChanged(value));
            cx.notify();
        }
    }
}
impl Render for Slider {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        div()
            .id("slider")
            .h_6()
            .w_full()
            .cursor_pointer()
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, e: &MouseDownEvent, _, cx| {
                    s.dragging = true;
                    s.update_position(e.position.x, cx);
                }),
            )
            .on_mouse_move(cx.listener(|s, e: &MouseMoveEvent, _, cx| {
                if s.dragging {
                    s.update_position(e.position.x, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|s, _, _, _| s.dragging = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|s, _, _, _| s.dragging = false),
            )
            .child(
                super::progress::progress(
                    if self.max > self.min {
                        (self.value - self.min) / (self.max - self.min)
                    } else {
                        0.
                    },
                    self.theme,
                )
                .mt_2(),
            )
            .child(
                canvas(
                    move |b, _, cx| {
                        entity.update(cx, |s, _| s.bounds = b);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }
}
#[cfg(test)]
mod tests {
    use super::snap;
    #[test]
    fn snapping_and_bounds() {
        assert_eq!(snap(3.6, 1., 10., Some(2.)), 3.);
        assert_eq!(snap(99., 0., 10., Some(3.)), 9.);
        assert_eq!(snap(-1., 0., 10., None), 0.);
    }
    #[test]
    fn invalid_and_empty_ranges() {
        assert_eq!(snap(f32::NAN, 0., 1., None), 0.);
        assert_eq!(snap(5., 2., 2., None), 2.);
        assert_eq!(snap(0.4, 0., 1., Some(0.)), 0.4);
    }
}
