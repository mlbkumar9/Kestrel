# Kestrel contributor notes

## Project layout

- `LangMux_Native/` is the Kestrel desktop application: an eframe/egui Rust app with the remux worker and unit tests.
- `REMUX_Tauri/` is the existing Tauri + React desktop app.
- `REMUX Python Scripts/` contains the Tkinter, CustomTkinter, CLI, and Flask entrypoints.
- Keep changes to the native app isolated from the existing Tauri and Python applications unless a task explicitly asks otherwise.

## Native app commands

Run from `LangMux_Native/`:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
cargo run --release
```

## Conventions and gotchas

- Keep UI work on the eframe thread. Run scans and remux batches on `std::thread` workers and send events over `std::sync::mpsc`; do not add an async runtime or continuous idle repainting.
- Discover `mkvmerge`, `ffmpeg`, and `ffprobe` through `LangMux_Native/src/tools.rs`: executable-adjacent bundles, PATH, then platform defaults. Keep tool version probing and Windows process creation there.
- Keep MKVToolNix and FFmpeg stream-copy behavior in `LangMux_Native/src/mkv.rs`; use FFprobe metadata when `mkvmerge -J` fails, preserving both MKVToolNix IDs and FFmpeg stream indices.
- Windows child processes must use `CREATE_NO_WINDOW` (`0x08000000`) through `std::os::windows::process::CommandExt`.
- Persist the selected remux engine and other user preferences through eframe storage. Keep scans/remux work on `std::thread` workers and report progress over the event channel.
- Release packaging is defined by `.github/workflows/release.yml` and `scripts/package-windows.ps1` / `scripts/package-unix.sh`; Windows packages bundle licensed FFmpeg and MKVToolNix tools, while macOS/Linux use system tools.
- Keep track selection and path/argument rules covered by unit tests and JSON fixtures. Keep build output under ignored `target/` directories and screenshots outside the repository.
