import os
import json
import shutil
import subprocess
import threading
import time
import logging
import queue
import re
from pathlib import Path
from flask import Flask, jsonify, request, Response

# --- Configuration ---
app = Flask(__name__)
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s',
    handlers=[
        logging.FileHandler('backend_debug.log', mode='w', encoding='utf-8'),
        logging.StreamHandler()
    ]
)
logger = logging.getLogger(__name__)

# --- Global State ---
state_lock = threading.Lock()
state = {
    "progress": {},  # Map of file_index -> {status, pct, elapsed, remaining, etc.}
    "batch_status": "idle",  # idle, running, paused
    "cancel_flag": False,
    "logs": []
}
update_queue = queue.Queue()

# --- Helpers ---
def find_mkvmerge():
    mk = shutil.which("mkvmerge") or shutil.which("mkvmerge.exe")
    if mk: return mk
    candidate = r"C:\Program Files\MKVToolNix\mkvmerge.exe"
    return candidate if Path(candidate).exists() else None

MKVMERGE_PATH = find_mkvmerge()
logger.info(f"MKVMERGE_PATH: {MKVMERGE_PATH}")

def identify_tracks(src):
    if not MKVMERGE_PATH: return {"ok": False, "err": "mkvmerge not found"}
    cmd = [MKVMERGE_PATH, "--identify", "--identification-format", "json", str(src)]
    try:
        res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False, timeout=30)
        if res.returncode != 0: return {"ok": False, "err": res.stderr}
        return {"ok": True, "data": json.loads(res.stdout)}
    except Exception as e:
        return {"ok": False, "err": str(e)}

def get_languages(files):
    audio = set()
    subs = set()
    for f in files:
        res = identify_tracks(f)
        if res["ok"]:
            for t in res["data"].get("tracks", []):
                lang = (t.get("properties", {}).get("language") or "und").lower()
                ttype = (t.get("type") or "").lower()
                if ttype == "audio": audio.add(lang)
                elif ttype in ("subtitles", "subtitle"): subs.add(lang)
    return sorted(list(audio)), sorted(list(subs))

def normalize_langs(langs):
    if not langs:
        return []
    if isinstance(langs, str):
        langs = langs.split(",")
    return [str(x).strip().lower() for x in langs if str(x).strip()]

# --- Routes ---

@app.route('/')
def index():
    return jsonify({
        "name": "MKV Batch Remuxer API",
        "status": "ok",
        "mkvmerge": bool(MKVMERGE_PATH),
        "endpoints": ["/api/scan", "/api/remux", "/api/cancel", "/api/stream", "/api/status"],
    })

@app.route('/api/status')
def api_status():
    with state_lock:
        return jsonify({"ok": True, "mkvmerge": MKVMERGE_PATH, **{k: state[k] for k in ("batch_status", "progress")}})

@app.route('/api/scan', methods=['POST'])
def scan_dir():
    data = request.json or {}
    path = data.get('path')
    if not path or not os.path.isdir(path):
        return jsonify({"ok": False, "error": "Invalid directory"})
    
    files = sorted([str(p) for p in Path(path).glob("*.mkv")])
    if not files:
        return jsonify({"ok": True, "files": [], "audio": [], "subs": []})
    
    # Scan a sample of files for available languages (full scan can be slow on large folders)
    scan_files = files[:10] 
    audio, subs = get_languages(scan_files)
    
    file_data = []
    for f in files:
        p = Path(f)
        file_data.append({
            "name": p.name,
            "path": str(p),
            "size": p.stat().st_size
        })
        
    return jsonify({"ok": True, "files": file_data, "audio": audio, "subs": subs})

@app.route('/api/remux', methods=['POST'])
def start_remux():
    data = request.json or {}
    files = data.get('files', [])
    out_dir = data.get('outputDir')
    audio_langs = normalize_langs(data.get('audioLangs', []))
    sub_langs = normalize_langs(data.get('subLangs', []))
    skip_exists = data.get('skipExists', True)
    dry_run = bool(data.get('dryRun', data.get('dry_run', False)))
    
    if not files or not out_dir:
        return jsonify({"ok": False, "error": "Missing arguments"})
    if not MKVMERGE_PATH:
        return jsonify({"ok": False, "error": "mkvmerge not found"})
    if not audio_langs:
        return jsonify({"ok": False, "error": "At least one audio language is required"})
        
    with state_lock:
        if state["batch_status"] == "running":
            return jsonify({"ok": False, "error": "Already running"})
        state["batch_status"] = "running"
        state["cancel_flag"] = False
        state["progress"] = {}
    
    threading.Thread(
        target=run_batch,
        args=(files, out_dir, audio_langs, sub_langs, skip_exists, dry_run),
        daemon=True,
    ).start()
    return jsonify({"ok": True, "dryRun": dry_run})

@app.route('/api/cancel', methods=['POST'])
def cancel_remux():
    with state_lock:
        state["cancel_flag"] = True
    return jsonify({"ok": True})

@app.route('/api/stream')
def stream():
    def event_stream():
        while True:
            time.sleep(0.5)
            with state_lock:
                payload = json.dumps(state)
            yield f"data: {payload}\n\n"
    return Response(event_stream(), mimetype="text/event-stream")

# --- Batch Logic ---

def run_batch(file_list, out_dir, audio_langs, sub_langs, skip_exists, dry_run=False):
    Path(out_dir).mkdir(parents=True, exist_ok=True)
    
    for i, f_path in enumerate(file_list):
        with state_lock:
            if state["cancel_flag"]:
                break
        
        f_path = Path(f_path)
        out_path = Path(out_dir) / f_path.name
        
        update_state(i, "processing", 0)
        
        if skip_exists and out_path.exists() and out_path.stat().st_size > 0:
            update_state(i, "skipped", 100)
            continue
            
        # Identify
        ident = identify_tracks(f_path)
        if not ident["ok"]:
            update_state(i, "error", 0, error=ident["err"])
            continue
            
        # Select
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
            update_state(i, "error", 0, error="No matching audio")
            continue

        # Dry-run: validate tracks only, do not write output
        if dry_run:
            update_state(i, "success", 100)
            continue
            
        # Command
        cmd = [MKVMERGE_PATH, "-o", str(out_path), "--audio-tracks", ",".join(audio_ids)]
        if sub_ids:
            cmd += ["--subtitle-tracks", ",".join(sub_ids)]
        else:
            cmd += ["--no-subtitles"]
        cmd += ["--ui-language", "en", str(f_path)]
        
        try:
            proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
            re_prog = re.compile(r"(?:#GUI#progress|Progress:)\s*([0-9]{1,3})")
            
            for line in iter(proc.stdout.readline, ""):
                with state_lock:
                    cancelled = state["cancel_flag"]
                if cancelled:
                    proc.terminate()
                    try:
                        proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait()
                    break
                m = re_prog.search(line)
                if m:
                    update_state(i, "processing", int(m.group(1)))
            
            if proc.poll() is None:
                proc.wait()
            with state_lock:
                cancelled = state["cancel_flag"]
            if cancelled:
                if out_path.exists():
                    try:
                        out_path.unlink()
                    except OSError:
                        pass
                update_state(i, "cancelled", 0)
            elif proc.returncode == 0 and out_path.exists() and out_path.stat().st_size > 0:
                update_state(i, "success", 100)
            else:
                update_state(i, "error", 0, error="mkvmerge failed")
                
        except Exception as e:
            update_state(i, "error", 0, error=str(e))
            
    with state_lock:
        state["batch_status"] = "idle"

def update_state(idx, status, pct, error=None):
    with state_lock:
        state["progress"][str(idx)] = {
            "status": status,
            "pct": pct,
            "error": error
        }

if __name__ == '__main__':
    logger.info(app.url_map)
    app.run(port=5000, threaded=True)
