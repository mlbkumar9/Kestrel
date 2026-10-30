# LangMux contributor notes

## Project layout

- `LangMux_Native/` is the recommended desktop application: an eframe/egui Rust app with the remux worker and unit tests.
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
- Discover `mkvmerge` on `PATH`, then check `C:\Program Files\MKVToolNix\mkvmerge.exe` on Windows. Surface missing-tool errors in the UI.
- Windows child processes must use `CREATE_NO_WINDOW` (`0x08000000`) through `std::os::windows::process::CommandExt`.
- Keep track selection and path/argument rules in `LangMux_Native/src/mkv.rs`; cover behavior with unit tests and JSON fixtures.
- Persist user preferences through eframe storage. Keep build output under ignored `target/` directories and screenshots outside the repository.
