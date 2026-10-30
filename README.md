# LangMux

**Selective MKV language remuxer** — batch remux Matroska (`.mkv`) files with chosen audio and subtitle tracks by ISO 639-2 language code. The native Rust desktop app is the recommended daily-use interface; the existing Tauri, Python GUI, CLI, and Flask API remain available.

**Requires [MKVToolNix](https://mkvtoolnix.download/)** — `mkvmerge` must be installed and on your PATH (or at the default Windows install path).

---

## Features

- Batch-process every `.mkv` in a folder
- Select audio/subtitle tracks by ISO 639-2 language code (e.g. `eng`, `jpn`)
- Auto-scan folders for available languages
- Skip files that already exist in the output directory
- Dry-run mode to preview matching tracks without writing files
- Live per-file progress (where `mkvmerge` reports it)
- Cancel / stop support — abort kills the active `mkvmerge` process (GUI, Tauri, Flask API)

---

## Prerequisites

| Requirement | Used by |
|-------------|---------|
| **MKVToolNix** (`mkvmerge`) | All entrypoints |
| **Rust toolchain** (`rustup`) | Native and Tauri desktop apps |
| **Python 3.7+** | Python GUI, CLI, and API |
| **Node.js 18+** | Tauri desktop app |

Optional Python packages are listed in `requirements.txt` (Rich, CustomTkinter, Pillow, Flask).

---

## Installation

```bash
git clone https://github.com/mlbkumar9/LangMux.git
cd LangMux
pip install -r requirements.txt
```

For the Tauri desktop app:

```bash
cd REMUX_Tauri
npm install
```

---

## Usage

### 1. Native desktop app (recommended)

Fast native Rust application with a dark egui interface, language chips, batch controls, and logs.

```bash
cd LangMux_Native
cargo run --release
```

Build a platform-specific release binary with `cargo build --release`. The app requires `mkvmerge` on `PATH` (or the default MKVToolNix install path on Windows).

### 2. Tauri desktop app

Modern React desktop app with drag-and-drop, pause/stop, logs, and batch stats.

```bash
cd REMUX_Tauri
npm run tauri dev      # development (Vite on http://localhost:1420)
npm run tauri build    # production installer
```

1. Choose an **input folder** (or drag-and-drop onto the window).
2. Set **output folder** (defaults to `<input>/remuxed` when empty).
3. Under **Settings**, set **audio/subtitle languages** (comma-separated ISO codes).
4. Click **Start Batch** (Space). **Esc** stops after killing the current file’s `mkvmerge` process.

### 3. Classic GUI — `remux_gui.py`

Dark-themed Tkinter interface with progress table and log export.

```bash
python "REMUX Python Scripts/remux_gui.py"
```

Shortcuts: `Ctrl+O` browse input, `Ctrl+S` start, `Esc` cancel (with confirmation).

### 4. Modern GUI — `REMUX_ModernGUI.py`

CustomTkinter UI with sidebar and per-file progress bars.

```bash
python "REMUX Python Scripts/REMUX_ModernGUI.py"
```

CustomTkinter and Pillow are included in `requirements.txt`.

### 5. CLI — `REMUX_Script.py`

Interactive terminal workflow with a Rich live progress table. Writes `remux_log.txt` to the output folder.

```bash
python "REMUX Python Scripts/REMUX_Script.py"
```

Prompts for input/output folders, language selection, skip-existing, and dry-run options.

### 6. Flask API — `backend.py`

JSON REST API for integration or a custom web front-end. Listens on **port 5000** by default.

```bash
python "REMUX Python Scripts/backend.py"
```

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/` | GET | API name, `mkvmerge` availability, endpoint list |
| `/api/scan` | POST | `{"path": "..."}` → file list + detected languages |
| `/api/remux` | POST | Start batch: `files`, `outputDir`, `audioLangs`, `subLangs`, optional `skipExists`, `dryRun` |
| `/api/cancel` | POST | Cancel running batch (terminates current `mkvmerge`) |
| `/api/status` | GET | Current batch status and per-file progress |
| `/api/stream` | GET | SSE progress stream (JSON state every 0.5s) |

`dryRun: true` identifies matching tracks and reports success without writing output files.

Example scan:

```bash
curl -s -X POST http://127.0.0.1:5000/api/scan -H "Content-Type: application/json" -d "{\"path\": \"C:\\\\Videos\\\\mkv\"}"
```

---

## Project structure

```
LangMux/
├── LangMux_Native/            # Recommended native Rust + egui desktop app
│   ├── src/                   # UI, theme, remux logic, and worker
│   ├── tests/fixtures/        # mkvmerge JSON fixtures
│   └── assets/                # Embedded fonts and licenses
├── REMUX Python Scripts/
│   ├── remux_gui.py          # Tkinter GUI (classic)
│   ├── REMUX_ModernGUI.py    # CustomTkinter GUI
│   ├── REMUX_Script.py       # Rich CLI
│   └── backend.py            # Flask API
├── REMUX_Tauri/              # Tauri + React desktop app (LangMux)
│   ├── src/                  # React frontend
│   ├── src-tauri/            # Rust backend
│   └── public/               # Static assets (app-icon.png)
├── requirements.txt
├── README.md
└── LICENSE
```

---

## Language codes

Use three-letter **ISO 639-2** codes: `eng` (English), `jpn` (Japanese), `spa` (Spanish), `fre`/`fra` (French), `ger`/`deu` (German), `und` (undefined/unknown).

If a track has no language tag, it is treated as `und`. Language matching is case-insensitive.

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| `mkvmerge not found` | Install MKVToolNix; add to PATH or use default `C:\Program Files\MKVToolNix\` on Windows |
| No matching audio | Verify language codes match tags shown after scanning |
| Native app build fails | Run `cargo build --release` from `LangMux_Native/`; ensure Rust is installed |
| Tauri dev fails | Run `npm install` in `REMUX_Tauri`; ensure Node.js and Rust are installed |
| Tauri blank window | Confirm Vite is on port 1420 (`npm run tauri dev` starts both) |
| Empty output file | Check disk space and write permissions on the output folder |
| Flask cancel seems stuck | Cancel sets a flag and kills the active `mkvmerge`; wait for the current file to finish aborting |

---

## License

MIT — see [LICENSE](LICENSE).

## Acknowledgments

- [MKVToolNix](https://mkvtoolnix.download/)
- [Rich](https://github.com/Textualize/rich)
- [Tauri](https://tauri.app/)
- [egui](https://www.egui.rs/)
