# MKV Batch Remuxer

A cross-platform toolkit for batch remuxing Matroska (`.mkv`) files with selective audio and subtitle tracks. Pick the interface that fits your workflow: desktop app (Tauri), classic GUI (Tkinter), modern GUI (CustomTkinter), interactive CLI (Rich), or a Flask JSON API.

**Requires [MKVToolNix](https://mkvtoolnix.download/)** — `mkvmerge` must be installed and on your PATH (or at the default Windows install path).

---

## Features

- Batch-process every `.mkv` in a folder
- Select audio/subtitle tracks by ISO 639-2 language code (e.g. `eng`, `jpn`)
- Auto-scan folders for available languages
- Skip files that already exist in the output directory
- Dry-run mode to preview without writing files
- Live per-file progress (where `mkvmerge` reports it)
- Cancel / stop support in GUI and Tauri app

---

## Prerequisites

| Requirement | Notes |
|-------------|-------|
| **Python 3.7+** | For Python scripts |
| **MKVToolNix** | Provides `mkvmerge` |
| **Node.js 18+** | For Tauri desktop app only |
| **Rust toolchain** | For building Tauri (`rustup`) |

---

## Installation

```bash
git clone https://github.com/mlbkumar9/Project_13.git
cd Project_13
pip install -r requirements.txt
```

For the Tauri desktop app:

```bash
cd REMUX_Tauri
npm install
```

---

## Usage

### 1. Tauri Desktop App (recommended UI)

Modern React desktop app with drag-and-drop, pause/stop, logs, and batch stats.

```bash
cd REMUX_Tauri
npm run tauri dev      # development
npm run tauri build    # production installer
```

1. Choose an **input folder** (or drag-and-drop onto the window).
2. Set **output folder** (defaults to `<input>/remuxed`).
3. Configure **audio/subtitle languages** under Settings (comma-separated ISO codes).
4. Click **Start Batch** (Space). Press **Esc** to stop after the current file.

### 2. Classic GUI — `REMUX_GUI.py`

Dark-themed Tkinter interface with progress table and log export.

```bash
python "REMUX Python Scripts/REMUX_GUI.py"
```

Shortcuts: `Ctrl+O` browse input, `Ctrl+S` start, `Esc` cancel.

### 3. Modern GUI — `REMUX_ModernGUI.py`

CustomTkinter UI with sidebar and per-file progress bars.

```bash
pip install customtkinter Pillow   # if not already installed
python "REMUX Python Scripts/REMUX_ModernGUI.py"
```

### 4. CLI — `REMUX_Script.py`

Interactive terminal workflow with a Rich live progress table. Writes `remux_log.txt` to the output folder.

```bash
python "REMUX Python Scripts/REMUX_Script.py"
```

### 5. Flask API — `backend.py`

JSON REST API for integration or a custom web front-end.

```bash
python "REMUX Python Scripts/backend.py"
```

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/scan` | POST | `{"path": "..."}` → file list + detected languages |
| `/api/remux` | POST | Start batch: `files`, `outputDir`, `audioLangs`, `subLangs`, optional `skipExists`, `dryRun` |
| `/api/cancel` | POST | Cancel running batch (kills current `mkvmerge`) |
| `/api/status` | GET | Current batch status |
| `/api/stream` | GET | SSE progress stream |

`dryRun: true` identifies tracks and reports success without writing output files (same behavior as the other UIs).

---

## Project Structure

```
Project_13/
├── REMUX Python Scripts/
│   ├── REMUX_GUI.py          # Tkinter GUI
│   ├── REMUX_ModernGUI.py    # CustomTkinter GUI
│   ├── REMUX_Script.py       # Rich CLI
│   └── backend.py            # Flask API
├── REMUX_Tauri/              # Tauri + React desktop app
│   ├── src/                  # React frontend
│   ├── src-tauri/            # Rust backend
│   └── public/               # Static assets (app-icon.png)
├── requirements.txt
├── README.md
└── LICENSE
```

---

## Language Codes

Use three-letter **ISO 639-2** codes: `eng` (English), `jpn` (Japanese), `spa` (Spanish), `fre`/`fra` (French), `ger`/`deu` (German), `und` (undefined/unknown).

If a track has no language tag, it is treated as `und`.

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| `mkvmerge not found` | Install MKVToolNix; add to PATH or use default `C:\Program Files\MKVToolNix\` on Windows |
| No matching audio | Verify language codes match tags shown after scanning |
| Tauri build fails | Run `npm install` in `REMUX_Tauri`, ensure Rust is installed |
| Empty output file | Check disk space and write permissions on the output folder |

---

## License

MIT — see [LICENSE](LICENSE).

## Acknowledgments

- [MKVToolNix](https://mkvtoolnix.download/)
- [Rich](https://github.com/Textualize/rich)
- [Tauri](https://tauri.app/)
