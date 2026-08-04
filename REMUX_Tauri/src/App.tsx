import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { open as openShell } from "@tauri-apps/plugin-shell";
import { listen } from "@tauri-apps/api/event";
import { 
  Settings, 
  FileText, 
  FolderOpen, 
  Play, 
  Square, 
  CheckCircle2, 
  AlertCircle, 
  Loader2,
  HardDrive,
  Pause,
  ExternalLink,
  Trash2,
  ChevronLeft,
  ChevronRight,
  Upload,
  Clock,
  BarChart3,
  Info,
  X,
  Zap,
  Database,
  Activity,
  Home,
  Moon,
  Sun
} from "lucide-react";
import { cn } from "./lib/utils";

interface FileInfo {
  name: string;
  path: string;
  size: number;
}

interface ProgressEvent {
  id: string;
  status: "pending" | "processing" | "success" | "error" | "cancelled";
  pct: number;
  error?: string;
}

interface BatchStats {
  total_files: number;
  total_size: number;
  estimated_time: number | null;
}

interface Toast {
  id: number;
  message: string;
  type: "success" | "error" | "info";
}

function formatSize(bytes: number) {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

function formatTime(seconds: number) {
  if (!isFinite(seconds) || seconds < 0) return "--:--";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}h ${m}m ${s}s`;
  return `${m}m ${s}s`;
}

function App() {
  const [files, setFiles] = useState<FileInfo[]>([]);
  const [progress, setProgress] = useState<Record<string, ProgressEvent>>({});
  const [inputDir, setInputDir] = useState("");
  const [outputDir, setOutputDir] = useState("");
  const [isRunning, setIsRunning] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  
  // Stats
  const [startTime, setStartTime] = useState<number | null>(null);
  const [elapsedTime, setElapsedTime] = useState(0);
  const [eta, setEta] = useState<number | null>(null);
  const [batchStats, setBatchStats] = useState<BatchStats | null>(null);
  const [processingSpeed, setProcessingSpeed] = useState<number>(0); // MB/s
  const [averageSpeed, setAverageSpeed] = useState<number>(0); // Average MB/s after completion
  const [processedBytes, setProcessedBytes] = useState<number>(0);

  // Settings
  const [audioLangs, setAudioLangs] = useState("eng, jpn");
  const [subLangs, setSubLangs] = useState("eng");
  const [skipExists, setSkipExists] = useState(true);
  const [dryRun, setDryRun] = useState(false);
  const [availableLangs, setAvailableLangs] = useState<{ audio: string[]; subtitles: string[] } | null>(null);
  const [mkvmergePath, setMkvmergePath] = useState<string | null>(null);

  // UI State
  const [activePage, setActivePage] = useState<"dashboard" | "settings" | "logs">("dashboard");
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const [showHelp, setShowHelp] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  const [logLines, setLogLines] = useState<string[]>([]);

  // Apply theme to document
  useEffect(() => {
    const root = document.documentElement;
    
    if (theme === "light") {
      root.classList.remove("dark");
      root.classList.add("light");
      root.style.backgroundColor = "#ffffff";
      root.style.color = "#18181b";
    } else {
      root.classList.remove("light");
      root.classList.add("dark");
      root.style.backgroundColor = "#09090b";
      root.style.color = "#fafafa";
    }
  }, [theme]);
  
  const pausedRef = useRef(false);
  const abortRef = useRef(false);
  const toastIdRef = useRef(0);
  const filesRef = useRef<FileInfo[]>([]);
  const startTimeRef = useRef<number | null>(null);

  useEffect(() => {
    pausedRef.current = isPaused;
  }, [isPaused]);

  useEffect(() => {
    filesRef.current = files;
  }, [files]);

  useEffect(() => {
    startTimeRef.current = startTime;
  }, [startTime]);

  function joinPath(dir: string, file: string) {
    if (!dir) return file;
    const trimmed = dir.replace(/[\\/]+$/, "");
    return `${trimmed}\\${file}`;
  }

  function appendLog(message: string) {
    const stamp = new Date().toLocaleTimeString();
    setLogLines((prev) => [`[${stamp}] ${message}`, ...prev].slice(0, 200));
  }

  // Toast system
  const addToast = (message: string, type: "success" | "error" | "info") => {
    const id = ++toastIdRef.current;
    setToasts(prev => [...prev, { id, message, type }]);
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id));
    }, 4000);
  };

  useEffect(() => {
    invoke<string>("check_mkvmerge")
      .then((path) => {
        setMkvmergePath(path);
        appendLog(`mkvmerge found: ${path}`);
      })
      .catch(() => {
        setMkvmergePath(null);
        addToast("mkvmerge not found. Install MKVToolNix.", "error");
        appendLog("mkvmerge not found");
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Timer and ETA calculation
  useEffect(() => {
    let interval: ReturnType<typeof setInterval>;
    if (isRunning && startTime) {
      interval = setInterval(() => {
        const now = Date.now();
        const elapsed = (now - startTime) / 1000;
        setElapsedTime(elapsed);

        const completed = Object.values(progress).filter(p => p.status === 'success').length;
        if (completed > 0 && elapsed > 0) {
          const avgTimePerFile = elapsed / completed;
          const remaining = files.length - completed;
          setEta(avgTimePerFile * remaining);

          const completedSize = files.reduce((sum, f, idx) => {
            return progress[idx.toString()]?.status === "success" ? sum + f.size : sum;
          }, 0);
          setProcessingSpeed(completedSize / elapsed / 1024 / 1024);
        }
      }, 1000);
    } else {
      setProcessingSpeed(0);
    }
    return () => clearInterval(interval);
  }, [isRunning, startTime, progress, files]);

  useEffect(() => {
    const unlisten = listen<ProgressEvent>("progress", (event) => {
      setProgress((prev) => ({
        ...prev,
        [event.payload.id]: event.payload,
      }));

      const file = filesRef.current[parseInt(event.payload.id, 10)];
      if (event.payload.status === "success") {
        if (file) {
          setProcessedBytes((prev) => prev + file.size);
          appendLog(`OK: ${file.name}`);
        }
      } else if (event.payload.status === "error") {
        if (file) {
          addToast(`✗ ${file.name} failed: ${event.payload.error || "Unknown error"}`, "error");
          appendLog(`FAIL: ${file.name} — ${event.payload.error || "Unknown error"}`);
        }
      } else if (event.payload.status === "cancelled") {
        if (file) {
          appendLog(`CANCELLED: ${file.name}`);
        }
      }
    });

    const handleDropPaths = (paths: string[]) => {
      setIsDragging(false);
      if (paths.length > 0) {
        const path = paths[0];
        setInputDir(path);
        setOutputDir((prev) => prev || joinPath(path, "remuxed"));
        scanDir(path);
        addToast("Folder loaded successfully", "info");
      }
    };

    const unlistenDrop = listen<{ paths: string[] }>("tauri://drag-drop", (event) => {
      handleDropPaths(event.payload?.paths ?? []);
    });
    const unlistenHover = listen("tauri://drag-over", () => setIsDragging(true));
    const unlistenCancel = listen("tauri://drag-leave", () => setIsDragging(false));
    const unlistenDropLegacy = listen<{ paths: string[] }>("tauri://file-drop", (event) => {
      handleDropPaths(event.payload?.paths ?? []);
    });

    return () => {
      unlisten.then((f) => f());
      unlistenDrop.then((f) => f());
      unlistenHover.then((f) => f());
      unlistenCancel.then((f) => f());
      unlistenDropLegacy.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyPress = (e: KeyboardEvent) => {
      if (e.code === "Space" && !e.ctrlKey && !e.shiftKey && !e.altKey) {
        if (document.activeElement?.tagName !== "INPUT" && document.activeElement?.tagName !== "TEXTAREA") {
          e.preventDefault();
          if (isRunning) {
            setIsPaused(!isPaused);
          } else if (files.length > 0) {
            startRemux();
          }
        }
      }

      if (e.code === "Escape" && isRunning) {
        e.preventDefault();
        stopRemux();
      }

      if (e.ctrlKey && e.code === "KeyO" && !isRunning) {
        e.preventDefault();
        browseInput();
      }

      if (e.ctrlKey && e.code === "KeyB") {
        e.preventDefault();
        setIsSidebarCollapsed((prev) => !prev);
      }

      if (e.code === "F1") {
        e.preventDefault();
        setShowHelp(!showHelp);
      }
    };

    window.addEventListener("keydown", handleKeyPress);
    return () => window.removeEventListener("keydown", handleKeyPress);
  }, [isRunning, isPaused, files, showHelp]);

  async function browseInput() {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (selected && typeof selected === "string") {
      setInputDir(selected);
      setOutputDir((prev) => prev || joinPath(selected, "remuxed"));
      scanDir(selected);
    }
  }

  async function browseOutput() {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (selected && typeof selected === "string") {
      setOutputDir(selected);
    }
  }

  async function scanDir(path: string) {
    try {
      const res = await invoke<FileInfo[]>("scan_directory", { path });
      setFiles(res);
      setProgress({});
      setStartTime(null);
      setElapsedTime(0);
      setEta(null);
      setAverageSpeed(0);
      setProcessedBytes(0);
      appendLog(`Scanned ${res.length} MKV file(s) in ${path}`);

      try {
        const stats = await invoke<BatchStats>("get_batch_stats", { directory: path });
        setBatchStats(stats);
      } catch {
        // optional
      }

      try {
        const langs = await invoke<{ audio: string[]; subtitles: string[] }>("scan_available_languages", { directory: path });
        setAvailableLangs(langs);
      } catch {
        setAvailableLangs(null);
      }

      addToast(`Found ${res.length} MKV files`, "info");
    } catch {
      addToast("Failed to scan directory", "error");
      appendLog(`Failed to scan directory: ${path}`);
    }
  }

  async function startRemux() {
    if (files.length === 0) {
      addToast("No files to remux. Please load a folder first.", "error");
      return;
    }
    if (isRunning) {
      return;
    }
    if (!outputDir) {
      addToast("Please choose an output directory.", "error");
      return;
    }
    if (!mkvmergePath) {
      addToast("mkvmerge not found. Install MKVToolNix.", "error");
      return;
    }

    abortRef.current = false;
    try {
      await invoke("reset_cancel");
    } catch {
      // older builds without reset_cancel
    }
    setIsRunning(true);
    setIsPaused(false);
    const batchStart = Date.now();
    setStartTime(batchStart);
    setProcessedBytes(0);
    addToast(dryRun ? "Dry-run started" : "Batch processing started", "info");
    appendLog(dryRun ? "Dry-run started" : "Batch processing started");

    const aLangs = audioLangs.split(",").map((s) => s.trim().toLowerCase()).filter(Boolean);
    const sLangs = subLangs.split(",").map((s) => s.trim().toLowerCase()).filter(Boolean);
    if (aLangs.length === 0) {
      addToast("At least one audio language is required.", "error");
      setIsRunning(false);
      return;
    }

    let bytesDone = 0;
    const fileCount = files.length;

    const markRemainingCancelled = (fromIndex: number) => {
      setProgress((prev) => {
        const next = { ...prev };
        for (let j = fromIndex; j < fileCount; j++) {
          const fid = j.toString();
          const cur = next[fid];
          if (!cur || cur.status === "pending" || cur.status === "processing") {
            next[fid] = { id: fid, status: "cancelled", pct: 0 };
          }
        }
        return next;
      });
    };

    for (let i = 0; i < files.length; i++) {
      while (pausedRef.current && !abortRef.current) {
        await new Promise(r => setTimeout(r, 250));
      }
      if (abortRef.current) {
        markRemainingCancelled(i);
        appendLog("Batch aborted by user");
        break;
      }

      const f = files[i];
      const id = i.toString();

      setProgress((prev) => ({
        ...prev,
        [id]: { id, status: "pending", pct: 0 },
      }));

      try {
        await invoke("remux_file", {
          id,
          inputPath: f.path,
          outputPath: joinPath(outputDir, f.name),
          audioLangs: aLangs,
          subLangs: sLangs,
          skipExists,
          dryRun,
        });
        if (!abortRef.current) {
          bytesDone += f.size;
        }
      } catch (e) {
        const msg = String(e);
        const wasCancelled = abortRef.current || /cancelled/i.test(msg);
        setProgress((prev) => ({
          ...prev,
          [id]: {
            id,
            status: wasCancelled ? "cancelled" : "error",
            pct: 0,
            error: wasCancelled ? "Cancelled" : msg,
          },
        }));
        if (wasCancelled) {
          markRemainingCancelled(i + 1);
          appendLog("Batch aborted by user");
          break;
        }
      }
    }

    const finalElapsed = (Date.now() - batchStart) / 1000;
    if (finalElapsed > 0 && bytesDone > 0) {
      setAverageSpeed(bytesDone / finalElapsed / 1024 / 1024);
    }

    setIsRunning(false);
    setIsPaused(false);
    addToast(abortRef.current ? "Processing stopped" : "Batch processing complete", abortRef.current ? "info" : "success");
    appendLog(abortRef.current ? "Processing stopped" : "Batch processing complete");
  }

  function stopRemux() {
    abortRef.current = true;
    setIsPaused(false);
    invoke("cancel_remux").catch(() => {});
    addToast("Stopping — killing current remux...", "info");
    appendLog("Stop requested — killing mkvmerge if running");
  }

  function clearQueue() {
    if (isRunning) return;
    setFiles([]);
    setProgress({});
    setStartTime(null);
    setElapsedTime(0);
    setEta(null);
    setAverageSpeed(0);
    setProcessedBytes(0);
    setInputDir("");
    setBatchStats(null);
    setAvailableLangs(null);
    addToast("Queue cleared", "info");
  }

  const completedCount = Object.values(progress).filter(p => p.status === 'success').length;
  const errorCount = Object.values(progress).filter(p => p.status === 'error').length;
  const totalCount = files.length;
  const progressPct = totalCount > 0 ? (completedCount / totalCount) * 100 : 0;
  
  const filteredFiles = searchQuery 
    ? files.filter(f => f.name.toLowerCase().includes(searchQuery.toLowerCase()))
    : files;

  return (
    <div 
      className="h-screen w-full flex text-zinc-100 font-sans overflow-hidden selection:bg-blue-500/30 relative"
      style={{ backgroundColor: theme === "dark" ? "#09090b" : "#ffffff", color: theme === "dark" ? "#fafafa" : "#18181b" }}
    >
      
      {/* Toast Container */}
      <div className="fixed top-4 right-4 z-50 space-y-2 pointer-events-none">
        {toasts.map((toast) => (
          <div
            key={toast.id}
            className="glass-strong px-4 py-3 rounded-xl shadow-premium flex items-center gap-3 min-w-[300px] pointer-events-auto animate-slide-in-right"
          >
            {toast.type === "success" && <CheckCircle2 size={18} className="text-green-400" />}
            {toast.type === "error" && <AlertCircle size={18} className="text-red-400" />}
            {toast.type === "info" && <Info size={18} className="text-blue-400" />}
            <span className="text-sm flex-1">{toast.message}</span>
            <button
              onClick={() => setToasts(prev => prev.filter(t => t.id !== toast.id))}
              className="text-zinc-400 hover:text-zinc-200 transition-colors"
            >
              <X size={14} />
            </button>
          </div>
        ))}
      </div>

      {/* Help Overlay */}
      {showHelp && (
        <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-8 animate-fade-in" onClick={() => setShowHelp(false)}>
          <div className="glass-strong max-w-2xl w-full p-8 rounded-2xl shadow-premium animate-scale-in" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-2xl font-bold text-gradient-blue">Keyboard Shortcuts</h2>
              <button onClick={() => setShowHelp(false)} className="p-2 hover:bg-zinc-800 rounded-lg transition-colors">
                <X size={20} />
              </button>
            </div>
            
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-zinc-400">Play / Pause</span>
                  <kbd className="px-3 py-1 bg-zinc-900 border border-zinc-700 rounded text-sm">Space</kbd>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-zinc-400">Stop Processing</span>
                  <kbd className="px-3 py-1 bg-zinc-900 border border-zinc-700 rounded text-sm">Esc</kbd>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-zinc-400">Open Folder</span>
                  <kbd className="px-3 py-1 bg-zinc-900 border border-zinc-700 rounded text-sm">Ctrl + O</kbd>
                </div>
              </div>
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-zinc-400">Show Help</span>
                  <kbd className="px-3 py-1 bg-zinc-900 border border-zinc-700 rounded text-sm">F1</kbd>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-zinc-400">Toggle Sidebar</span>
                  <kbd className="px-3 py-1 bg-zinc-900 border border-zinc-700 rounded text-sm">Ctrl + B</kbd>
                </div>
              </div>
            </div>
            
            <div className="mt-6 pt-6 border-t border-zinc-800">
              <h3 className="text-sm font-semibold text-zinc-400 mb-3">Quick Tips</h3>
              <ul className="space-y-2 text-sm text-zinc-500">
                <li>• Drag and drop folders directly onto the window</li>
                <li>• Files are processed sequentially to avoid overwhelming your system</li>
                <li>• Output files are saved with the same name in the output directory</li>
              </ul>
            </div>
          </div>
        </div>
      )}
      
      {/* Drag Overlay */}
      {isDragging && (
        <div className="absolute inset-0 z-50 bg-blue-600/20 backdrop-blur-sm border-4 border-blue-500 border-dashed flex flex-col items-center justify-center text-blue-100 pointer-events-none animate-fade-in">
          <Upload size={64} className="mb-4 animate-bounce-subtle" />
          <h2 className="text-3xl font-bold text-gradient-blue">Drop Folder Here</h2>
          <p className="text-blue-200 mt-2">to scan for MKV files</p>
        </div>
      )}

      {/* Sidebar */}
      <div 
        className={cn(
          "flex flex-col border-r transition-all duration-300 ease-in-out",
          isSidebarCollapsed ? "w-20" : "w-64"
        )}
        style={{ 
          backgroundColor: theme === "dark" ? "#09090b" : "#f4f4f5",
          borderColor: theme === "dark" ? "rgba(39, 39, 42, 0.2)" : "rgba(228, 228, 231, 0.5)"
        }}
      >
        <div data-tauri-drag-region className="h-16 flex items-center px-6 border-b overflow-hidden whitespace-nowrap" style={{ borderColor: theme === "dark" ? "rgba(39, 39, 42, 0.2)" : "rgba(228, 228, 231, 0.5)" }}>
          <img 
            src="/app-icon.png" 
            alt="Remux Pro"
            className="w-8 h-8 min-w-[2rem] rounded-lg mr-3 shadow-glow-blue"
          />
          <span className={cn("font-bold text-lg tracking-tight transition-opacity duration-200 text-gradient-blue", isSidebarCollapsed ? "opacity-0 w-0" : "opacity-100")}>
            Remux Pro
          </span>
        </div>

        <div className="flex-1 py-6 px-3 space-y-2">
          {[
            { id: "dashboard", icon: Home, label: "Dashboard" },
            { id: "settings", icon: Settings, label: "Settings" },
            { id: "logs", icon: FileText, label: "Logs" }
          ].map((item) => (
            <button 
              key={item.id}
              onClick={() => setActivePage(item.id as any)}
              className={cn(
                "w-full flex items-center px-3 py-3 rounded-xl transition-all group relative",
                activePage === item.id 
                  ? "bg-zinc-800 text-white shadow-inner" 
                  : "text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/50",
                isSidebarCollapsed && "justify-center px-0"
              )}
              title={isSidebarCollapsed ? item.label : undefined}
            >
              <item.icon size={20} className={cn("transition-colors", activePage === item.id ? "text-blue-400" : "group-hover:text-zinc-200")} />
              <span className={cn("ml-3 font-medium text-sm transition-all duration-200 overflow-hidden whitespace-nowrap", isSidebarCollapsed ? "w-0 opacity-0 ml-0" : "w-auto opacity-100")}>
                {item.label}
              </span>
              
              {/* Active Indicator */}
              {activePage === item.id && (
                <div className="absolute left-0 top-1/2 -translate-y-1/2 w-1 h-8 bg-blue-500 rounded-r-full" />
              )}
            </button>
          ))}
        </div>

        {/* Batch Info Widget - Always show when sidebar expanded */}
        {!isSidebarCollapsed && (
          <div className="px-4 py-4 border-t space-y-2" style={{ borderColor: theme === "dark" ? "rgba(39, 39, 42, 0.5)" : "rgba(228, 228, 231, 0.8)" }}>
            <h4 className="text-xs font-bold uppercase tracking-wider" style={{ color: theme === "dark" ? "#71717a" : "#52525b" }}>Batch Info</h4>
            <div className="space-y-1 text-xs">
              {batchStats ? (
                <>
                  <div className="flex justify-between" style={{ color: theme === "dark" ? "#a1a1aa" : "#71717a" }}>
                    <span>Files:</span>
                    <span className="font-mono" style={{ color: theme === "dark" ? "#d4d4d8" : "#3f3f46" }}>{batchStats.total_files}</span>
                  </div>
                  <div className="flex justify-between" style={{ color: theme === "dark" ? "#a1a1aa" : "#71717a" }}>
                    <span>Size:</span>
                    <span className="font-mono" style={{ color: theme === "dark" ? "#d4d4d8" : "#3f3f46" }}>{formatSize(batchStats.total_size)}</span>
                  </div>
                  {batchStats.estimated_time && batchStats.estimated_time > 0 && (
                    <div className="flex justify-between" style={{ color: theme === "dark" ? "#a1a1aa" : "#71717a" }}>
                      <span>Est. Time:</span>
                      <span className="font-mono" style={{ color: theme === "dark" ? "#d4d4d8" : "#3f3f46" }}>{formatTime(batchStats.estimated_time)}</span>
                    </div>
                  )}
                  {processedBytes > 0 && (
                    <div className="flex justify-between" style={{ color: theme === "dark" ? "#a1a1aa" : "#71717a" }}>
                      <span>Processed:</span>
                      <span className="font-mono" style={{ color: theme === "dark" ? "#d4d4d8" : "#3f3f46" }}>{formatSize(processedBytes)}</span>
                    </div>
                  )}
                </>
              ) : (
                <div className="text-center py-2" style={{ color: theme === "dark" ? "#52525b" : "#a1a1aa" }}>
                  No folder loaded
                </div>
              )}
            </div>
          </div>
        )}

        <div className="p-4 border-t flex justify-end" style={{ borderColor: theme === "dark" ? "rgba(39, 39, 42, 0.5)" : "rgba(228, 228, 231, 0.8)" }}>
          <button 
            onClick={() => setIsSidebarCollapsed(!isSidebarCollapsed)}
            className="p-2 rounded-lg transition-colors"
            style={{ 
              color: theme === "dark" ? "#71717a" : "#a1a1aa",
              backgroundColor: "transparent"
            }}
            onMouseEnter={(e) => e.currentTarget.style.backgroundColor = theme === "dark" ? "#27272a" : "#e4e4e7"}
            onMouseLeave={(e) => e.currentTarget.style.backgroundColor = "transparent"}
            title="Toggle Sidebar"
          >
            {isSidebarCollapsed ? <ChevronRight size={18} /> : <ChevronLeft size={18} />}
          </button>
        </div>
      </div>

      {/* Main Content */}
      <div 
        className="flex-1 flex flex-col min-w-0 relative"
        style={{ backgroundColor: theme === "dark" ? "#09090b" : "#ffffff" }}
      >
        {/* Header */}
        <div data-tauri-drag-region className="h-16 flex items-center justify-between px-8 border-b border-zinc-800/20 bg-[#09090b] z-10">
          <div>
            <h1 className="text-3xl font-semibold tracking-tight text-zinc-200">
              {activePage === "dashboard" && "Dashboard"}
              {activePage === "settings" && "Settings"}
              {activePage === "logs" && "System Logs"}
            </h1>
          </div>
          
          {activePage === "dashboard" && (
            <button
              onClick={() => setShowHelp(true)}
              className="btn-ghost text-xs"
              title="Keyboard Shortcuts (F1)"
            >
              <Info size={16} className="mr-1" />
              Help
            </button>
          )}
        </div>

        {/* Scrollable Content */}
        <div className="flex-1 overflow-hidden flex flex-col p-6 pb-28">
          {activePage === "dashboard" && (
            <div className="flex-1 flex flex-col gap-6 h-full animate-fade-in">
              {/* Stats Cards - Always visible */}
              <div className="grid grid-cols-4 gap-4 flex-shrink-0 animate-slide-in-up">
                <div className="card-glass p-4">
                  <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-lg bg-blue-500/10 flex items-center justify-center">
                      <Activity size={20} className="text-blue-400" />
                    </div>
                    <div className="flex-1">
                      <div className="text-xs text-zinc-500">Completed</div>
                      <div className="text-2xl font-bold text-gradient-blue">{completedCount}/{totalCount || 0}</div>
                    </div>
                  </div>
                </div>
                
                <div className="card-glass p-4">
                  <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-lg bg-green-500/10 flex items-center justify-center">
                      <Zap size={20} className="text-green-400" />
                    </div>
                    <div className="flex-1">
                      <div className="text-xs text-zinc-500">Speed</div>
                      <div className="text-2xl font-bold text-gradient-green">
                        {isRunning ? processingSpeed.toFixed(1) : averageSpeed > 0 ? averageSpeed.toFixed(1) : "0.0"} MB/s
                      </div>
                      {!isRunning && averageSpeed > 0 && <div className="text-[10px] text-zinc-600">avg</div>}
                    </div>
                  </div>
                </div>
                
                <div className="card-glass p-4">
                  <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-lg bg-yellow-500/10 flex items-center justify-center">
                      <Clock size={20} className="text-yellow-400" />
                    </div>
                    <div className="flex-1">
                      <div className="text-xs text-zinc-500">Elapsed</div>
                      <div className="text-lg font-bold text-yellow-400">{formatTime(elapsedTime)}</div>
                    </div>
                  </div>
                </div>
                
                <div className="card-glass p-4">
                  <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-lg bg-purple-500/10 flex items-center justify-center">
                      <BarChart3 size={20} className="text-purple-400" />
                    </div>
                    <div className="flex-1">
                      <div className="text-xs text-zinc-500">Errors</div>
                      <div className="text-2xl font-bold text-red-400">{errorCount}</div>
                    </div>
                  </div>
                </div>
              </div>
              
              {/* Configuration Cards */}
              <div className="grid grid-cols-2 gap-4 flex-shrink-0">
                <div className="card-premium">
                  <label className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider mb-2 block">Input Source</label>
                  <div className="flex gap-2">
                    <div className="flex-1 bg-zinc-950 border border-zinc-800 rounded-lg px-3 py-2 text-sm text-zinc-300 font-mono truncate hover:border-zinc-700 transition-colors shadow-sm">
                      {inputDir || "No folder selected..."}
                    </div>
                    <button 
                      onClick={browseInput}
                      disabled={isRunning}
                      className="px-3 py-2 bg-zinc-800 hover:bg-zinc-700 disabled:opacity-50 text-zinc-200 rounded-lg transition-all shadow-sm hover:scale-105 active:scale-95"
                      title="Browse for input folder (Ctrl+O)"
                    >
                      <FolderOpen size={18} />
                    </button>
                  </div>
                </div>

                <div className="card-premium">
                  <label className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider mb-2 block">Output Destination</label>
                  <div className="flex gap-2">
                    <div className="flex-1 bg-zinc-950 border border-zinc-800 rounded-lg px-3 py-2 text-sm text-zinc-300 font-mono truncate hover:border-zinc-700 transition-colors shadow-sm">
                      {outputDir || "Auto-generated (./remuxed)"}
                    </div>
                    <button 
                      onClick={browseOutput}
                      disabled={isRunning}
                      className="px-3 py-2 bg-zinc-800 hover:bg-zinc-700 disabled:opacity-50 text-zinc-200 rounded-lg transition-all shadow-sm hover:scale-105 active:scale-95"
                      title="Browse for output folder"
                    >
                      <FolderOpen size={18} />
                    </button>
                  </div>
                </div>
              </div>

              {/* Queue Section */}
              <div className="flex-1 bg-zinc-900/30 border border-zinc-800/20 rounded-xl overflow-hidden flex flex-col min-h-0 shadow-premium">
                <div className="px-5 py-3 border-b border-zinc-800 flex items-center justify-between glass backdrop-blur">
                  <div className="flex items-center gap-4">
                    <div className="flex items-center gap-3">
                      <HardDrive size={16} className="text-zinc-400" />
                      <h3 className="text-xs font-bold text-zinc-300 uppercase tracking-wider">Processing Queue</h3>
                      <span className="px-2 py-0.5 rounded-full bg-zinc-800 text-zinc-400 text-[10px] font-mono">{filteredFiles.length}</span>
                    </div>
                    
                    {files.length > 0 && (
                      <input
                        type="text"
                        placeholder="Search files..."
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                        className="px-3 py-1 bg-zinc-950 border border-zinc-800 rounded-lg text-xs focus:border-blue-500 focus:ring-1 focus:ring-blue-500/20 transition-all w-48"
                      />
                    )}
                  </div>
                  
                  {files.length > 0 && !isRunning && (
                    <button 
                      onClick={clearQueue}
                      className="text-xs text-red-400 hover:text-red-300 flex items-center gap-1 px-2 py-1 rounded hover:bg-red-500/10 transition-all"
                      title="Clear entire queue"
                    >
                      <Trash2 size={12} />
                      Clear
                    </button>
                  )}
                </div>

                <div className="flex-1 overflow-auto custom-scrollbar">
                  {filteredFiles.length === 0 ? (
                    <div className="h-full flex flex-col items-center justify-center text-zinc-500 space-y-4 opacity-50">
                      <div className="w-16 h-16 rounded-full bg-zinc-800/50 flex items-center justify-center">
                        <Upload size={24} />
                      </div>
                      <div className="text-center">
                        <p className="text-sm font-medium">{files.length > 0 ? "No files match your search" : "No files in queue"}</p>
                        {files.length === 0 && <p className="text-xs mt-1">Drag & drop a folder or click Input Source</p>}
                      </div>
                    </div>
                  ) : (
                    <table className="w-full text-left text-sm">
                      <thead className="bg-zinc-900/90 sticky top-0 z-10 backdrop-blur-sm shadow-sm">
                        <tr className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider border-b border-zinc-800">
                          <th className="px-6 py-3 w-16">#</th>
                          <th className="px-6 py-3">Filename</th>
                          <th className="px-6 py-3 w-28 text-right">Size</th>
                          <th className="px-6 py-3 w-36">Status</th>
                          <th className="px-6 py-3 w-24 text-right">Progress</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-zinc-800/50">
                        {filteredFiles.map((f, i) => {
                          const originalIndex = files.indexOf(f);
                          const p = progress[originalIndex.toString()];
                          const status = p?.status || "pending";
                          
                          return (
                            <tr key={i} className="group hover:bg-zinc-800/30 transition-all text-xs">
                              <td className="px-6 py-3 text-zinc-500 font-mono">{originalIndex + 1}</td>
                              <td className="px-6 py-3 font-medium text-zinc-300 truncate max-w-[300px]" title={f.name}>{f.name}</td>
                              <td className="px-6 py-3 text-zinc-500 text-right font-mono">{formatSize(f.size)}</td>
                              <td className="px-6 py-3">
                                {status === "pending" && <span className="text-zinc-500 flex items-center gap-1.5"><Clock size={12}/> Pending</span>}
                                {status === "processing" && <span className="text-blue-400 flex items-center gap-1.5"><Loader2 size={12} className="animate-spin"/> Processing</span>}
                                {status === "success" && <span className="text-green-400 flex items-center gap-1.5"><CheckCircle2 size={12}/> Done</span>}
                                {status === "error" && <span className="text-red-400 flex items-center gap-1.5" title={p?.error}><AlertCircle size={12}/> Failed</span>}
                                {status === "cancelled" && <span className="text-yellow-500 flex items-center gap-1.5"><Square size={12}/> Cancelled</span>}
                              </td>
                              <td className="px-6 py-3 text-right">
                                {status === "processing" && (
                                  <div className="flex items-center gap-2 justify-end">
                                    <div className="w-16 h-1.5 bg-zinc-800 rounded-full overflow-hidden">
                                      <div 
                                        className="h-full bg-gradient-to-r from-blue-600 to-blue-500 transition-all duration-300"
                                        style={{ width: `${p.pct}%` }}
                                      />
                                    </div>
                                    <span className="font-mono text-zinc-400 w-10 text-right">{p.pct}%</span>
                                  </div>
                                )}
                                {status === "success" && <span className="font-mono text-green-400">100%</span>}
                                {status === "error" && <span className="font-mono text-red-400">0%</span>}
                                {status === "cancelled" && <span className="font-mono text-yellow-500">—</span>}
                                {status === "pending" && <span className="font-mono text-zinc-600">-</span>}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  )}
                </div>
              </div>
            </div>
          )}

          {activePage === "settings" && (
            <div className="max-w-2xl mx-auto space-y-8 w-full animate-fade-in overflow-auto custom-scrollbar" style={{maxHeight: 'calc(100vh - 200px)'}}>
              <div className="card-premium">
                <h3 className="text-lg font-medium mb-6 flex items-center gap-2">
                  <Database size={20} className="text-blue-400" />
                  Language Preferences
                </h3>
                <div className="space-y-6">
                  <div className="space-y-2">
                    <label className="text-sm font-medium text-zinc-300">Audio Languages</label>
                    <input 
                      value={audioLangs}
                      onChange={(e) => setAudioLangs(e.target.value)}
                      disabled={isRunning}
                      className="input-premium"
                      placeholder="e.g. eng, jpn"
                    />
                    <p className="text-xs text-zinc-500">Comma-separated ISO 639-2 codes (e.g. eng, jpn, spa, fre)</p>
                  </div>
                  
                  <div className="space-y-2">
                    <label className="text-sm font-medium text-zinc-300">Subtitle Languages</label>
                    <input 
                      value={subLangs}
                      onChange={(e) => setSubLangs(e.target.value)}
                      disabled={isRunning}
                      className="input-premium"
                      placeholder="e.g. eng"
                    />
                    <p className="text-xs text-zinc-500">Comma-separated ISO 639-2 codes</p>
                  </div>
                </div>
              </div>

              <div className="card-premium">
                <h3 className="text-lg font-medium mb-6 flex items-center gap-2">
                  <Settings size={20} className="text-blue-400" />
                  Processing Options
                </h3>
                <div className="space-y-4">
                  <div className="flex items-center justify-between">
                    <div className="space-y-0.5">
                      <label className="text-sm font-medium text-zinc-300">Skip Existing Files</label>
                      <p className="text-xs text-zinc-500">Do not re-process files that already exist in the output directory</p>
                    </div>
                    <input 
                      type="checkbox"
                      checked={skipExists}
                      onChange={(e) => setSkipExists(e.target.checked)}
                      disabled={isRunning}
                      className="h-5 w-5 rounded border-zinc-700 bg-zinc-950 text-blue-600 focus:ring-blue-500 focus:ring-offset-zinc-900"
                    />
                  </div>
                  <div className="flex items-center justify-between">
                    <div className="space-y-0.5">
                      <label className="text-sm font-medium text-zinc-300">Dry Run</label>
                      <p className="text-xs text-zinc-500">Preview matching tracks without writing output files</p>
                    </div>
                    <input 
                      type="checkbox"
                      checked={dryRun}
                      onChange={(e) => setDryRun(e.target.checked)}
                      disabled={isRunning}
                      className="h-5 w-5 rounded border-zinc-700 bg-zinc-950 text-blue-600 focus:ring-blue-500 focus:ring-offset-zinc-900"
                    />
                  </div>
                </div>
                {availableLangs && (
                  <div className="mt-6 pt-4 border-t border-zinc-800 text-xs text-zinc-400 space-y-1">
                    <div>Detected audio: <span className="text-zinc-200 font-mono">{availableLangs.audio.join(", ") || "none"}</span></div>
                    <div>Detected subs: <span className="text-zinc-200 font-mono">{availableLangs.subtitles.join(", ") || "none"}</span></div>
                  </div>
                )}
                <div className="mt-4 text-xs text-zinc-500">
                  mkvmerge: <span className="font-mono text-zinc-300">{mkvmergePath || "not found"}</span>
                </div>
              </div>

              <div className="card-premium">
                <h3 className="text-lg font-medium mb-6 flex items-center gap-2">
                  {theme === "dark" ? <Moon size={20} className="text-blue-400" /> : <Sun size={20} className="text-blue-400" />}
                  Appearance
                </h3>
                <div className="flex items-center justify-between">
                  <div className="space-y-0.5">
                    <label className="text-sm font-medium text-zinc-300">Theme</label>
                    <p className="text-xs text-zinc-500">Switch between dark and light mode</p>
                  </div>
                  <button
                    onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
                    className={cn(
                      "relative inline-flex h-6 w-11 items-center rounded-full transition-colors",
                      theme === "dark" ? "bg-blue-600" : "bg-zinc-700"
                    )}
                  >
                    <span
                      className={cn(
                        "inline-block h-4 w-4 transform rounded-full bg-white transition-transform",
                        theme === "dark" ? "translate-x-6" : "translate-x-1"
                      )}
                    />
                  </button>
                </div>
              </div>

              <div className="card-glass border-blue-500/20">
                <div className="flex items-start gap-3">
                  <Info size={20} className="text-blue-400 flex-shrink-0 mt-0.5" />
                  <div className="text-sm text-zinc-400">
                    <p className="font-medium text-zinc-300 mb-1">About Language Codes</p>
                    <p>Use ISO 639-2 three-letter codes. Common codes: <code className="px-1 py-0.5 bg-zinc-800 rounded text-xs">eng</code> (English), <code className="px-1 py-0.5 bg-zinc-800 rounded text-xs">jpn</code> (Japanese), <code className="px-1 py-0.5 bg-zinc-800 rounded text-xs">spa</code> (Spanish), <code className="px-1 py-0.5 bg-zinc-800 rounded text-xs">fre</code> (French), <code className="px-1 py-0.5 bg-zinc-800 rounded text-xs">ger</code> (German).</p>
                  </div>
                </div>
              </div>
            </div>
          )}
          
          {activePage === "logs" && (
             <div className="h-full bg-zinc-950 border border-zinc-800 rounded-xl p-4 font-mono text-xs text-zinc-400 overflow-auto custom-scrollbar shadow-inner animate-fade-in">
                <div className="mb-2 text-zinc-500 flex items-center gap-2">
                  <div className="w-2 h-2 rounded-full bg-green-500 animate-pulse" />
                  System initialized. Ready for processing.
                </div>
                {logLines.length === 0 && Object.keys(progress).length === 0 && (
                  <div className="text-zinc-600">No log entries yet.</div>
                )}
                {logLines.map((line, idx) => (
                  <div key={`log-${idx}`} className="mb-1 py-0.5 text-zinc-400">{line}</div>
                ))}
                {Object.entries(progress).map(([id, p]) => {
                  const file = files[parseInt(id)];
                  return (
                    <div key={id} className="mb-1 py-1">
                      <div className="flex items-center gap-2">
                        <span className="text-zinc-300">{file?.name || `File ${id}`}:</span>
                        <span className={cn(
                          "font-semibold",
                          p.status === "success" && "text-green-400",
                          p.status === "error" && "text-red-400",
                          p.status === "processing" && "text-blue-400",
                          p.status === "pending" && "text-yellow-400",
                          p.status === "cancelled" && "text-yellow-500"
                        )}>
                          {p.status.toUpperCase()}
                        </span>
                        {p.status === "processing" && <span className="text-zinc-500">({p.pct}%)</span>}
                      </div>
                      {p.error && p.status === "error" && <div className="text-red-500 ml-8 text-[11px]">└─ Error: {p.error}</div>}
                    </div>
                  );
                })}
             </div>
          )}
        </div>

        {/* Floating Status Bar */}
        <div className="absolute bottom-6 left-6 right-6 h-20 glass-strong rounded-2xl shadow-premium flex items-center px-6 gap-6 z-40 border border-zinc-800/50">
           {/* Progress Info */}
           <div className="flex-1 space-y-2.5">
              <div className="flex items-center justify-between text-xs">
                 <div className="flex items-center gap-4">
                    <span className="font-bold text-zinc-200 tracking-wide">BATCH PROGRESS</span>
                    <span className="text-zinc-400 bg-zinc-800/50 px-2 py-0.5 rounded-full">{completedCount} / {totalCount}</span>
                 </div>
                 <div className="flex items-center gap-4 font-mono text-zinc-400 text-[11px]">
                    <span className="flex items-center gap-1.5"><Clock size={12}/> {formatTime(elapsedTime)}</span>
                    <span className="text-zinc-600">|</span>
                    <span>ETA: {eta !== null ? formatTime(eta) : "--:--"}</span>
                    {isRunning && processingSpeed > 0 && (
                      <>
                        <span className="text-zinc-600">|</span>
                        <span className="flex items-center gap-1.5"><Activity size={12}/> {processingSpeed.toFixed(1)} MB/s</span>
                      </>
                    )}
                 </div>
              </div>
              {/* Rounded Pill Progress Bar with animated gradient */}
              <div className="h-2.5 w-full bg-zinc-950 rounded-full overflow-hidden border border-zinc-800/50 shadow-inner">
                 <div 
                   className={cn(
                     "h-full rounded-full transition-all duration-500 relative overflow-hidden",
                     isRunning ? "bg-gradient-to-r from-blue-600 via-blue-500 to-blue-600 bg-[length:200%_100%] animate-progress-flow" : "bg-zinc-700"
                   )}
                   style={{ width: `${progressPct}%` }}
                 >
                    {isRunning && progressPct < 100 && <div className="absolute inset-0 bg-white/10 animate-shimmer" />}
                 </div>
              </div>
           </div>

           {/* Controls */}
           <div className="flex items-center gap-3 pl-6 border-l border-zinc-800/50 h-10">
              <button 
                 onClick={() => outputDir ? openShell(outputDir) : null}
                 disabled={!outputDir}
                 className="p-2.5 rounded-xl bg-zinc-800/50 hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 transition-all disabled:opacity-30 hover:scale-105 active:scale-95"
                 title="Open Output Folder"
              >
                 <ExternalLink size={18} />
              </button>
              
              {isRunning && (
                <button 
                   onClick={() => setIsPaused(!isPaused)}
                   className={cn(
                     "p-2.5 rounded-xl transition-all hover:scale-105 active:scale-95",
                     isPaused ? "bg-yellow-500/10 text-yellow-500 border border-yellow-500/20 shadow-glow-blue" : "bg-zinc-800/50 text-zinc-400 hover:text-zinc-200"
                   )}
                   title={isPaused ? "Resume (Space)" : "Pause (Space)"}
                >
                   {isPaused ? <Play size={18} fill="currentColor" /> : <Pause size={18} fill="currentColor" />}
                </button>
              )}

              <button 
                onClick={isRunning ? stopRemux : startRemux}
                disabled={files.length === 0}
                className={cn(
                  "flex items-center gap-2 px-6 py-2.5 rounded-xl text-sm font-bold transition-all shadow-lg hover:scale-105 active:scale-95",
                  isRunning 
                    ? "bg-red-500/10 text-red-500 hover:bg-red-500/20 border border-red-500/20" 
                    : "bg-gradient-to-r from-blue-600 to-blue-500 hover:from-blue-500 hover:to-blue-400 text-white shadow-glow-blue disabled:opacity-50 disabled:cursor-not-allowed"
                )}
                title={isRunning ? "Stop (Esc)" : "Start Batch (Space)"}
              >
                {isRunning ? <Square size={16} fill="currentColor" /> : <Play size={16} fill="currentColor" />}
                {isRunning ? "STOP" : "START BATCH"}
              </button>
           </div>
        </div>
      </div>
    </div>
  );
}

export default App;
