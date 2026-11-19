//! Preset management UI: the context menu (right-click in the sidebar, or the "..." button on
//! the Transcode page), the rename and delete dialogs, import and export.

use gpui::{
    AnyElement, BoxShadow, Context, Entity, Focusable, FontWeight, MouseButton, MouseDownEvent,
    Pixels, Point, SharedString, Window, div, point, prelude::*, px,
};

use crate::app::{Kestrel, Pick};
use crate::state::presets;
use crate::widgets::input::{TextEvent, TextInput};
use crate::widgets::overlay::{overlay_panel, scrim};
use crate::widgets::{BtnKind, TagKind, button, icon};

/// An open preset context menu: which preset, and where the click happened.
#[derive(Clone, Copy, Debug)]
pub struct PresetMenu {
    pub index: usize,
    pub at: Point<Pixels>,
}

/// A preset dialog waiting for an answer.
pub enum PresetDialog {
    Rename {
        id: String,
        input: Entity<TextInput>,
    },
    Delete {
        id: String,
        name: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Rename,
    Duplicate,
    Delete,
    Export,
    ExportAll,
    Import,
}

/// Width of the context menu; used to keep it inside the window.
const MENU_WIDTH: f32 = 208.0;
const MENU_ITEM: f32 = 30.0;

impl Kestrel {
    /// Opens the context menu for `presets[index]` at a window position.
    pub fn open_preset_menu(&mut self, index: usize, at: Point<Pixels>, cx: &mut Context<Self>) {
        if index >= self.presets.len() {
            return;
        }
        self.open_menu = None;
        self.preset_menu = Some(PresetMenu { index, at });
        cx.notify();
    }

    fn preset_menu_entries(&self, index: usize) -> Vec<(Action, &'static str, &'static str, bool)> {
        let user = self
            .presets
            .get(index)
            .is_some_and(|preset| !preset.builtin);
        let any_user = self.presets.iter().any(|preset| !preset.builtin);
        vec![
            (Action::Rename, "file", "Rename...", user),
            (Action::Duplicate, "copy", "Duplicate", true),
            (Action::Delete, "trash", "Delete...", user),
            (Action::Export, "download", "Export preset...", user),
            (Action::Import, "upload", "Import presets...", true),
            (
                Action::ExportAll,
                "download",
                "Export all saved...",
                any_user,
            ),
        ]
    }

    fn run_preset_action(
        &mut self,
        action: Action,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preset_menu = None;
        let Some(preset) = self.presets.get(index).cloned() else {
            return;
        };
        match action {
            Action::Rename => self.open_rename_dialog(preset.id, preset.name, window, cx),
            Action::Duplicate => {
                match presets::duplicate(&mut self.presets, &preset.id, &mut self.next_preset) {
                    Ok(copy) => {
                        let name = self.presets[copy].name.clone();
                        self.toast(format!("Created preset {name}"), TagKind::Ok, cx);
                    }
                    Err(error) => self.toast(error, TagKind::Warn, cx),
                }
            }
            Action::Delete => {
                if preset.builtin {
                    self.toast(
                        format!("{} is built in and cannot be deleted", preset.name),
                        TagKind::Warn,
                        cx,
                    );
                } else {
                    self.preset_dialog = Some(PresetDialog::Delete {
                        id: preset.id,
                        name: preset.name,
                    });
                }
            }
            Action::Export => match presets::export_one(&self.presets, &preset.id) {
                Ok(json) => self.export_text(
                    "Export preset",
                    presets::export_file_name(Some(&preset.name)),
                    json,
                    cx,
                ),
                Err(error) => self.toast(error, TagKind::Warn, cx),
            },
            Action::ExportAll => match presets::export_all(&self.presets) {
                Ok(json) => self.export_text(
                    "Export saved presets",
                    presets::export_file_name(None),
                    json,
                    cx,
                ),
                Err(error) => self.toast(error, TagKind::Warn, cx),
            },
            Action::Import => self.pick(Pick::ImportPresets, cx),
        }
        cx.notify();
    }

    fn open_rename_dialog(
        &mut self,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(cx);
            input.placeholder("Preset name");
            input.set_text(&name, cx);
            input
        });
        cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, TextEvent::Submit) {
                let name = input.read(cx).text().to_owned();
                this.confirm_rename(&name, cx);
            }
        })
        .detach();
        window.focus(&input.focus_handle(cx));
        self.preset_dialog = Some(PresetDialog::Rename { id, input });
    }

    fn confirm_rename(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(PresetDialog::Rename { id, .. }) = &self.preset_dialog else {
            return;
        };
        let id = id.clone();
        match presets::rename(&mut self.presets, &id, name) {
            Ok((old, new)) => {
                if self.form.preset == old {
                    self.form.preset = new.clone();
                }
                self.preset_dialog = None;
                self.toast(format!("Renamed to {new}"), TagKind::Ok, cx);
            }
            // The dialog stays open so the name can be fixed.
            Err(error) => self.toast(error, TagKind::Warn, cx),
        }
        cx.notify();
    }

    fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(PresetDialog::Delete { id, .. }) = self.preset_dialog.take() else {
            return;
        };
        match presets::delete(&mut self.presets, &id) {
            Ok(name) => self.toast(format!("Deleted preset {name}"), TagKind::Ok, cx),
            Err(error) => self.toast(error, TagKind::Warn, cx),
        }
        cx.notify();
    }

    /// Adds the presets of an imported file.
    pub fn import_presets_text(&mut self, text: Result<String, String>, cx: &mut Context<Self>) {
        let result =
            text.and_then(|text| presets::import(&mut self.presets, &text, &mut self.next_preset));
        match result {
            Ok(report) => {
                let mut message = format!(
                    "Imported {} preset{}",
                    report.added,
                    if report.added == 1 { "" } else { "s" }
                );
                if report.renamed > 0 {
                    message.push_str(&format!(
                        " ({} renamed to keep names unique)",
                        report.renamed
                    ));
                }
                self.toast(message, TagKind::Ok, cx);
            }
            Err(error) => self.toast(error, TagKind::Bad, cx),
        }
        cx.notify();
    }

    pub fn render_preset_menu(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.preset_menu?;
        let t = self.theme();
        let entries = self.preset_menu_entries(menu.index);
        let height = entries.len() as f32 * MENU_ITEM + 8.0;
        let viewport = window.viewport_size();
        let left = menu
            .at
            .x
            .min(viewport.width - px(MENU_WIDTH + 8.0))
            .max(px(8.0));
        let top = menu
            .at
            .y
            .min(viewport.height - px(height + 8.0))
            .max(px(8.0));
        let items = entries.into_iter().map(|(action, glyph, text, enabled)| {
            div()
                .id(SharedString::from(format!("preset-menu-{action:?}")))
                .flex()
                .items_center()
                .gap(px(8.0))
                .h(px(MENU_ITEM))
                .px(px(10.0))
                .rounded(px(6.0))
                .text_size(px(13.0))
                .whitespace_nowrap()
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .text_color(t.text)
                        .hover(move |s| s.bg(t.raised))
                })
                .when(!enabled, |this| this.text_color(t.faint))
                .child(icon(glyph, 13.0, if enabled { t.muted } else { t.faint }))
                .child(text)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        if enabled {
                            this.run_preset_action(action, menu.index, window, cx);
                        }
                    }),
                )
        });
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.preset_menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _, _, cx| {
                        this.preset_menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .absolute()
                        .left(left)
                        .top(top)
                        .w(px(MENU_WIDTH))
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
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(items),
                )
                .into_any_element(),
        )
    }

    pub fn render_preset_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = self.theme();
        let (title, body, confirm_label) = match self.preset_dialog.as_ref()? {
            PresetDialog::Rename { input, .. } => {
                ("Rename preset", input.clone().into_any_element(), "Rename")
            }
            PresetDialog::Delete { name, .. } => (
                "Delete preset",
                div()
                    .text_color(t.muted)
                    .child(format!("Delete \"{name}\"? This cannot be undone."))
                    .into_any_element(),
                "Delete",
            ),
        };
        let deleting = matches!(self.preset_dialog, Some(PresetDialog::Delete { .. }));
        Some(
            scrim(&t)
                .flex()
                .items_start()
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
                                .child(title),
                        )
                        .child(body)
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(8.0))
                                .child(
                                    button("preset-dialog-cancel", &t, BtnKind::Default, false)
                                        .child("Cancel")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.close_overlays(window, cx);
                                        })),
                                )
                                .child(
                                    button("preset-dialog-ok", &t, BtnKind::Primary, false)
                                        .child(confirm_label)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if deleting {
                                                this.confirm_delete(cx);
                                            } else if let Some(PresetDialog::Rename {
                                                input, ..
                                            }) = &this.preset_dialog
                                            {
                                                let name = input.read(cx).text().to_owned();
                                                this.confirm_rename(&name, cx);
                                            }
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
