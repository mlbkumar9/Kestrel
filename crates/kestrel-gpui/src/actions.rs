//! Keyboard actions and their default bindings.

use gpui::{App, KeyBinding, actions};

actions!(
    kestrel,
    [
        OpenPalette,
        AddFiles,
        AddToQueue,
        Dismiss,
        PaletteUp,
        PaletteDown,
        GoLibrary,
        GoRemux,
        GoTranscode,
        GoQueue,
        GoActivity,
        GoMetrics,
    ]
);

pub const PALETTE_CONTEXT: &str = "KestrelPalette";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-k", OpenPalette, None),
        KeyBinding::new("ctrl-o", AddFiles, None),
        KeyBinding::new("ctrl-enter", AddToQueue, None),
        KeyBinding::new("escape", Dismiss, None),
        KeyBinding::new("up", PaletteUp, Some(PALETTE_CONTEXT)),
        KeyBinding::new("down", PaletteDown, Some(PALETTE_CONTEXT)),
        KeyBinding::new("ctrl-1", GoLibrary, None),
        KeyBinding::new("ctrl-2", GoRemux, None),
        KeyBinding::new("ctrl-3", GoTranscode, None),
        KeyBinding::new("ctrl-4", GoQueue, None),
        KeyBinding::new("ctrl-5", GoActivity, None),
        KeyBinding::new("ctrl-6", GoMetrics, None),
    ]);
}
