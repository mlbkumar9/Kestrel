#!/usr/bin/env python3
"""
REMUX_ModernGUI.py
A modern, dark-themed GUI for batch remuxing MKV files using CustomTkinter.
Features:
- Modern UI with rounded corners and dark mode
- Automatic language detection
- Batch processing with progress tracking
- Drag and drop support (via path entry)
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
from typing import Optional, Dict, List, Tuple

import tkinter as tk
from tkinter import filedialog, messagebox
import customtkinter as ctk
from PIL import Image

# --- Constants ---
MKVMERGE_TIMEOUT = 10.0
PROGRESS_UPDATE_INTERVAL = 100  # milliseconds
DEFAULT_WINDOW_SIZE = "1000x750"
MIN_WINDOW_SIZE = (900, 650)

# --- Configuration ---
ctk.set_appearance_mode("Dark")  # Modes: "System" (standard), "Dark", "Light"
ctk.set_default_color_theme("dark-blue")  # Themes: "blue" (standard), "green", "dark-blue"

# --- Logging ---
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s',
    handlers=[
        logging.FileHandler('remux_gui.log', encoding='utf-8'),
        logging.StreamHandler()
    ]
)
logger = logging.getLogger(__name__)

# --- Core Logic (Reused & Adapted) ---

def find_mkvmerge() -> Optional[str]:
    """Find mkvmerge executable."""
    mk = shutil.which("mkvmerge") or shutil.which("mkvmerge.exe")
    if mk:
        return mk
    candidate = r"C:\Program Files\MKVToolNix\mkvmerge.exe"
    return candidate if Path(candidate).exists() else None

def identify_tracks(mkvmerge_path: str, src: Path, timeout: float = MKVMERGE_TIMEOUT):
    """Identify tracks in an MKV file."""
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
    """Scan files for languages."""
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

def fmt_size(size_bytes: int) -> str:
    for unit in ['B', 'KB', 'MB', 'GB', 'TB']:
        if size_bytes < 1024.0:
            return f"{size_bytes:.1f} {unit}"
        size_bytes /= 1024.0
    return f"{size_bytes:.1f} PB"

def fmt_time(sec: Optional[float]) -> str:
    if sec is None: return "--:--"
    try:
        s = int(round(sec))
        m, s = divmod(s, 60)
        return f"{m:02d}:{s:02d}"
    except: return "--:--"

# --- UI Classes ---

class ModernRemuxApp(ctk.CTk):
    def __init__(self):
        super().__init__()

        self.title("MKV Batch Remuxer Pro")
        self.geometry(DEFAULT_WINDOW_SIZE)
        self.minsize(*MIN_WINDOW_SIZE)
        
        # Set grid layout 1x2
        self.grid_rowconfigure(0, weight=1)
        self.grid_columnconfigure(1, weight=1)

        self.mkvmerge_path = find_mkvmerge()
        self.update_queue = queue.Queue()
        self.running = False
        self.cancel_event = threading.Event()
        self.file_map = {}
        self.available_audio_langs = []
        self.available_sub_langs = []

        # --- Sidebar ---
        self.sidebar_frame = ctk.CTkFrame(self, width=200, corner_radius=0)
        self.sidebar_frame.grid(row=0, column=0, rowspan=4, sticky="nsew")
        self.sidebar_frame.grid_rowconfigure(4, weight=1)

        self.logo_label = ctk.CTkLabel(self.sidebar_frame, text="REMUX\nPRO", font=ctk.CTkFont(size=24, weight="bold"))
        self.logo_label.grid(row=0, column=0, padx=20, pady=(20, 10))

        self.sidebar_btn_1 = ctk.CTkButton(self.sidebar_frame, text="Dashboard", command=self.sidebar_button_event)
        self.sidebar_btn_1.grid(row=1, column=0, padx=20, pady=10)
        
        # Appearance Mode
        self.appearance_mode_label = ctk.CTkLabel(self.sidebar_frame, text="Appearance Mode:", anchor="w")
        self.appearance_mode_label.grid(row=5, column=0, padx=20, pady=(10, 0))
        self.appearance_mode_optionemenu = ctk.CTkOptionMenu(self.sidebar_frame, values=["Dark", "Light", "System"],
                                                                       command=self.change_appearance_mode_event)
        self.appearance_mode_optionemenu.grid(row=6, column=0, padx=20, pady=(10, 20))

        # --- Main Content ---
        self.main_frame = ctk.CTkFrame(self, corner_radius=10)
        self.main_frame.grid(row=0, column=1, sticky="nsew", padx=20, pady=20)
        self.main_frame.grid_columnconfigure(1, weight=1)
        self.main_frame.grid_rowconfigure(2, weight=1) # Progress area expands

        # 1. Directories Section
        self.create_dir_section()

        # 2. Language & Options Section
        self.create_options_section()

        # 3. Progress Section
        self.create_progress_section()

        # 4. Status Bar & Actions
        self.create_action_section()

        if not self.mkvmerge_path:
            self.status_label.configure(text="Error: mkvmerge not found! Install MKVToolNix.", text_color="red")
            self.start_button.configure(state="disabled")

    def create_dir_section(self):
        self.dir_frame = ctk.CTkFrame(self.main_frame)
        self.dir_frame.grid(row=0, column=0, columnspan=2, sticky="ew", padx=10, pady=(10, 5))
        self.dir_frame.grid_columnconfigure(1, weight=1)

        ctk.CTkLabel(self.dir_frame, text="Input Directory:").grid(row=0, column=0, padx=10, pady=10, sticky="w")
        self.input_entry = ctk.CTkEntry(self.dir_frame, placeholder_text="Select input folder...")
        self.input_entry.grid(row=0, column=1, padx=10, pady=10, sticky="ew")
        ctk.CTkButton(self.dir_frame, text="Browse", width=80, command=self.browse_input).grid(row=0, column=2, padx=10, pady=10)

        ctk.CTkLabel(self.dir_frame, text="Output Directory:").grid(row=1, column=0, padx=10, pady=10, sticky="w")
        self.output_entry = ctk.CTkEntry(self.dir_frame, placeholder_text="Select output folder (optional)...")
        self.output_entry.grid(row=1, column=1, padx=10, pady=10, sticky="ew")
        ctk.CTkButton(self.dir_frame, text="Browse", width=80, command=self.browse_output).grid(row=1, column=2, padx=10, pady=10)

    def create_options_section(self):
        self.opt_frame = ctk.CTkFrame(self.main_frame)
        self.opt_frame.grid(row=1, column=0, columnspan=2, sticky="ew", padx=10, pady=5)
        self.opt_frame.grid_columnconfigure((1, 3), weight=1)

        # Audio
        ctk.CTkLabel(self.opt_frame, text="Audio Langs:").grid(row=0, column=0, padx=10, pady=10, sticky="w")
        self.audio_entry = ctk.CTkEntry(self.opt_frame, placeholder_text="e.g. eng, jpn")
        self.audio_entry.grid(row=0, column=1, padx=10, pady=10, sticky="ew")
        self.audio_entry.insert(0, "eng")

        # Subtitles
        ctk.CTkLabel(self.opt_frame, text="Subtitle Langs:").grid(row=0, column=2, padx=10, pady=10, sticky="w")
        self.sub_entry = ctk.CTkEntry(self.opt_frame, placeholder_text="e.g. eng, spa")
        self.sub_entry.grid(row=0, column=3, padx=10, pady=10, sticky="ew")
        self.sub_entry.insert(0, "eng")

        # Checkboxes
        self.skip_exists_var = ctk.BooleanVar(value=True)
        self.dry_run_var = ctk.BooleanVar(value=False)
        
        ctk.CTkCheckBox(self.opt_frame, text="Skip Existing", variable=self.skip_exists_var).grid(row=1, column=0, columnspan=2, padx=10, pady=10, sticky="w")
        ctk.CTkCheckBox(self.opt_frame, text="Dry Run", variable=self.dry_run_var).grid(row=1, column=2, columnspan=2, padx=10, pady=10, sticky="w")

        # Available Langs Info
        self.avail_info = ctk.CTkLabel(self.opt_frame, text="Scan input to see available languages", text_color="gray")
        self.avail_info.grid(row=2, column=0, columnspan=4, padx=10, pady=5)

    def create_progress_section(self):
        self.prog_frame = ctk.CTkScrollableFrame(self.main_frame, label_text="Progress")
        self.prog_frame.grid(row=2, column=0, columnspan=2, sticky="nsew", padx=10, pady=5)
        self.prog_frame.grid_columnconfigure(1, weight=1) # Filename expands
        
        # Headers
        headers = ["Status", "Filename", "Progress", "Size", "Time"]
        weights = [0, 1, 1, 0, 0]
        for i, h in enumerate(headers):
            ctk.CTkLabel(self.prog_frame, text=h, font=ctk.CTkFont(weight="bold")).grid(row=0, column=i, padx=5, pady=5, sticky="w")

        self.file_rows = [] # List to keep track of row widgets

    def create_action_section(self):
        self.action_frame = ctk.CTkFrame(self.main_frame, fg_color="transparent")
        self.action_frame.grid(row=3, column=0, columnspan=2, sticky="ew", padx=10, pady=10)
        
        self.status_label = ctk.CTkLabel(self.action_frame, text="Ready", anchor="w")
        self.status_label.pack(side="left", padx=10)

        self.cancel_button = ctk.CTkButton(self.action_frame, text="Cancel", fg_color="transparent", border_width=2, text_color=("gray10", "#DCE4EE"), state="disabled", command=self.cancel_remux)
        self.cancel_button.pack(side="right", padx=10)

        self.start_button = ctk.CTkButton(self.action_frame, text="Start Remux", command=self.start_remux)
        self.start_button.pack(side="right", padx=10)

    # --- Event Handlers ---

    def sidebar_button_event(self):
        pass

    def change_appearance_mode_event(self, new_appearance_mode: str):
        ctk.set_appearance_mode(new_appearance_mode)

    def browse_input(self):
        path = filedialog.askdirectory()
        if path:
            self.input_entry.delete(0, "end")
            self.input_entry.insert(0, path)
            if not self.output_entry.get():
                self.output_entry.insert(0, str(Path(path) / "remuxed"))
            self.scan_languages()

    def browse_output(self):
        path = filedialog.askdirectory()
        if path:
            self.output_entry.delete(0, "end")
            self.output_entry.insert(0, path)

    def scan_languages(self):
        path = self.input_entry.get()
        if not path or not os.path.isdir(path): return
        
        self.status_label.configure(text="Scanning languages...")
        
        def _scan():
            files = sorted(Path(path).glob("*.mkv"))
            if not files:
                self.after(0, lambda: self.status_label.configure(text="No MKV files found."))
                return
            
            a, s = get_available_languages(self.mkvmerge_path, files)
            
            def _update():
                self.available_audio_langs = a
                self.available_sub_langs = s
                txt = f"Found: {len(a)} Audio ({','.join(a[:3])}...), {len(s)} Subs ({','.join(s[:3])}...)"
                self.avail_info.configure(text=txt)
                self.status_label.configure(text="Ready")
            
            self.after(0, _update)

        threading.Thread(target=_scan, daemon=True).start()

    def add_file_row(self, index, filename, size):
        # Row widgets
        row_idx = index + 1
        
        status_lbl = ctk.CTkLabel(self.prog_frame, text="Pending", text_color="gray")
        status_lbl.grid(row=row_idx, column=0, padx=5, pady=2, sticky="w")
        
        name_lbl = ctk.CTkLabel(self.prog_frame, text=filename, anchor="w")
        name_lbl.grid(row=row_idx, column=1, padx=5, pady=2, sticky="ew")
        
        prog_bar = ctk.CTkProgressBar(self.prog_frame)
        prog_bar.grid(row=row_idx, column=2, padx=5, pady=2, sticky="ew")
        prog_bar.set(0)
        
        size_lbl = ctk.CTkLabel(self.prog_frame, text=size)
        size_lbl.grid(row=row_idx, column=3, padx=5, pady=2, sticky="e")
        
        time_lbl = ctk.CTkLabel(self.prog_frame, text="--:--")
        time_lbl.grid(row=row_idx, column=4, padx=5, pady=2, sticky="e")

        self.file_rows.append({
            "status": status_lbl,
            "progress": prog_bar,
            "time": time_lbl
        })

    def start_remux(self):
        if self.running: return
        
        inp = self.input_entry.get()
        outp = self.output_entry.get()
        
        if not inp or not os.path.isdir(inp):
            messagebox.showerror("Error", "Invalid input directory")
            return
            
        files = sorted(Path(inp).glob("*.mkv"))
        if not files:
            messagebox.showinfo("Info", "No MKV files found")
            return

        audio_langs = [x.strip().lower() for x in self.audio_entry.get().split(',') if x.strip()]
        sub_langs = [x.strip().lower() for x in self.sub_entry.get().split(',') if x.strip()]
        if not audio_langs:
            messagebox.showerror("Error", "At least one audio language is required")
            return
        if not outp:
            outp = str(Path(inp) / "remuxed")
            self.output_entry.delete(0, "end")
            self.output_entry.insert(0, outp)

        # Clear old rows
        for widget in self.prog_frame.winfo_children():
            if int(widget.grid_info()["row"]) > 0: # Skip headers
                widget.destroy()
        self.file_rows = []

        # Setup UI
        self.running = True
        self.start_button.configure(state="disabled")
        self.cancel_button.configure(state="normal")
        self.cancel_event.clear()
        
        # Create rows
        for i, f in enumerate(files):
            self.add_file_row(i, f.name, fmt_size(f.stat().st_size))
        
        threading.Thread(target=self.run_batch, args=(files, Path(outp), audio_langs, sub_langs), daemon=True).start()
        self.after(100, self.process_queue)

    def cancel_remux(self):
        if self.running:
            self.cancel_event.set()
            self.status_label.configure(text="Cancelling...")

    def process_queue(self):
        try:
            while True:
                msg = self.update_queue.get_nowait()
                idx = msg.get('row_idx')
                if idx is not None and idx < len(self.file_rows):
                    row = self.file_rows[idx]
                    
                    if 'status' in msg:
                        row['status'].configure(text=msg['status'])
                        if "OK" in msg['status']: row['status'].configure(text_color="green")
                        elif "Fail" in msg['status']: row['status'].configure(text_color="red")
                        
                    if 'pct' in msg:
                        row['progress'].set(msg['pct'] / 100.0)
                        
                    if 'remaining' in msg and msg['remaining'] is not None:
                        row['time'].configure(text=fmt_time(msg['remaining']))
                
                if msg.get('batch_finished'):
                    self.running = False
                    self.start_button.configure(state="normal")
                    self.cancel_button.configure(state="disabled")
                    self.status_label.configure(text="Batch Processing Complete")
                    
        except queue.Empty:
            pass
        
        if self.running:
            self.after(100, self.process_queue)

    def run_batch(self, files, out_dir, audio_langs, sub_langs):
        out_dir.mkdir(parents=True, exist_ok=True)
        
        for i, src in enumerate(files):
            if self.cancel_event.is_set(): break
            
            out_file = out_dir / src.name
            self.remux_single_file(i, src, out_file, audio_langs, sub_langs)
            
        self.update_queue.put({'batch_finished': True})

    def remux_single_file(self, idx, src, dst, audio_langs, sub_langs):
        def send(**kwargs):
            kwargs['row_idx'] = idx
            self.update_queue.put(kwargs)

        if not self.mkvmerge_path:
            send(status="No mkvmerge", pct=0)
            return

        if self.skip_exists_var.get() and dst.exists() and dst.stat().st_size > 0:
            send(status="Skipped", pct=100, remaining=0)
            return

        if self.dry_run_var.get():
            send(status="Dry Run", pct=100, remaining=0)
            time.sleep(0.5)
            return

        # Identify
        ident = identify_tracks(self.mkvmerge_path, src)
        if not ident["ok"]:
            send(status="ID Failed", pct=0)
            return

        # Select tracks
        audio_ids = []
        sub_ids = []
        for t in ident["data"].get("tracks", []):
            lang = (t.get("properties", {}).get("language") or "und").lower()
            tid = t.get("id")
            ttype = (t.get("type") or "").lower()
            if tid is None:
                continue
            if ttype == "audio" and lang in audio_langs:
                audio_ids.append(str(tid))
            elif ttype in ("subtitles", "subtitle") and lang in sub_langs:
                sub_ids.append(str(tid))

        if not audio_ids:
            send(status="No Audio", pct=0)
            return

        # Command
        cmd = [self.mkvmerge_path, "-o", str(dst), "--audio-tracks", ",".join(audio_ids)]
        if sub_ids:
            cmd += ["--subtitle-tracks", ",".join(sub_ids)]
        else:
            cmd += ["--no-subtitles"]
        cmd += ["--ui-language", "en", str(src)]

        # Run
        try:
            proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, 
                                  text=True, encoding="utf-8", errors="replace")
            
            start_time = time.time()
            re_prog = re.compile(r"(?:#GUI#progress|Progress:)\s*([0-9]{1,3})")
            
            for line in iter(proc.stdout.readline, ""):
                if self.cancel_event.is_set():
                    proc.terminate()
                    try:
                        proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                    send(status="Cancelled", pct=0)
                    return

                m = re_prog.search(line)
                if m:
                    pct = max(0, min(100, int(m.group(1))))
                    elapsed = time.time() - start_time
                    rem = (elapsed * (100 - pct) / pct) if pct > 0 else 0
                    send(status="Processing", pct=pct, remaining=rem)
            
            proc.wait()
            if proc.returncode == 0 and dst.exists() and dst.stat().st_size > 0:
                send(status="OK", pct=100, remaining=0)
            else:
                send(status="Failed", pct=0)

        except Exception as e:
            logger.error(f"Remux error: {e}")
            send(status="Error", pct=0)

if __name__ == "__main__":
    app = ModernRemuxApp()
    app.mainloop()
