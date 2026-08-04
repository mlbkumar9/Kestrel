// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use window_vibrancy::{apply_acrylic, apply_mica};

/// Set by `cancel_remux`; cleared by `reset_cancel` at batch start.
static CANCEL_FLAG: AtomicBool = AtomicBool::new(false);
/// Active mkvmerge remux child (stdout/stderr already taken). Kill + wait from cancel.
static CURRENT_CHILD: Mutex<Option<Child>> = Mutex::new(None);

fn take_and_kill_child() {
    if let Ok(mut guard) = CURRENT_CHILD.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn store_child(child: Child) {
    if let Ok(mut guard) = CURRENT_CHILD.lock() {
        *guard = Some(child);
    }
}

fn take_child() -> Option<Child> {
    CURRENT_CHILD.lock().ok().and_then(|mut g| g.take())
}

fn cleanup_partial_output(out_p: &Path) {
    if out_p.exists() {
        let _ = std::fs::remove_file(out_p);
    }
}

#[derive(Clone, serde::Serialize)]
struct FileInfo {
    name: String,
    path: String,
    size: u64,
}

#[derive(Clone, serde::Serialize)]
struct FileMetadata {
    name: String,
    duration: Option<f64>,
    audio_tracks: Vec<TrackInfo>,
    subtitle_tracks: Vec<TrackInfo>,
    video_codec: Option<String>,
    container: String,
}

#[derive(Clone, serde::Serialize)]
struct TrackInfo {
    id: u64,
    language: String,
    codec: String,
    title: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct ProgressEvent {
    id: String,
    status: String, // "pending", "processing", "success", "error", "skipped"
    pct: u32,
    error: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct AvailableLanguages {
    audio: Vec<String>,
    subtitles: Vec<String>,
}

#[derive(Clone, serde::Serialize)]
struct BatchStats {
    total_files: usize,
    total_size: u64,
    estimated_time: Option<f64>,
}

fn find_mkvmerge() -> Result<String, String> {
    let default_win = r"C:\Program Files\MKVToolNix\mkvmerge.exe";
    if Path::new(default_win).exists() {
        return Ok(default_win.to_string());
    }

    // Fall back to PATH lookup via `where`/`which`
    #[cfg(windows)]
    let lookup = Command::new("where").arg("mkvmerge").output();
    #[cfg(not(windows))]
    let lookup = Command::new("which").arg("mkvmerge").output();

    if let Ok(output) = lookup {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = stdout.lines().next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() && Path::new(trimmed).exists() {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }

    Err("mkvmerge not found. Install MKVToolNix and ensure mkvmerge is on PATH.".to_string())
}

fn normalize_langs(langs: &[String]) -> Vec<String> {
    langs
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_mkv_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("mkv"))
        .unwrap_or(false)
}

fn emit_progress(window: &tauri::Window, id: &str, status: &str, pct: u32, error: Option<String>) {
    let _ = window.emit(
        "progress",
        ProgressEvent {
            id: id.to_string(),
            status: status.to_string(),
            pct,
            error,
        },
    );
}

#[tauri::command]
fn check_mkvmerge() -> Result<String, String> {
    find_mkvmerge()
}

#[tauri::command]
fn reset_cancel() {
    CANCEL_FLAG.store(false, Ordering::SeqCst);
}

/// Kill the in-flight mkvmerge process (if any) and request batch abort.
#[tauri::command]
fn cancel_remux() -> Result<(), String> {
    CANCEL_FLAG.store(true, Ordering::SeqCst);
    take_and_kill_child();
    Ok(())
}

#[tauri::command]
fn scan_directory(path: String) -> Result<Vec<FileInfo>, String> {
    let p = Path::new(&path);
    if !p.exists() || !p.is_dir() {
        return Err("Invalid directory".to_string());
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(p) {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_mkv_path(&path) {
                if let Ok(metadata) = entry.metadata() {
                    files.push(FileInfo {
                        name: entry.file_name().to_string_lossy().to_string(),
                        path: path.to_string_lossy().to_string(),
                        size: metadata.len(),
                    });
                }
            }
        }
    }
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(files)
}

#[tauri::command]
fn get_file_metadata(input_path: String) -> Result<FileMetadata, String> {
    let mkvmerge = find_mkvmerge()?;

    let output = Command::new(&mkvmerge)
        .args(["--identify", "--identification-format", "json", &input_path])
        .output()
        .map_err(|e| format!("Failed to run mkvmerge: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let json_data: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| e.to_string())?;

    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();
    let mut video_codec = None;

    if let Some(tracks) = json_data["tracks"].as_array() {
        for track in tracks {
            let type_ = track["type"].as_str().unwrap_or("");
            let lang = track["properties"]["language"]
                .as_str()
                .unwrap_or("und")
                .to_lowercase();
            let id_val = track["id"].as_u64().unwrap_or(0);
            let codec = track["codec"].as_str().unwrap_or("unknown").to_string();
            let title = track["properties"]["track_name"]
                .as_str()
                .map(String::from);

            match type_ {
                "video" => {
                    if video_codec.is_none() {
                        video_codec = Some(codec.clone());
                    }
                }
                "audio" => {
                    audio_tracks.push(TrackInfo {
                        id: id_val,
                        language: lang,
                        codec,
                        title,
                    });
                }
                "subtitles" | "subtitle" => {
                    subtitle_tracks.push(TrackInfo {
                        id: id_val,
                        language: lang,
                        codec,
                        title,
                    });
                }
                _ => {}
            }
        }
    }

    let duration = json_data["container"]["properties"]["duration"]
        .as_f64()
        .map(|d| d / 1_000_000_000.0); // Convert nanoseconds to seconds

    let container = json_data["container"]["type"]
        .as_str()
        .unwrap_or("mkv")
        .to_string();

    let filename = Path::new(&input_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(FileMetadata {
        name: filename,
        duration,
        audio_tracks,
        subtitle_tracks,
        video_codec,
        container,
    })
}

#[tauri::command]
fn scan_available_languages(directory: String) -> Result<AvailableLanguages, String> {
    let files = scan_directory(directory)?;
    let mut audio_langs = std::collections::HashSet::new();
    let mut subtitle_langs = std::collections::HashSet::new();

    // Cap scan size for responsiveness on huge folders
    for file in files.into_iter().take(50) {
        if let Ok(metadata) = get_file_metadata(file.path) {
            for track in metadata.audio_tracks {
                audio_langs.insert(track.language);
            }
            for track in metadata.subtitle_tracks {
                subtitle_langs.insert(track.language);
            }
        }
    }

    let mut audio: Vec<String> = audio_langs.into_iter().collect();
    let mut subtitles: Vec<String> = subtitle_langs.into_iter().collect();
    audio.sort();
    subtitles.sort();

    Ok(AvailableLanguages { audio, subtitles })
}

#[tauri::command]
fn get_batch_stats(directory: String) -> Result<BatchStats, String> {
    let files = scan_directory(directory)?;
    let total_files = files.len();
    let total_size: u64 = files.iter().map(|f| f.size).sum();

    // Simple estimation: assume ~2 minutes per GB of data
    let estimated_time = if total_size > 0 {
        Some((total_size as f64 / 1_073_741_824.0) * 120.0)
    } else {
        None
    };

    Ok(BatchStats {
        total_files,
        total_size,
        estimated_time,
    })
}

#[tauri::command]
async fn remux_file(
    window: tauri::Window,
    id: String,
    input_path: String,
    output_path: String,
    audio_langs: Vec<String>,
    sub_langs: Vec<String>,
    skip_exists: bool,
    dry_run: Option<bool>,
) -> Result<(), String> {
    let dry_run = dry_run.unwrap_or(false);
    let audio_langs = normalize_langs(&audio_langs);
    let sub_langs = normalize_langs(&sub_langs);
    let out_p = Path::new(&output_path);

    if audio_langs.is_empty() {
        let error_msg = "At least one audio language is required".to_string();
        emit_progress(&window, &id, "error", 0, Some(error_msg.clone()));
        return Err(error_msg);
    }

    if let Some(parent) = out_p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    if skip_exists && out_p.exists() {
        if let Ok(meta) = out_p.metadata() {
            if meta.len() > 0 {
                emit_progress(&window, &id, "success", 100, None);
                return Ok(());
            }
        }
    }

    let mkvmerge = match find_mkvmerge() {
        Ok(path) => path,
        Err(err) => {
            emit_progress(&window, &id, "error", 0, Some(err.clone()));
            return Err(err);
        }
    };

    let output = Command::new(&mkvmerge)
        .args(["--identify", "--identification-format", "json", &input_path])
        .output()
        .map_err(|e| {
            let err_msg = format!("Failed to run mkvmerge: {}", e);
            emit_progress(&window, &id, "error", 0, Some(err_msg.clone()));
            err_msg
        })?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr).to_string();
        emit_progress(&window, &id, "error", 0, Some(error_msg.clone()));
        return Err(error_msg);
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let json_data: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| e.to_string())?;

    let mut audio_ids = Vec::new();
    let mut sub_ids = Vec::new();

    if let Some(tracks) = json_data["tracks"].as_array() {
        for track in tracks {
            let type_ = track["type"].as_str().unwrap_or("").to_lowercase();
            let lang = track["properties"]["language"]
                .as_str()
                .unwrap_or("und")
                .to_lowercase();
            let id_val = track["id"].as_u64().unwrap_or(0);

            if type_ == "audio" && audio_langs.iter().any(|l| l == &lang) {
                audio_ids.push(id_val.to_string());
            } else if (type_ == "subtitles" || type_ == "subtitle")
                && sub_langs.iter().any(|l| l == &lang)
            {
                sub_ids.push(id_val.to_string());
            }
        }
    }

    if audio_ids.is_empty() {
        let error_msg = "No matching audio tracks found".to_string();
        emit_progress(&window, &id, "error", 0, Some(error_msg.clone()));
        return Err(error_msg);
    }

    if dry_run {
        emit_progress(&window, &id, "success", 100, None);
        return Ok(());
    }

    let mut args = vec![
        "-o".to_string(),
        output_path.clone(),
        "--audio-tracks".to_string(),
        audio_ids.join(","),
    ];

    if !sub_ids.is_empty() {
        args.push("--subtitle-tracks".to_string());
        args.push(sub_ids.join(","));
    } else {
        args.push("--no-subtitles".to_string());
    }

    args.push("--ui-language".to_string());
    args.push("en".to_string());
    args.push(input_path.clone());

    if CANCEL_FLAG.load(Ordering::SeqCst) {
        emit_progress(&window, &id, "cancelled", 0, Some("Cancelled".to_string()));
        return Err("cancelled".to_string());
    }

    emit_progress(&window, &id, "processing", 0, None);

    // Pipe both streams; drain stderr on a side thread to avoid deadlocks
    let mut child = Command::new(&mkvmerge)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let err_msg = format!("Failed to spawn mkvmerge: {}", e);
            emit_progress(&window, &id, "error", 0, Some(err_msg.clone()));
            err_msg
        })?;

    let stderr_handle = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let mut reader = BufReader::new(stderr);
            let _ = reader.read_to_string(&mut buf);
            buf
        })
    });

    let stdout = child.stdout.take();
    // Expose Child so cancel_remux can kill mid-file without orphaning
    store_child(child);

    // Prefer GUI progress if present; fall back to "Progress: N%"
    let re = regex::Regex::new(r"(?:#GUI#progress|Progress:)\s*([0-9]{1,3})").unwrap();

    if let Some(stdout) = stdout {
        let reader = BufReader::new(stdout);
        for line in reader.lines().flatten() {
            if CANCEL_FLAG.load(Ordering::SeqCst) {
                break;
            }
            if let Some(caps) = re.captures(&line) {
                if let Some(m) = caps.get(1) {
                    if let Ok(pct) = m.as_str().parse::<u32>() {
                        let pct = pct.min(100);
                        emit_progress(&window, &id, "processing", pct, None);
                    }
                }
            }
        }
    }

    let cancelled = CANCEL_FLAG.load(Ordering::SeqCst);

    // Reap: cancel_remux may have already taken + waited the child
    let wait_result = match take_child() {
        Some(mut child) => {
            if cancelled {
                let _ = child.kill();
            }
            child.wait().map_err(|e| e.to_string())
        }
        None if cancelled => Err("cancelled".to_string()),
        None => Err("mkvmerge process handle lost".to_string()),
    };

    let error_output = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if cancelled {
        cleanup_partial_output(out_p);
        emit_progress(
            &window,
            &id,
            "cancelled",
            0,
            Some("Cancelled".to_string()),
        );
        return Err("cancelled".to_string());
    }

    let status = wait_result?;

    if status.success() && out_p.exists() {
        if let Ok(meta) = out_p.metadata() {
            if meta.len() > 0 {
                emit_progress(&window, &id, "success", 100, None);
                return Ok(());
            }
        }
    }

    // Clean up empty/partial outputs on failure
    if out_p.exists() {
        if let Ok(meta) = out_p.metadata() {
            if meta.len() == 0 {
                let _ = std::fs::remove_file(out_p);
            } else if !status.success() {
                let _ = std::fs::remove_file(out_p);
            }
        }
    }

    let error_msg = if !error_output.trim().is_empty() {
        format!("mkvmerge failed: {}", error_output.trim())
    } else {
        format!("mkvmerge failed with exit code: {:?}", status.code())
    };
    emit_progress(&window, &id, "error", 0, Some(error_msg.clone()));
    Err(error_msg)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "windows")]
                {
                    // Best-effort visual effect; never panic if unsupported
                    let _ = apply_mica(&window, Some(true))
                        .or_else(|_| apply_acrylic(&window, Some((18, 18, 18, 125))));
                }
                let _ = window;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_directory,
            remux_file,
            get_file_metadata,
            scan_available_languages,
            get_batch_stats,
            check_mkvmerge,
            cancel_remux,
            reset_cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
