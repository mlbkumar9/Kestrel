# Legacy tools

This page covers the entry points that predate Kestrel 4.0: the egui app and the Python scripts. They remain in the repository and are not part of the 4.0 app. Back to the [README](../README.md).

The Tauri + React app (`REMUX_Tauri/`) was removed on 2026-11-20; its last version is in commit `3fe3cd5`.

## egui app (`kestrel-media`)

The Kestrel 3.x desktop app. It is the app that the v3.0.x GitHub releases ship.

```sh
cargo run -p kestrel-media --release
```

Build a release binary with `cargo build -p kestrel-media --release`. The binary is `target/release/kestrel` (`kestrel.exe` on Windows). The Windows release packages bundle FFmpeg and MKVToolNix; macOS and Linux use the tools on `PATH` or in the platform defaults.

## Python scripts (`REMUX Python Scripts/`)

These need Python 3.7 or newer and `mkvmerge`. Install the optional packages with `pip install -r requirements.txt` (Rich, CustomTkinter, Pillow, Flask).

| Script | Command | Notes |
|---|---|---|
| `remux_gui.py` | `python "REMUX Python Scripts/remux_gui.py"` | Tkinter GUI. `Ctrl+O` browse input, `Ctrl+S` start, `Esc` cancel. |
| `REMUX_ModernGUI.py` | `python "REMUX Python Scripts/REMUX_ModernGUI.py"` | CustomTkinter UI with per-file progress bars. |
| `REMUX_Script.py` | `python "REMUX Python Scripts/REMUX_Script.py"` | Interactive terminal workflow with a Rich progress table. Writes `remux_log.txt` to the output folder. |
| `backend.py` | `python "REMUX Python Scripts/backend.py"` | Flask JSON API on port 5000. |

### Flask API

| Endpoint | Method | Description |
|---|---|---|
| `/` | GET | API name, `mkvmerge` availability, endpoint list |
| `/api/scan` | POST | `{"path": "..."}` returns a file list and detected languages |
| `/api/remux` | POST | Start a batch: `files`, `outputDir`, `audioLangs`, `subLangs`, optional `skipExists`, `dryRun` |
| `/api/cancel` | POST | Cancel the running batch (terminates the current `mkvmerge`) |
| `/api/status` | GET | Current batch status and per-file progress |
| `/api/stream` | GET | SSE progress stream (JSON state every 0.5 s) |

`dryRun: true` identifies matching tracks and reports success without writing output files.

```bash
curl -s -X POST http://127.0.0.1:5000/api/scan -H "Content-Type: application/json" -d "{\"path\": \"C:\\Videos\\mkv\"}"
```

### Language codes

These tools use three-letter ISO 639-2 codes: `eng`, `jpn`, `spa`, `fre`/`fra`, `ger`/`deu`, `und`. A track without a language tag counts as `und`. Matching ignores case.

### Troubleshooting

| Problem | Fix |
|---|---|
| `mkvmerge not found` in Python | Install MKVToolNix and add it to `PATH`, or use the default `C:\Program Files\MKVToolNix\` on Windows |
| No matching audio | Check that the language codes match the tags shown after scanning |
| Empty output file | Check disk space and write permissions on the output folder |
| Flask cancel seems stuck | Cancel sets a flag and kills the active `mkvmerge`; wait for the current file to finish aborting |
