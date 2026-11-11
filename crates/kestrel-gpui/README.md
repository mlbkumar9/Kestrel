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
