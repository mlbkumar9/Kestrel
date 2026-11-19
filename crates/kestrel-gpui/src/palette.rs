//! Command palette (Ctrl+K) and the preset-name dialog.

use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, MouseButton, Window, div, prelude::*, px,
};

use crate::actions::{PALETTE_CONTEXT, PaletteDown, PaletteUp};
use crate::app::Kestrel;
use crate::state::Page;
use crate::widgets::input::{TextEvent, TextInput};
use crate::widgets::overlay::{overlay_panel, scrim};
use crate::widgets::{BtnKind, button, icon, kbd, label};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    AddFiles,
    AddFolder,
    Go(Page),
    Preset(usize),
    StartQueue,
    TogglePause,
    ToggleTheme,
}

pub struct PaletteState {
    pub input: Entity<TextInput>,
    pub selected: usize,
}

pub struct PaletteItem {
    pub section: &'static str,
    pub label: String,
    pub hint: &'static str,
    pub command: Command,
}

const GO_HINTS: [&str; 7] = [
    "Ctrl 1", "Ctrl 2", "Ctrl 3", "Ctrl 4", "Ctrl 5", "Ctrl 6", "",
];

/// Every palette entry that matches `query`, grouped by section.
pub fn items(app: &Kestrel, query: &str) -> Vec<PaletteItem> {
    let mut all = vec![
        PaletteItem {
            section: "Actions",
            label: "Add files...".to_owned(),
            hint: "Ctrl O",
            command: Command::AddFiles,
        },
        PaletteItem {
            section: "Actions",
            label: "Add folder...".to_owned(),
            hint: "",
            command: Command::AddFolder,
        },
        PaletteItem {
            section: "Actions",
            label: "Start queue".to_owned(),
            hint: "",
            command: Command::StartQueue,
        },
        PaletteItem {
            section: "Actions",
            label: if app.queue.paused {
                "Resume queue".to_owned()
            } else {
                "Pause queue".to_owned()
            },
            hint: "",
            command: Command::TogglePause,
        },
        PaletteItem {
            section: "Actions",
            label: "Toggle light / dark".to_owned(),
            hint: "",
            command: Command::ToggleTheme,
        },
    ];
    for (index, page) in Page::ALL.into_iter().enumerate() {
        all.push(PaletteItem {
            section: "Go to",
            label: format!("Go to {}", page.label()),
            hint: GO_HINTS[index],
            command: Command::Go(page),
        });
    }
    for (index, preset) in app.presets.iter().enumerate() {
        all.push(PaletteItem {
            section: "Presets",
            label: format!("Apply preset: {}", preset.name),
            hint: "",
            command: Command::Preset(index),
        });
    }
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    all.retain(|item| {
        let label = item.label.to_lowercase();
        words.iter().all(|word| label.contains(word))
    });
    all
}

impl Kestrel {
    pub fn render_palette(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let t = self.theme();
        let palette = self.palette.as_ref()?;
        let input = palette.input.clone();
        let selected = palette.selected;
        let query = input.read(cx).text().to_owned();
        let entries = items(self, &query);
        let selected = selected.min(entries.len().saturating_sub(1));
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut section = "";
        for (index, item) in entries.iter().enumerate() {
            if item.section != section {
                section = item.section;
                rows.push(
                    div()
                        .px(px(10.0))
                        .pt(px(8.0))
                        .pb(px(4.0))
                        .child(label(section, &t))
                        .into_any_element(),
                );
            }
            let command = item.command;
            let on = index == selected;
            rows.push(
                div()
                    .id(("palette-item", index))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(9.0))
                    .rounded(px(8.0))
                    .cursor_pointer()
                    .text_color(if on { t.text } else { t.muted })
                    .when(on, |this| this.bg(t.raised))
                    .hover(move |s| s.bg(t.raised).text_color(t.text))
                    .child(div().child(item.label.clone()))
                    .when(!item.hint.is_empty(), |this| {
                        this.child(
                            div()
                                .flex_1()
                                .flex()
                                .justify_end()
                                .child(kbd(item.hint, &t)),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_command(command, window, cx);
                    }))
                    .into_any_element(),
            );
        }
        if entries.is_empty() {
            rows.push(
                div()
                    .px(px(14.0))
                    .py(px(18.0))
                    .text_color(t.faint)
                    .child("No matching commands")
                    .into_any_element(),
            );
        }
        Some(
            scrim(&t)
                .flex()
                .justify_center()
                .pt(px(110.0))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.close_overlays(window, cx);
                        cx.stop_propagation();
                    }),
                )
                .child(
                    overlay_panel(&t)
                        .w(px(560.0))
                        .max_w(gpui::relative(0.9))
                        .h_auto()
                        .max_h(px(480.0))
                        .key_context(PALETTE_CONTEXT)
                        .on_action(cx.listener(|this, _: &PaletteUp, _, cx| {
                            this.palette_move(false, cx);
                        }))
                        .on_action(cx.listener(|this, _: &PaletteDown, _, cx| {
                            this.palette_move(true, cx);
                        }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .px(px(14.0))
                                .py(px(12.0))
                                .border_b_1()
                                .border_color(t.line)
                                .child(icon("search", 16.0, t.muted))
                                .child(div().flex_1().child(input))
                                .child(kbd("Esc", &t)),
                        )
                        .child(
                            div()
                                .id("palette-list")
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_h_0()
                                .p(px(6.0))
                                .overflow_y_scroll()
                                .children(rows),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Dialog asking for a preset name.
    pub fn render_save_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let t = self.theme();
        let input = self.save_dialog.as_ref()?.input.clone();
        let _ = window;
        Some(
            scrim(&t)
                .flex()
                .justify_center()
                .pt(px(160.0))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.close_overlays(window, cx);
                        cx.stop_propagation();
                    }),
                )
                .child(
                    overlay_panel(&t)
                        .w(px(420.0))
                        .gap(px(14.0))
                        .p(px(18.0))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Save preset as"),
                        )
                        .child(input.clone())
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(8.0))
                                .child(
                                    button("save-cancel", &t, BtnKind::Default, false)
                                        .child("Cancel")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.close_overlays(window, cx);
                                        })),
                                )
                                .child(
                                    button("save-ok", &t, BtnKind::Primary, false)
                                        .child("Save preset")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let name = input.read(cx).text().to_owned();
                                            this.save_preset(&name, cx);
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    pub fn open_save_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(cx);
            input.placeholder("Preset name");
            input
        });
        cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, TextEvent::Submit) {
                let name = input.read(cx).text().to_owned();
                this.save_preset(&name, cx);
            }
        })
        .detach();
        window.focus(&input.focus_handle(cx));
        self.save_dialog = Some(crate::app::SaveDialog { input });
        cx.notify();
    }
}
