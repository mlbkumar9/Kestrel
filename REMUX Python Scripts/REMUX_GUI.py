#!/usr/bin/env python3
"""
remux_gui.py
A GUI for the batch remuxing script with automatic language detection, built with tkinter.
Modern dark mode Tkinter UI with progress table and log export.
"""

import json
import logging
import os
import queue
import shutil
import subprocess
import sys
import threading
import time
import re
from pathlib import Path
from typing import Optional, Dict, List
import tkinter as tk
from tkinter import ttk, filedialog, messagebox, scrolledtext

# --- Constants ---
MKVMERGE_TIMEOUT = 10.0
PROGRESS_UPDATE_INTERVAL = 100  # milliseconds
DEFAULT_WINDOW_SIZE = "1100x720"
MIN_WINDOW_SIZE = (900, 600)

# --- Dark Theme Colors ---
DARK_BG = "#1e1e1e"  # Main background
DARK_FG = "#d4d4d4"  # Main foreground text
DARK_FRAME = "#252526"  # Frame background
DARK_ENTRY = "#3c3c3c"  # Entry field background
DARK_SELECT = "#0078d4"  # Selection/accent color
DARK_HOVER = "#2d2d30"  # Hover state
DARK_BORDER = "#3e3e42"  # Border color
DARK_SUCCESS = "#4ec9b0"  # Success green
DARK_ERROR = "#f48771"  # Error red
DARK_WARNING = "#dcdcaa"  # Warning yellow
DARK_INFO = "#4fc1ff"  # Info blue

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s',
    handlers=[
        logging.FileHandler('remux_gui.log', encoding='utf-8'),
        logging.StreamHandler()
    ]
)
logger = logging.getLogger(__name__)

# --- Core Remuxing Logic ---

def find_mkvmerge() -> Optional[str]:
    """Find mkvmerge executable in system PATH or common installation locations."""
    mk = shutil.which("mkvmerge") or shutil.which("mkvmerge.exe")
    if mk:
        return mk
    candidate = r"C:\Program Files\MKVToolNix\mkvmerge.exe"
    return candidate if Path(candidate).exists() else None

def identify_tracks(mkvmerge_path: str, src: Path, timeout: float = MKVMERGE_TIMEOUT):
    """Identify tracks in an MKV file using mkvmerge."""
    cmd = [mkvmerge_path, "--identify", "--identification-format", "json", str(src)]
    try:
        res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, 
                           text=True, timeout=timeout, check=False)
        if res.returncode != 0:
            return {"ok": False, "err": res.stderr.strip() or res.stdout.strip() or f"rc={res.returncode}"}
        return {"ok": True, "data": json.loads(res.stdout)}
    except Exception as e:
        logger.error(f"Error identifying tracks for {src}: {e}")
        return {"ok": False, "err": str(e)}

def get_available_languages(mkvmerge_path: str, files: list):
    """Scan all files and return available audio and subtitle languages."""
    all_audio_langs = set()
    all_sub_langs = set()
    
    for file in files:
        ident = identify_tracks(mkvmerge_path, file)
        if not ident["ok"]:
            continue
        
        for track in ident["data"].get("tracks", []):
            track_type = track.get("type", "").lower()
            lang = track.get("properties", {}).get("language", "und").lower()
            
            if track_type == "audio":
                all_audio_langs.add(lang)
            elif track_type in ("subtitles", "subtitle"):
                all_sub_langs.add(lang)
    
    return sorted(all_audio_langs), sorted(all_sub_langs)

def fmt_time(sec: Optional[float]) -> str:
    """Format seconds as MM:SS."""
    if sec is None:
        return "--:--"
    try:
        s = int(round(sec))
        m, s = divmod(s, 60)
        return f"{m:02d}:{s:02d}"
    except (ValueError, TypeError):
        return "--:--"

def fmt_size(size_bytes: int) -> str:
    """Format file size in human-readable format."""
    for unit in ['B', 'KB', 'MB', 'GB', 'TB']:
        if size_bytes < 1024.0:
            return f"{size_bytes:.1f} {unit}"
        size_bytes /= 1024.0
    return f"{size_bytes:.1f} PB"

def is_writable(path: Path) -> bool:
    """Check if a directory path is writable."""
    try:
        if not path.exists():
            path.mkdir(parents=True, exist_ok=True)
        test_file = path / ".write_test"
        test_file.write_text("test")
        test_file.unlink()
        return True
    except Exception:
        return False

def remux_file(mkvmerge_path: str, src_path: Path, out_path: Path, audio_langs: list, sub_langs: list,
               skip_if_exists: bool, dry_run: bool, update_queue: queue.Queue, row_idx: int, 
               cancel_event: threading.Event):
    """
    Performs the remux operation for a single file and sends progress updates to the GUI queue.
    """
    start_time = time.time()
    proc = None
    
    def send_update(data):
        data['row_idx'] = row_idx
        update_queue.put(data)

    send_update({"status": "Pending", "pct": 0, "elapsed": 0, "remaining": None})

    # Check for cancellation
    if cancel_event.is_set():
        send_update({"status": "Cancelled", "pct": 0, "finished": True, "success": False})
        return

    if skip_if_exists and out_path.exists() and out_path.stat().st_size > 0:
        send_update({"status": "Skipped (exists)", "pct": 100, "elapsed": 0, "finished": True, "success": True})
        return

    ident = identify_tracks(mkvmerge_path, src_path)
    if not ident["ok"]:
        send_update({"status": f"ID Failed: {ident.get('err', 'Unknown error')}", "pct": 0, "finished": True, "success": False, "error": ident.get('err', 'Unknown error')})
        return

    audio_ids = [
        t["id"] for t in ident["data"].get("tracks", [])
        if t.get("type") == "audio" and (t.get("properties", {}).get("language") or "und").lower() in audio_langs
    ]
    sub_ids = [
        t["id"] for t in ident["data"].get("tracks", [])
        if t.get("type") in ("subtitles", "subtitle") and (t.get("properties", {}).get("language") or "und").lower() in sub_langs
    ]

    if not audio_ids:
        send_update({"status": "No desired audio", "pct": 0, "finished": True, "success": False, "error": "No audio tracks matching selected languages found"})
        return

    cmd = [mkvmerge_path, "-o", str(out_path), "--audio-tracks", ",".join(map(str, audio_ids))]
    if sub_ids:
        cmd += ["--subtitle-tracks", ",".join(map(str, sub_ids))]
    else:
        cmd += ["--no-subtitles"]
    cmd += ["--ui-language", "en", str(src_path)]

    if dry_run:
        send_update({"status": "Dry-run (skipped)", "pct": 100, "elapsed": 0, "finished": True, "success": True})
        return

    try:
        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                text=True, bufsize=1, encoding="utf-8", errors="replace")
    except Exception as e:
        logger.error(f"Failed to launch mkvmerge for {src_path}: {e}")
        send_update({"status": f"Launch failed: {str(e)}", "pct": 0, "finished": True, "success": False, "error": str(e)})
        return

    re_progress = re.compile(r"(?:#GUI#progress|Progress:)\s*([0-9]{1,3})")
    error_lines = []
    
    try:
        for line in iter(proc.stdout.readline, ""):
            if cancel_event.is_set():
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                send_update({"status": "Cancelled", "pct": 0, "finished": True, "success": False})
                return
            
            # Collect error lines
            if "error" in line.lower() or "warning" in line.lower():
                error_lines.append(line.strip())
            
            m = re_progress.search(line)
            if m:
                pct = int(m.group(1))
                # Clamp percentage to valid range [0, 100] to prevent negative remaining time
                if pct < 0:
                    pct = 0
                if pct > 100:
                    pct = 100
                elapsed = time.time() - start_time
                remaining = (elapsed * (100 - pct) / pct) if pct > 0 else None
                send_update({"status": "Processing", "pct": pct, "elapsed": elapsed, "remaining": remaining})
    except Exception as e:
        logger.error(f"Error reading mkvmerge output for {src_path}: {e}")
    
    proc.wait()
    final_elapsed = time.time() - start_time

    if cancel_event.is_set():
        send_update({"status": "Cancelled", "pct": 0, "finished": True, "success": False})
        return

    if proc.returncode == 0 and out_path.exists() and out_path.stat().st_size > 0:
        send_update({"status": "OK", "pct": 100, "elapsed": final_elapsed, "remaining": 0, "finished": True, "success": True})
    else:
        error_msg = "\n".join(error_lines[-5:]) if error_lines else f"Return code: {proc.returncode}"
        send_update({"status": f"FAILED (rc={proc.returncode})", "elapsed": final_elapsed, "finished": True, "success": False, "error": error_msg})


# --- Tooltip Class ---

class ToolTip:
    """Create a dark-themed tooltip for a given widget."""
    def __init__(self, widget, text='widget info'):
        self.widget = widget
        self.text = text
        self.tipwindow = None
        self.id = None
        self.x = self.y = 0
        self.widget.bind('<Enter>', self.enter)
        self.widget.bind('<Leave>', self.leave)

    def enter(self, event=None):
        self.schedule()

    def leave(self, event=None):
        self.unschedule()
        self.hidetip()

    def schedule(self):
        self.unschedule()
        self.id = self.widget.after(500, self.showtip)

    def unschedule(self):
        id = self.id
        self.id = None
        if id:
            self.widget.after_cancel(id)

    def showtip(self, event=None):
        x, y, cx, cy = self.widget.bbox("insert") if hasattr(self.widget, 'bbox') else (0, 0, 0, 0)
        x += self.widget.winfo_rootx() + 25
        y += self.widget.winfo_rooty() + 20
        self.tipwindow = tw = tk.Toplevel(self.widget)
        tw.wm_overrideredirect(True)
        tw.wm_geometry("+%d+%d" % (x, y))
        tw.configure(bg=DARK_FRAME)
        label = tk.Label(tw, text=self.text, justify=tk.LEFT,
                        background=DARK_FRAME, foreground=DARK_FG,
                        relief=tk.SOLID, borderwidth=1,
                        font=("Segoe UI", "9", "normal"),
                        padx=8, pady=4)
        label.pack(ipadx=1)

    def hidetip(self):
        tw = self.tipwindow
        self.tipwindow = None
        if tw:
            tw.destroy()


# --- GUI Application ---

class RemuxApp:
    def __init__(self, root):
        self.root = root
        self.root.title("MKV Batch Remuxer")
        self.root.geometry(DEFAULT_WINDOW_SIZE)
        self.root.minsize(*MIN_WINDOW_SIZE)
        
        # Configure dark theme for root window
        self.root.configure(bg=DARK_BG)
        
        # Try to set dark title bar on Windows
        try:
            if sys.platform == "win32":
                import ctypes
                DWMWA_USE_IMMERSIVE_DARK_MODE = 20
                hwnd = ctypes.windll.user32.GetParent(root.winfo_id())
                ctypes.windll.dwmapi.DwmSetWindowAttribute(
                    hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, 
                    ctypes.byref(ctypes.c_int(1)), 
                    ctypes.sizeof(ctypes.c_int)
                )
        except Exception:
            pass  # Fallback if dark mode API not available

        self.mkvmerge_path = find_mkvmerge()
        self.update_queue = queue.Queue()
        self.running = False
        self.cancel_event = threading.Event()
        self.active_processes: List[subprocess.Popen] = []
        self.available_audio_langs = []
        self.available_sub_langs = []
        self.file_map: Dict[int, Dict] = {}
        self.error_log: List[Dict] = []
        self.batch_start_time = None

        # Setup keyboard shortcuts
        self.root.bind('<Control-o>', lambda e: self.browse_input())
        self.root.bind('<Control-s>', lambda e: self.start_remux() if not self.running else None)
        self.root.bind('<Escape>', lambda e: self.cancel_remux() if self.running else None)

        # Configure dark theme styles and build UI
        self.setup_dark_theme()
        self.build_ui()
    
    def setup_dark_theme(self):
        """Configure dark theme styles for ttk widgets."""
        style = ttk.Style()
        style.theme_use('clam')
        
        # Configure styles
        style.configure('TFrame', background=DARK_FRAME)
        style.configure('TLabelFrame', background=DARK_FRAME, foreground=DARK_FG, borderwidth=1)
        style.configure('TLabelFrame.Label', background=DARK_FRAME, foreground=DARK_FG)
        style.configure('TLabel', background=DARK_FRAME, foreground=DARK_FG)
        style.configure('TEntry', fieldbackground=DARK_ENTRY, foreground=DARK_FG, borderwidth=1)
        style.configure('TButton', background=DARK_ENTRY, foreground=DARK_FG, borderwidth=0, padding=8)
        style.map('TButton', background=[('active', DARK_HOVER), ('pressed', DARK_SELECT)])
        style.configure('TCheckbutton', background=DARK_FRAME, foreground=DARK_FG)
        style.map('TCheckbutton', background=[('active', DARK_FRAME)])
        style.configure('TTreeview', background=DARK_ENTRY, foreground=DARK_FG, fieldbackground=DARK_ENTRY)
        style.configure('TScrollbar', background=DARK_FRAME, troughcolor=DARK_BG, borderwidth=0)
    
    def create_button(self, parent, text, command, style='primary'):
        """Create a styled button with hover effects."""
        btn = tk.Button(parent, text=text, command=command,
                       font=("Segoe UI", "9", "bold"),
                       bg=DARK_SELECT if style == 'primary' else DARK_ENTRY,
                       fg="#ffffff" if style == 'primary' else DARK_FG,
                       activebackground=DARK_HOVER,
                       activeforeground="#ffffff" if style == 'primary' else DARK_FG,
                       relief=tk.FLAT, bd=0,
                       padx=16, pady=8,
                       cursor="hand2",
                       disabledforeground="#666666",
                       disabledbackground="#3a3a3a")
        
        def on_enter(e):
            if btn['state'] != 'disabled':
                btn.config(bg=DARK_INFO if style == 'primary' else DARK_HOVER)
        
        def on_leave(e):
            if btn['state'] != 'disabled':
                btn.config(bg=DARK_SELECT if style == 'primary' else DARK_ENTRY)
        
        btn.bind("<Enter>", on_enter)
        btn.bind("<Leave>", on_leave)
        return btn

    def build_ui(self):
        """Construct the main application widgets."""
        root = self.root

        # --- UI Setup ---
        main_frame = tk.Frame(root, bg=DARK_BG, padx=20, pady=15)
        main_frame.pack(fill=tk.BOTH, expand=True)

        # Input/Output Frame
        io_frame = tk.LabelFrame(main_frame, text=" 📁 Directories ", 
                                font=("Segoe UI", "10", "bold"),
                                bg=DARK_FRAME, fg=DARK_FG,
                                padx=15, pady=12,
                                relief=tk.FLAT, bd=1, highlightthickness=1,
                                highlightbackground=DARK_BORDER, highlightcolor=DARK_SELECT)
        io_frame.pack(fill=tk.X, pady=8)
        io_frame.columnconfigure(1, weight=1)

        tk.Label(io_frame, text="Input:", font=("Segoe UI", "9"),
                bg=DARK_FRAME, fg=DARK_FG).grid(row=0, column=0, sticky=tk.W, padx=8, pady=8)
        self.input_dir = tk.StringVar()
        input_entry = tk.Entry(io_frame, textvariable=self.input_dir,
                              font=("Segoe UI", "9"),
                              bg=DARK_ENTRY, fg=DARK_FG,
                              insertbackground=DARK_FG,
                              relief=tk.FLAT, bd=5,
                              highlightthickness=1, highlightbackground=DARK_BORDER,
                              highlightcolor=DARK_SELECT)
        input_entry.grid(row=0, column=1, sticky=tk.EW, padx=8, pady=8, ipady=6)
        ToolTip(input_entry, "Select the directory containing MKV files to remux (Ctrl+O)")
        self.create_button(io_frame, "Browse...", self.browse_input).grid(row=0, column=2, padx=8, pady=8)

        tk.Label(io_frame, text="Output:", font=("Segoe UI", "9"),
                bg=DARK_FRAME, fg=DARK_FG).grid(row=1, column=0, sticky=tk.W, padx=8, pady=8)
        self.output_dir = tk.StringVar()
        output_entry = tk.Entry(io_frame, textvariable=self.output_dir,
                               font=("Segoe UI", "9"),
                               bg=DARK_ENTRY, fg=DARK_FG,
                               insertbackground=DARK_FG,
                               relief=tk.FLAT, bd=5,
                               highlightthickness=1, highlightbackground=DARK_BORDER,
                               highlightcolor=DARK_SELECT)
        output_entry.grid(row=1, column=1, sticky=tk.EW, padx=8, pady=8, ipady=6)
        ToolTip(output_entry, "Select where remuxed files will be saved")
        self.create_button(io_frame, "Browse...", self.browse_output).grid(row=1, column=2, padx=8, pady=8)

        # Available Languages Display Frame
        avail_frame = tk.LabelFrame(main_frame, text=" 🌐 Available Languages in MKV Files ",
                                    font=("Segoe UI", "10", "bold"),
                                    bg=DARK_FRAME, fg=DARK_FG,
                                    padx=15, pady=12,
                                    relief=tk.FLAT, bd=1, highlightthickness=1,
                                    highlightbackground=DARK_BORDER, highlightcolor=DARK_SELECT)
        avail_frame.pack(fill=tk.X, pady=8)
        
        # Audio languages display
        tk.Label(avail_frame, text="Audio:", font=("Segoe UI", "9"),
              bg=DARK_FRAME, fg=DARK_FG).grid(row=0, column=0, sticky=tk.W, padx=8, pady=8)
        self.avail_audio_label = tk.Label(avail_frame, text="Select input directory to scan...",
                                         font=("Segoe UI", "9"),
                                         bg=DARK_ENTRY, fg="#888888",
                                         relief=tk.FLAT, padx=10, pady=8,
                                         anchor=tk.W)
        self.avail_audio_label.grid(row=0, column=1, sticky=tk.EW, padx=8, pady=8)
        
        # Subtitle languages display
        tk.Label(avail_frame, text="Subtitles:", font=("Segoe UI", "9"),
              bg=DARK_FRAME, fg=DARK_FG).grid(row=1, column=0, sticky=tk.W, padx=8, pady=8)
        self.avail_sub_label = tk.Label(avail_frame, text="Select input directory to scan...",
                                       font=("Segoe UI", "9"),
                                       bg=DARK_ENTRY, fg="#888888",
                                       relief=tk.FLAT, padx=10, pady=8,
                                       anchor=tk.W)
        self.avail_sub_label.grid(row=1, column=1, sticky=tk.EW, padx=8, pady=8)
        
        avail_frame.columnconfigure(1, weight=1)

        # Options Frame
        opts_frame = tk.LabelFrame(main_frame, text=" ⚙️ Language Selection ",
                                  font=("Segoe UI", "10", "bold"),
                                  bg=DARK_FRAME, fg=DARK_FG,
                                  padx=15, pady=12,
                                  relief=tk.FLAT, bd=1, highlightthickness=1,
                                  highlightbackground=DARK_BORDER, highlightcolor=DARK_SELECT)
        opts_frame.pack(fill=tk.X, pady=8)
        opts_frame.columnconfigure(1, weight=1)
        opts_frame.columnconfigure(3, weight=1)

        tk.Label(opts_frame, text="Audio Langs:", font=("Segoe UI", "9"),
                bg=DARK_FRAME, fg=DARK_FG).grid(row=0, column=0, sticky=tk.W, padx=8, pady=6)
        self.audio_langs = tk.StringVar(value="eng")
        audio_entry = tk.Entry(opts_frame, textvariable=self.audio_langs,
                              font=("Segoe UI", "9"),
                              bg=DARK_ENTRY, fg=DARK_FG,
                              insertbackground=DARK_FG,
                              relief=tk.FLAT, bd=5,
                              highlightthickness=1, highlightbackground=DARK_BORDER,
                              highlightcolor=DARK_SELECT)
        audio_entry.grid(row=0, column=1, sticky=tk.EW, padx=8, pady=6, ipady=6)
        ToolTip(audio_entry, "Comma-separated ISO 639-2 language codes (e.g., eng,jpn,spa)")
        tk.Label(opts_frame, text="(comma-separated, e.g., eng,jpn)",
                font=("Segoe UI", "8"),
                bg=DARK_FRAME, fg="#888888").grid(row=1, column=1, sticky=tk.W, padx=8)

        tk.Label(opts_frame, text="Subtitle Langs:", font=("Segoe UI", "9"),
                bg=DARK_FRAME, fg=DARK_FG).grid(row=0, column=2, sticky=tk.W, padx=8, pady=6)
        self.sub_langs = tk.StringVar(value="eng")
        sub_entry = tk.Entry(opts_frame, textvariable=self.sub_langs,
                            font=("Segoe UI", "9"),
                            bg=DARK_ENTRY, fg=DARK_FG,
                            insertbackground=DARK_FG,
                            relief=tk.FLAT, bd=5,
                            highlightthickness=1, highlightbackground=DARK_BORDER,
                            highlightcolor=DARK_SELECT)
        sub_entry.grid(row=0, column=3, sticky=tk.EW, padx=8, pady=6, ipady=6)
        ToolTip(sub_entry, "Comma-separated ISO 639-2 language codes for subtitles")
        tk.Label(opts_frame, text="(comma-separated, e.g., eng,ger)",
                font=("Segoe UI", "8"),
                bg=DARK_FRAME, fg="#888888").grid(row=1, column=3, sticky=tk.W, padx=8)

        # Quick select buttons
        btn_frame = tk.Frame(opts_frame, bg=DARK_FRAME)
        btn_frame.grid(row=2, column=0, columnspan=4, pady=12)
        
        self.create_button(btn_frame, "Select All Audio", self.select_all_audio, 'secondary').pack(side=tk.LEFT, padx=4)
        self.create_button(btn_frame, "Select All Subtitles", self.select_all_subs, 'secondary').pack(side=tk.LEFT, padx=4)
        self.create_button(btn_frame, "Clear Audio", lambda: self.audio_langs.set(""), 'secondary').pack(side=tk.LEFT, padx=4)
        self.create_button(btn_frame, "Clear Subtitles", lambda: self.sub_langs.set(""), 'secondary').pack(side=tk.LEFT, padx=4)

        self.skip_exists = tk.BooleanVar(value=True)
        skip_cb = tk.Checkbutton(opts_frame, text="Skip if output exists", variable=self.skip_exists,
                                 font=("Segoe UI", "9"),
                                 bg=DARK_FRAME, fg=DARK_FG,
                                 activebackground=DARK_FRAME, activeforeground=DARK_FG,
                                 selectcolor=DARK_BG,
                                 cursor="hand2")
        skip_cb.grid(row=3, column=0, columnspan=2, sticky=tk.W, padx=8, pady=8)
        ToolTip(skip_cb, "Skip files that already exist in the output directory")
        
        self.dry_run = tk.BooleanVar(value=False)
        dry_cb = tk.Checkbutton(opts_frame, text="Dry-run only", variable=self.dry_run,
                               font=("Segoe UI", "9"),
                               bg=DARK_FRAME, fg=DARK_FG,
                               activebackground=DARK_FRAME, activeforeground=DARK_FG,
                               selectcolor=DARK_BG,
                               cursor="hand2")
        dry_cb.grid(row=3, column=2, columnspan=2, sticky=tk.W, padx=8, pady=8)
        ToolTip(dry_cb, "Preview mode - shows what would be done without creating files")

        # File List / Progress Table
        tree_frame = tk.LabelFrame(main_frame, text=" 📊 Progress ",
                                   font=("Segoe UI", "10", "bold"),
                                   bg=DARK_FRAME, fg=DARK_FG,
                                   padx=15, pady=12,
                                   relief=tk.FLAT, bd=1, highlightthickness=1,
                                   highlightbackground=DARK_BORDER, highlightcolor=DARK_SELECT)
        tree_frame.pack(fill=tk.BOTH, expand=True, pady=8)

        # Create treeview with scrollbar
        tree_container = tk.Frame(tree_frame, bg=DARK_FRAME)
        tree_container.pack(fill=tk.BOTH, expand=True)

        self.tree = ttk.Treeview(tree_container, columns=("filename", "size", "progress", "pct", "time", "status"), 
                                show="headings", height=12)
        self.tree.heading("filename", text="Filename")
        self.tree.heading("size", text="Size")
        self.tree.heading("progress", text="Progress")
        self.tree.heading("pct", text="%")
        self.tree.heading("time", text="Elapsed/Rem")
        self.tree.heading("status", text="Status")

        self.tree.column("filename", width=280)
        self.tree.column("size", width=90, anchor=tk.E)
        self.tree.column("progress", width=180)
        self.tree.column("pct", width=50, anchor=tk.E)
        self.tree.column("time", width=120, anchor=tk.E)
        self.tree.column("status", width=150)

        # Configure treeview tags for dark theme
        self.tree.tag_configure("success", background=DARK_FRAME, foreground=DARK_SUCCESS)
        self.tree.tag_configure("error", background=DARK_FRAME, foreground=DARK_ERROR)
        self.tree.tag_configure("processing", background=DARK_FRAME, foreground=DARK_INFO)

        vsb = ttk.Scrollbar(tree_container, orient="vertical", command=self.tree.yview)
        self.tree.configure(yscrollcommand=vsb.set)
        vsb.pack(side=tk.RIGHT, fill=tk.Y)
        self.tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)

        # Progress bars storage
        self.progress_bars: Dict[int, ttk.Progressbar] = {}

        # Control Frame
        control_frame = tk.Frame(main_frame, bg=DARK_BG, pady=10)
        control_frame.pack(fill=tk.X)
        
        self.start_button = self.create_button(control_frame, "▶ Start Remux", self.start_remux, 'primary')
        self.start_button.pack(side=tk.RIGHT, padx=6)
        ToolTip(self.start_button, "Start remuxing process (Ctrl+S)")

        self.cancel_button = self.create_button(control_frame, "⏹ Cancel", self.cancel_remux, 'secondary')
        self.cancel_button.pack(side=tk.RIGHT, padx=6)
        self.cancel_button.config(state=tk.DISABLED, bg="#3a3a3a", fg="#666666")
        ToolTip(self.cancel_button, "Cancel current operation (Esc)")

        self.export_button = self.create_button(control_frame, "💾 Export Log", self.export_log, 'secondary')
        self.export_button.pack(side=tk.RIGHT, padx=6)
        self.export_button.config(state=tk.DISABLED, bg="#3a3a3a", fg="#666666")
        ToolTip(self.export_button, "Export operation log to file")

        self.status_label = tk.Label(control_frame, 
                                    text="✨ Ready. Select an input directory to begin.",
                                    font=("Segoe UI", "9"),
                                    bg=DARK_BG, fg=DARK_FG,
                                    anchor=tk.W)
        self.status_label.pack(side=tk.LEFT, fill=tk.X, expand=True, padx=10)

        if not self.mkvmerge_path:
            self.show_error("mkvmerge not found. Please install MKVToolNix and ensure mkvmerge is in your system's PATH.")
            self.start_button.config(state=tk.DISABLED, bg="#3a3a3a", fg="#666666")

    def browse_input(self):
        """Browse for input directory."""
        path = filedialog.askdirectory(title="Select Input Directory")
        if path:
            self.input_dir.set(path)
            if not self.output_dir.get():
                self.output_dir.set(str(Path(path) / "remuxed"))
            # Automatically scan for languages
            self.scan_languages()

    def browse_output(self):
        """Browse for output directory."""
        path = filedialog.askdirectory(title="Select Output Directory")
        if path:
            self.output_dir.set(path)

    def show_error(self, message):
        """Display error message in a dialog."""
        messagebox.showerror("Error", message)
        logger.error(message)

    def show_info(self, message):
        """Display info message in a dialog."""
        messagebox.showinfo("Information", message)

    def scan_languages(self):
        """Automatically scan MKV files and display available languages."""
        input_path = Path(self.input_dir.get())
        
        if not input_path.is_dir():
            return
        
        mkv_files = sorted(input_path.glob("*.mkv"))
        if not mkv_files:
            self.avail_audio_label.config(text="No MKV files found", fg=DARK_ERROR)
            self.avail_sub_label.config(text="No MKV files found", fg=DARK_ERROR)
            self.status_label.config(text="❌ No .mkv files found in the input directory.")
            return
        
        self.avail_audio_label.config(text=f"Scanning {len(mkv_files)} files...", fg=DARK_INFO)
        self.avail_sub_label.config(text=f"Scanning {len(mkv_files)} files...", fg=DARK_INFO)
        self.status_label.config(text=f"🔍 Scanning {len(mkv_files)} files for available languages...")
        self.root.update()
        
        # Run scan in separate thread to avoid freezing GUI
        def scan_thread():
            try:
                audio_langs, sub_langs = get_available_languages(self.mkvmerge_path, mkv_files)
                self.root.after(0, lambda: self.update_available_languages(audio_langs, sub_langs))
            except Exception as e:
                logger.error(f"Error scanning languages: {e}")
                self.root.after(0, lambda: self.show_error(f"Error scanning files: {e}"))
        
        threading.Thread(target=scan_thread, daemon=True).start()

    def update_available_languages(self, audio_langs, sub_langs):
        """Update the display with scanned languages."""
        self.available_audio_langs = audio_langs
        self.available_sub_langs = sub_langs
        
        if audio_langs:
            audio_text = ", ".join(audio_langs)
            self.avail_audio_label.config(text=audio_text, fg=DARK_SUCCESS)
        else:
            self.avail_audio_label.config(text="None found", fg=DARK_WARNING)
        
        if sub_langs:
            sub_text = ", ".join(sub_langs)
            self.avail_sub_label.config(text=sub_text, fg=DARK_SUCCESS)
        else:
            self.avail_sub_label.config(text="None found", fg=DARK_WARNING)
        
        self.status_label.config(text=f"✅ Scan complete. Found {len(audio_langs)} audio and {len(sub_langs)} subtitle languages.")

    def select_all_audio(self):
        """Select all available audio languages."""
        if self.available_audio_langs:
            self.audio_langs.set(",".join(self.available_audio_langs))
        else:
            self.show_info("No audio languages available. Please scan files first.")

    def select_all_subs(self):
        """Select all available subtitle languages."""
        if self.available_sub_langs:
            self.sub_langs.set(",".join(self.available_sub_langs))
        else:
            self.show_info("No subtitle languages available. Please scan files first.")

    def start_remux(self):
        """Start the remuxing process."""
        if self.running:
            return

        input_path = Path(self.input_dir.get())
        output_path = Path(self.output_dir.get())

        # Validation
        if not input_path.is_dir():
            self.show_error("Invalid input directory.")
            return
        
        if not output_path.parent.exists():
            self.show_error("Output directory parent does not exist.")
            return

        # Check if output directory is writable
        if not is_writable(output_path):
            self.show_error(f"Output directory is not writable: {output_path}")
            return
        
        audio_langs = [lang.strip() for lang in self.audio_langs.get().lower().split(',') if lang.strip()]
        if not audio_langs:
            self.show_error("At least one audio language is required.")
            return
            
        sub_langs = [lang.strip() for lang in self.sub_langs.get().lower().split(',') if lang.strip()]

        mkv_files = sorted(input_path.glob("*.mkv"))
        if not mkv_files:
            self.status_label.config(text="No .mkv files found in the input directory.")
            return

        try:
            output_path.mkdir(parents=True, exist_ok=True)
        except Exception as e:
            self.show_error(f"Cannot create output directory: {e}")
            return
        
        # Clear previous data
        self.tree.delete(*self.tree.get_children())
        self.file_map = {}
        self.progress_bars = {}
        self.error_log = []
        self.cancel_event.clear()
        self.batch_start_time = time.time()
        
        # Add files to tree
        for i, f in enumerate(mkv_files):
            try:
                file_size = f.stat().st_size if f.exists() else 0
                size_str = fmt_size(file_size)
            except Exception:
                size_str = "Unknown"
            
            item_id = self.tree.insert("", "end", values=(f.name, size_str, "", "0%", "--:--/--:--", "Pending"))
            self.file_map[i] = {"id": item_id, "path": f, "size": file_size}
            
            # Create progress bar (we'll use text representation for now, but store for future use)
            # Note: ttk.Progressbar doesn't work well in Treeview, so we'll use text bars

        self.running = True
        self.start_button.config(state=tk.DISABLED, bg="#3a3a3a", fg="#666666")
        self.cancel_button.config(state=tk.NORMAL, bg=DARK_ENTRY, fg=DARK_FG)
        self.export_button.config(state=tk.DISABLED, bg="#3a3a3a", fg="#666666")
        self.status_label.config(text=f"⚙️ Processing {len(mkv_files)} files...")

        self.worker_thread = threading.Thread(
            target=self.run_batch_thread,
            args=(mkv_files, output_path, audio_langs, sub_langs, self.skip_exists.get(), self.dry_run.get()),
            daemon=True
        )
        self.worker_thread.start()
        self.root.after(PROGRESS_UPDATE_INTERVAL, self.process_queue)

    def run_batch_thread(self, mkv_files, output_path, audio_langs, sub_langs, skip_exists, dry_run):
        """Run batch remuxing in a separate thread."""
        try:
            for i, src_path in enumerate(mkv_files):
                if self.cancel_event.is_set():
                    break
                out_path = output_path / src_path.name
                remux_file(self.mkvmerge_path, src_path, out_path, audio_langs, sub_langs,
                          skip_exists, dry_run, self.update_queue, i, self.cancel_event)
        except Exception as e:
            logger.error(f"Error in batch thread: {e}")
            self.update_queue.put({"type": "error", "error": str(e)})
        finally:
            self.update_queue.put({"type": "finished"})

    def cancel_remux(self):
        """Cancel the current remuxing operation."""
        if not self.running:
            return
        
        if messagebox.askyesno("Cancel", "Are you sure you want to cancel the current operation?"):
            self.cancel_event.set()
            self.running = False
            self.start_button.config(state=tk.NORMAL, bg=DARK_SELECT, fg="#ffffff")
            self.cancel_button.config(state=tk.DISABLED, bg="#3a3a3a", fg="#666666")
            self.status_label.config(text="⏸ Cancelling... Please wait for current file to finish.")

    def process_queue(self):
        """Process messages from the remux queue and update GUI."""
        try:
            while not self.update_queue.empty():
                msg = self.update_queue.get_nowait()

                if msg.get("type") == "finished":
                    self.running = False
                    self.cancel_event.clear()
                    self.start_button.config(state=tk.NORMAL, bg=DARK_SELECT, fg="#ffffff")
                    self.cancel_button.config(state=tk.DISABLED, bg="#3a3a3a", fg="#666666")
                    self.export_button.config(state=tk.NORMAL, bg=DARK_ENTRY, fg=DARK_FG)
                    
                    def _status_ok(values):
                        status = str(values[5]) if len(values) > 5 else ""
                        return status in ("OK", "Skipped (exists)", "Dry-run (skipped)")

                    ok_count = sum(1 for i in self.file_map.values() if _status_ok(self.tree.item(i['id'])['values']))
                    fail_count = len(self.file_map) - ok_count
                    total_time = time.time() - self.batch_start_time if self.batch_start_time else 0
                    
                    self.status_label.config(text=f"✅ Completed in {fmt_time(total_time)}. OK: {ok_count}, Failed: {fail_count}.")
                    self.show_info(f"Remuxing complete!\n\nOK: {ok_count}\nFailed: {fail_count}\nTotal time: {fmt_time(total_time)}")
                    return

                if msg.get("type") == "error":
                    self.show_error(f"Error during processing: {msg.get('error', 'Unknown error')}")
                    continue

                row_idx = msg.get("row_idx")
                if row_idx is None or row_idx not in self.file_map:
                    continue

                item_id = self.file_map[row_idx]["id"]
                
                # Safely parse percentage value
                pct = msg.get("pct", 0)
                if isinstance(pct, (int, float)):
                    pct_val = int(pct)
                else:
                    try:
                        pct_str = str(pct).replace('%', '').strip()
                        pct_val = int(float(pct_str)) if pct_str else 0
                    except (ValueError, AttributeError):
                        pct_val = 0
                pct_val = max(0, min(100, pct_val))  # Clamp to 0-100
                
                elapsed = msg.get("elapsed", 0)
                remaining = msg.get("remaining")
                
                time_str = f"{fmt_time(elapsed)}/{fmt_time(remaining)}"
                
                status = msg.get("status", "Pending")
                
                # Create progress bar representation with colors
                blocks = pct_val // 5
                if status == "OK":
                    bar = "█" * blocks + "░" * (20 - blocks)
                    tag = "success"
                elif "FAILED" in status or "Failed" in status or "Cancelled" in status:
                    bar = "█" * blocks + "░" * (20 - blocks)
                    tag = "error"
                elif status == "Processing":
                    bar = "█" * blocks + "░" * (20 - blocks)
                    tag = "processing"
                else:
                    bar = "█" * blocks + "░" * (20 - blocks)
                    tag = ""
                
                # Store error if present
                if "error" in msg:
                    self.error_log.append({
                        "file": self.file_map[row_idx]["path"].name,
                        "error": msg["error"],
                        "status": status
                    })

                # Get current values to preserve size
                current_values = list(self.tree.item(item_id)['values'])
                size_str = current_values[1] if len(current_values) > 1 else ""

                # Apply tag based on status
                tags = [tag] if tag else []
                self.tree.item(item_id, values=(
                    self.file_map[row_idx]["path"].name,
                    size_str,
                    bar,
                    f"{pct_val}%",
                    time_str,
                    status
                ), tags=tags)

        except Exception as e:
            logger.error(f"Error processing queue: {e}")
        finally:
            if self.running:
                self.root.after(PROGRESS_UPDATE_INTERVAL, self.process_queue)

    def export_log(self):
        """Export operation log to a file."""
        if not self.file_map:
            self.show_info("No operations to export.")
            return
        
        filename = filedialog.asksaveasfilename(
            defaultextension=".txt",
            filetypes=[("Text files", "*.txt"), ("All files", "*.*")],
            title="Export Log"
        )
        
        if not filename:
            return
        
        try:
            with open(filename, 'w', encoding='utf-8') as f:
                f.write("MKV Batch Remuxer - Operation Log\n")
                f.write("=" * 50 + "\n\n")
                f.write(f"Date: {time.strftime('%Y-%m-%d %H:%M:%S')}\n")
                f.write(f"Input Directory: {self.input_dir.get()}\n")
                f.write(f"Output Directory: {self.output_dir.get()}\n")
                f.write(f"Audio Languages: {self.audio_langs.get()}\n")
                f.write(f"Subtitle Languages: {self.sub_langs.get()}\n\n")
                f.write("-" * 50 + "\n\n")
                
                f.write("File Results:\n")
                f.write("-" * 50 + "\n")
                for idx, file_info in self.file_map.items():
                    item_id = file_info['id']
                    values = self.tree.item(item_id)['values']
                    f.write(f"File: {values[0]}\n")
                    f.write(f"  Size: {values[1]}\n")
                    f.write(f"  Progress: {values[3]}\n")
                    f.write(f"  Time: {values[4]}\n")
                    f.write(f"  Status: {values[5]}\n\n")
                
                if self.error_log:
                    f.write("\nErrors:\n")
                    f.write("-" * 50 + "\n")
                    for error_entry in self.error_log:
                        f.write(f"File: {error_entry['file']}\n")
                        f.write(f"  Status: {error_entry['status']}\n")
                        f.write(f"  Error: {error_entry['error']}\n\n")
                
                total_time = time.time() - self.batch_start_time if self.batch_start_time else 0
                def _status_ok(values):
                    status = str(values[5]) if len(values) > 5 else ""
                    return status in ("OK", "Skipped (exists)", "Dry-run (skipped)")

                ok_count = sum(1 for i in self.file_map.values() if _status_ok(self.tree.item(i['id'])['values']))
                fail_count = len(self.file_map) - ok_count
                
                f.write("\nSummary:\n")
                f.write("-" * 50 + "\n")
                f.write(f"Total Files: {len(self.file_map)}\n")
                f.write(f"Successful: {ok_count}\n")
                f.write(f"Failed: {fail_count}\n")
                f.write(f"Total Time: {fmt_time(total_time)}\n")
            
            self.show_info(f"Log exported successfully to:\n{filename}")
            logger.info(f"Log exported to {filename}")
        except Exception as e:
            self.show_error(f"Failed to export log: {e}")
            logger.error(f"Error exporting log: {e}")


if __name__ == "__main__":
    root = tk.Tk()
    app = RemuxApp(root)
    root.mainloop()
