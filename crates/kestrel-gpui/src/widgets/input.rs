//! Single-line text input with an accent focus ring.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, Window, actions, div, fill, point,
    prelude::*, px, relative, size,
};

use crate::theme::{RADIUS_CONTROL, Theme};

actions!(
    kestrel_text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Confirm,
    ]
);

pub const CONTEXT: &str = "KestrelTextInput";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        KeyBinding::new("left", Left, Some(CONTEXT)),
        KeyBinding::new("right", Right, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("ctrl-a", SelectAll, Some(CONTEXT)),
        KeyBinding::new("ctrl-v", Paste, Some(CONTEXT)),
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
        KeyBinding::new("ctrl-x", Cut, Some(CONTEXT)),
        KeyBinding::new("home", Home, Some(CONTEXT)),
        KeyBinding::new("end", End, Some(CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(CONTEXT)),
    ]);
}

/// Emitted by [`TextInput`]: the text changed through typing or paste, or Enter was pressed.
#[derive(Clone, Debug)]
pub enum TextEvent {
    Changed,
    Submit,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected: Range<usize>,
    reversed: bool,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
    selecting: bool,
    pub mono: bool,
    pub numeric: bool,
    pub disabled: bool,
    /// Shows a path-like value as `start...end` while the field is not focused and the
    /// text is wider than the box. Focusing the field shows and edits the full text.
    pub middle_ellipsis: bool,
    /// Whether the last paint showed the shortened text.
    ellipsized: bool,
    /// Draws no box, for inputs embedded in another container such as the palette.
    pub bare: bool,
    pub height: f32,
    pub font_size: f32,
    pub radius: f32,
}

impl EventEmitter<TextEvent> for TextInput {}

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: SharedString::default(),
            selected: 0..0,
            reversed: false,
            last_layout: None,
            last_bounds: None,
            scroll_x: px(0.0),
            selecting: false,
            mono: false,
            numeric: false,
            disabled: false,
            middle_ellipsis: false,
            ellipsized: false,
            bare: false,
            height: 32.0,
            font_size: 13.0,
            radius: RADIUS_CONTROL,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn placeholder(&mut self, text: impl Into<SharedString>) {
        self.placeholder = text.into();
    }

    /// Replaces the text without emitting a change event.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.content.as_ref() == text {
            return;
        }
        self.content = text.to_owned().into();
        let end = self.content.len();
        self.selected = end..end;
        self.scroll_x = px(0.0);
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected = offset..offset;
        self.reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.reversed {
            self.selected.start = offset;
        } else {
            self.selected.end = offset;
        }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
        cx.notify();
    }

    fn previous(&self, offset: usize) -> usize {
        self.content[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next(&self, offset: usize) -> usize {
        self.content[offset..]
            .chars()
            .next()
            .map_or(self.content.len(), |ch| offset + ch.len_utf8())
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.move_to(self.previous(self.cursor()), cx);
        } else {
            self.move_to(self.selected.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.move_to(self.next(self.selected.end), cx);
        } else {
            self.move_to(self.selected.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous(self.cursor()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next(self.cursor()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected = 0..self.content.len();
        self.reversed = false;
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(self.previous(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(self.next(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace(['\r', '\n'], " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextEvent::Submit);
    }

    fn index_for_position(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if self.content.is_empty() {
            return 0;
        }
        if self.ellipsized {
            // The shortened text is on screen; a click lands the caret at the end.
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left() + self.scroll_x)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle);
        cx.stop_propagation();
        self.selecting = true;
        let index = self.index_for_position(event.position);
        if event.modifiers.shift {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_for_position(event.position), cx);
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected),
            reversed: self.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .unwrap_or_else(|| self.selected.clone());
        let filtered: String = if self.numeric {
            new_text
                .chars()
                .filter(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '.'))
                .collect()
        } else {
            new_text.to_owned()
        };
        self.content = format!(
            "{}{}{}",
            &self.content[..range.start],
            filtered,
            &self.content[range.end..]
        )
        .into();
        let cursor = range.start + filtered.len();
        self.selected = cursor..cursor;
        self.reversed = false;
        cx.emit(TextEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range(range_utf16, new_text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + layout.x_for_index(range.start) - self.scroll_x,
                bounds.top(),
            ),
            point(
                bounds.left() + layout.x_for_index(range.end) - self.scroll_x,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let index = layout.index_for_x(position.x - bounds.left() + self.scroll_x)?;
        Some(self.offset_to_utf16(index))
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct Prepaint {
    line: Option<ShapedLine>,
    /// Shortened line painted instead of `line` when the field shows `start...end`.
    shown: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_x: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

const ELLIPSIS: char = '\u{2026}';

/// `text` cut to `keep` characters around an ellipsis. A third of them stay at the start and
/// the rest at the end, so a path keeps its root and its last folder or file name.
pub fn middle_ellipsis(text: &str, keep: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if keep >= chars.len() {
        return text.to_owned();
    }
    let head = keep / 3;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push(ELLIPSIS);
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// The longest [`middle_ellipsis`] of `text` that `fits`; the lone ellipsis if none does.
/// `text` itself is returned when it already fits.
pub fn fit_middle_ellipsis(text: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(text) {
        return text.to_owned();
    }
    let (mut low, mut high) = (0, text.chars().count());
    while low < high {
        let mid = (low + high).div_ceil(2);
        if fits(&middle_ellipsis(text, mid)) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    middle_ellipsis(text, low)
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = *cx.global::<Theme>();
        let input = self.input.read(cx);
        let content = input.content.clone();
        let content_is_empty = content.is_empty();
        let full_text = content.to_string();
        let selected = input.selected.clone();
        let cursor = input.cursor();
        let style = window.text_style();
        let (display, color) = if content.is_empty() {
            (input.placeholder.clone(), theme.faint)
        } else if input.disabled {
            (content, theme.faint)
        } else {
            (content, theme.text)
        };
        let run = TextRun {
            len: display.len(),
            font: style.font(),
            color: color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display, font_size, &[run], None);

        let shorten = input.middle_ellipsis
            && !input.disabled
            && !content_is_empty
            && !input.focus_handle.is_focused(window)
            && line.width > bounds.size.width;
        let shown = shorten.then(|| {
            let shape = |text: &str| {
                let run = TextRun {
                    len: text.len(),
                    font: style.font(),
                    color: color.into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                window
                    .text_system()
                    .shape_line(text.to_owned().into(), font_size, &[run], None)
            };
            let text = fit_middle_ellipsis(&full_text, |candidate| {
                shape(candidate).width <= bounds.size.width
            });
            shape(&text)
        });

        let mut scroll_x = input.scroll_x;
        let cursor_x = line.x_for_index(cursor);
        let width = bounds.size.width;
        if cursor_x - scroll_x > width - px(2.0) {
            scroll_x = cursor_x - width + px(2.0);
        }
        if cursor_x < scroll_x {
            scroll_x = cursor_x;
        }
        if line.width <= width || shown.is_some() {
            scroll_x = px(0.0);
        }
        let origin_x = bounds.left() - scroll_x;
        let (selection, cursor_quad) = if shown.is_some() {
            (None, None)
        } else if selected.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(origin_x + cursor_x, bounds.top() + px(1.0)),
                        size(px(1.5), bounds.size.height - px(2.0)),
                    ),
                    theme.accent,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(origin_x + line.x_for_index(selected.start), bounds.top()),
                        point(origin_x + line.x_for_index(selected.end), bounds.bottom()),
                    ),
                    theme.accent_soft,
                )),
                None,
            )
        };
        Prepaint {
            line: Some(line),
            shown,
            cursor: cursor_quad,
            selection,
            scroll_x,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        if !self.input.read(cx).disabled {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }
        let line = prepaint.line.take();
        let shown = prepaint.shown.take();
        let selection = prepaint.selection.take();
        let cursor = prepaint.cursor.take();
        let scroll_x = prepaint.scroll_x;
        let line_height = window.line_height();
        let focused = focus_handle.is_focused(window);
        let ellipsized = shown.is_some();
        let painted = shown.or_else(|| line.clone());
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = selection {
                window.paint_quad(selection);
            }
            if let Some(line) = &painted {
                line.paint(
                    point(bounds.left() - scroll_x, bounds.top()),
                    line_height,
                    window,
                    cx,
                )
                .ok();
            }
            if focused && let Some(cursor) = cursor {
                window.paint_quad(cursor);
            }
        });
        self.input.update(cx, |input, _| {
            input.last_layout = line;
            input.ellipsized = ellipsized;
            input.last_bounds = Some(bounds);
            input.scroll_x = scroll_x;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let focused = self.focus_handle.is_focused(window);
        let ring = gpui::BoxShadow {
            color: theme.accent_soft.into(),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(3.0),
        };
        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(self.height))
            .px(px(if self.bare { 0.0 } else { 10.0 }))
            .text_size(px(self.font_size))
            .when(self.mono, |this| this.font_family("JetBrains Mono"))
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(if self.disabled {
                CursorStyle::OperationNotAllowed
            } else {
                CursorStyle::IBeam
            })
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::confirm))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .when(!self.bare, |this| {
                this.rounded(px(self.radius))
                    .border_1()
                    .bg(theme.raised)
                    .border_color(if focused { theme.accent } else { theme.line2 })
                    .when(focused, |this| this.shadow(vec![ring]))
                    .when(self.disabled, |this| this.opacity(0.55))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(TextElement { input: cx.entity() }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn middle_ellipsis_keeps_the_start_and_the_end() {
        let path = "D:/Media/Movies/Some Long Folder/Season 01/Episode.mkv";
        assert_eq!(middle_ellipsis(path, 12), "D:/M\u{2026}sode.mkv");
    }

    #[test]
    fn middle_ellipsis_leaves_short_text_alone_and_cuts_on_characters() {
        assert_eq!(middle_ellipsis("abc", 3), "abc");
        assert_eq!(middle_ellipsis("abc", 9), "abc");
        assert_eq!(middle_ellipsis("", 0), "");
        let text = "\u{e9}".repeat(9);
        assert_eq!(
            middle_ellipsis(&text, 6),
            format!("\u{e9}\u{e9}\u{2026}{}", "\u{e9}".repeat(4))
        );
    }

    #[test]
    fn fit_picks_the_longest_candidate_that_fits() {
        let path = "C:/Users/Someone/Videos/Output/Season 02";
        let fits = |text: &str| text.chars().count() <= 20;
        let shown = fit_middle_ellipsis(path, fits);
        assert_eq!(shown.chars().count(), 20);
        assert!(shown.starts_with("C:/Use"));
        assert!(shown.ends_with("Season 02"));
        assert_eq!(fit_middle_ellipsis("short", fits), "short");
        assert_eq!(fit_middle_ellipsis(path, |_| false), "\u{2026}");
    }
}
