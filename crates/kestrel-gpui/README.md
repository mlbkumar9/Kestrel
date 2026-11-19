# Kestrel GPUI shell

A GPUI 0.2.2 desktop shell with local theme and navigation state. Media processing
is not connected yet. Widgets are implemented locally without a component crate.

Run from the repository root with `cargo run -p kestrel-gpui`.
See the root AGENTS.md for Linux dependencies and software-rendering settings.

## Screenshots on headless Linux

Build with `cargo build --release -p kestrel-gpui`. Use Xvfb without Composite
and a window manager so GPUI uses a depth-24 visual and receives visibility events:

```sh
Xvfb :98 -screen 0 1280x800x24 -extension Composite &
DISPLAY=:98 openbox &
export XDG_RUNTIME_DIR="/tmp/xdg-$(id -u)"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
DISPLAY=:98 VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
  target/release/kestrel-gpui &
DISPLAY=:98 xdotool search --sync --onlyvisible --name '^Kestrel$'
DISPLAY=:98 import -window <window-id> /home/ubuntu/screens/dashboard-dark-expanded.png
```

Create the screenshot directory first. Use the window ID printed by xdotool.
Composite-enabled displays may capture transparent or black content.

## Layout

The app follows the approved prototype `plan/design/Main.dc.html`.

- `src/main.rs`: window, fonts, asset source, CLI file arguments (`kestrel-gpui <media>...`).
- `src/app.rs`: root view `Kestrel`, background events, queue and probe wiring.
- `src/shell.rs`: sidebar, header, footer, banners and toasts. `src/palette.rs`: Ctrl+K palette.
- `src/state/`: view models (`TranscodeForm`, `RemuxTrackEdit`, library, queue, activity).
  Core mappings: `state/transcode.rs` (form to `JobSettings` and back) and `state/remux.rs`
  (track edits and added files to `JobSettings.track_edits`).
- `src/pages/`: Library, Remux, Transcode, Queue, Activity, Metrics (disabled 4.1 placeholder), Settings.
- `src/widgets/`: own widgets (buttons, selects, text input, sliders, tabs, tables, overlays).
- `assets/icons/*.svg`: embedded icon set loaded through the `AssetSource` in `src/assets.rs`.

Shortcuts: Ctrl+K palette, Ctrl+O add media, Ctrl+Enter queue the current transcode, Ctrl+1..6 pages,
Ctrl+B sidebar, Alt+Left and Alt+Right (or mouse buttons 4 and 5) page history.

The top bar is client-drawn (transparent title bar): sidebar toggle, back and forward, breadcrumb, command
palette field, queue status pill, Activity bell with an unread counter, theme, Add media, Start or Pause queue,
and on Windows and Linux the window buttons. Drag regions are siblings of the buttons, never parents.

## Logo

The sidebar logo is `assets/kestrel-mark.svg` when that file exists at build time
(`build.rs` sets the `kestrel_mark` cfg). Otherwise it falls back to `kestrel-app/assets/kestrel.png`.

## Debug environment variables

| Variable | Effect |
|---|---|
| `KESTREL_DEBUG_PAGE` | open on `library`, `remux`, `transcode`, `queue`, `activity`, `metrics` or `settings` |
| `KESTREL_DEBUG_THEME` | `dark`, `light` or `system` |
| `KESTREL_DEBUG_TAB` | Transcode tab name (for example `Audio`) |
| `KESTREL_DEBUG_PALETTE` | `1` opens the command palette |
| `KESTREL_DEBUG_QUEUE` | `1` queues the loaded files once probed, `run` also starts the queue |
| `KESTREL_DEBUG_LEGACY_BANNER` | `1` shows the legacy-install banner |
