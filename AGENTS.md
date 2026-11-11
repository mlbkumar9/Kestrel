# Kestrel contributor notes

## Project layout

- `crates/kestrel-app/` is the Kestrel desktop application: an eframe/egui Rust app with the remux worker and unit tests.
- `crates/kestrel-core/` is the placeholder for the UI-independent engine.
- `crates/kestrel-gpui/` is the GPUI desktop shell with local widgets and themes.
- `REMUX_Tauri/` is the existing Tauri + React desktop app.
- `REMUX Python Scripts/` contains the Tkinter, CustomTkinter, CLI, and Flask entrypoints.
- Keep changes to the native app isolated from the existing Tauri and Python applications unless a task explicitly asks otherwise.

## Native app commands

Run from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p kestrel-media --release
cargo run -p kestrel-media --release
cargo run -p kestrel-gpui
```

The GPUI Linux shell needs `libxkbcommon-dev`, `libwayland-dev`,
`libxkbcommon-x11-dev`, `libfontconfig1-dev`, and `mesa-vulkan-drivers`.
Run with a valid, user-owned `XDG_RUNTIME_DIR` (mode 0700) and `DISPLAY=:0`.
Without a GPU, set
`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json`.

`vendor/xattr` patches xattr 0.2.3 for GPUI's zed-async-tar dependency:
use libc's ENODATA instead of ENOATTR on Linux/Android only; keep libc's ENOATTR
on macOS/BSD. Keep its licenses
and exclude it from workspace formatting and linting.

Build GPUI Windows binaries on Windows with `cargo build --release -p kestrel-gpui`
using the MSVC Rust toolchain, Visual Studio C++ build tools, and Windows SDK.
GPUI's build script needs the SDK's `fxc.exe` for release shaders and `rc.exe`
for its default `windows-manifest` feature. A Linux cross-build fails with missing
`shaders_bytes.rs`. No GPUI fork is used. Linux builds, the egui Windows
cross-build, and MSI packaging stay on Linux; the egui cross-build remains
supported until phase 4 removes egui.

## Conventions and gotchas

- Keep UI work on the eframe thread. Run scans and remux batches on `std::thread` workers and send events over `std::sync::mpsc`; do not add an async runtime or continuous idle repainting.
- Discover `mkvmerge`, `ffmpeg`, and `ffprobe` through `crates/kestrel-app/src/tools.rs`: executable-adjacent bundles, PATH, then platform defaults. Keep tool version probing and Windows process creation there.
- Keep MKVToolNix and FFmpeg stream-copy behavior in `crates/kestrel-app/src/mkv.rs`; use FFprobe metadata when `mkvmerge -J` fails, preserving both MKVToolNix IDs and FFmpeg stream indices.
- Windows child processes must use `CREATE_NO_WINDOW` (`0x08000000`) through `std::os::windows::process::CommandExt`.
- Persist the selected remux engine and other user preferences through eframe storage. Keep scans/remux work on `std::thread` workers and report progress over the event channel.
- Release packaging is defined by `.github/workflows/release.yml` and `scripts/package-windows.ps1` / `scripts/package-unix.sh`; Windows packages bundle licensed FFmpeg and MKVToolNix tools, while macOS/Linux use system tools.
- Keep track selection and path/argument rules covered by unit tests and JSON fixtures. Keep build output under ignored `target/` directories and screenshots outside the repository.
