# LangMux Roadmap

LangMux is moving from a single-purpose MKV language remuxer to a fast, native media toolbox built on MKVToolNix and FFmpeg. The native Rust app (`LangMux_Native/`) is the primary app from v2.0.0 on.

## v2.0 — Native app (current)

- Native Rust (eframe/egui) desktop app with a refined dark interface.
- Language-based remux of audio and subtitle tracks with MKVToolNix (`mkvmerge`).
- FFmpeg integration: `ffprobe` reads metadata as a fallback, and FFmpeg can act as a stream-copy remux engine when MKVToolNix is not available.
- The Windows release bundles `mkvmerge`, `ffmpeg` and `ffprobe`. macOS and Linux use the tools installed on the system.
- CI builds Windows, macOS and Linux binaries for every tagged release.

## v2.1 — Cross-platform parity

- Signed and notarized macOS `.dmg` (Apple Silicon and Intel) that bundles FFmpeg and MKVToolNix.
- Linux AppImage plus `.deb` packages, with FFmpeg and MKVToolNix declared as dependencies or bundled.
- Signed Windows installer (MSI/NSIS) as an alternative to the portable zip.
- In-app update check against GitHub Releases.
- Per-platform default tool paths, plus a Settings screen to override the paths to `mkvmerge`, `ffmpeg` and `ffprobe`.

## v2.2 — Track editor

- Per-file track table: enable or disable individual tracks, reorder them, set the default and forced flags, and edit language and title tags.
- Bulk rules such as "keep the first `eng` audio track and drop commentary" and "keep forced subtitles only".
- Saved remux profiles.

## v3.0 — Transcoding

The transcode UI provides a source, a preset, tabbed settings and a queue.

- **Source and queue:** add files or folders; each queue item stores a snapshot of its settings. Start, pause and stop the queue; reorder and retry items.
- **Presets:** built-in presets (Fast 1080p H.264, HQ 1080p H.265, 4K HEVC 10-bit, AV1 SVT, Web/Discord, Apple devices) plus user presets you can import and export as JSON.
- **Summary tab:** container (MKV/MP4/WebM), web-optimized/faststart, and metadata passthrough.
- **Dimensions tab:** resolution limit, anamorphic mode, cropping (automatic via `cropdetect` or custom), and rotation.
- **Filters tab:** deinterlace (yadif/bwdif), denoise (hqdn3d/nlmeans), deblock, sharpen, grayscale, and color-space conversion.
- **Video tab:** encoder (x264, x265, SVT-AV1, VP9, NVENC, QSV, AMF, VideoToolbox), constant quality (CRF/CQ) or average bitrate with 2-pass, encoder preset/tune/profile/level, frame rate (same as source, constant or peak), and extra encoder options.
- **Audio tab:** track selection rules (reusing LangMux language rules), and per track: encoder (AAC, Opus, AC3, E-AC3, FLAC or passthrough), bitrate, mixdown, sample rate and DRC/gain.
- **Subtitles tab:** language-based selection, burn-in, forced-only, default track, and import of external SRT/ASS files.
- **Chapters tab:** keep chapters, and import or export chapter names.
- **Preview:** short encode preview of a chosen segment, with before/after frames.
- **Engine:** FFmpeg run as a child process with `-progress pipe:1` for per-file progress, an ETA and fps. Hardware encoders are detected at startup with `ffmpeg -encoders` and per-device test encodes.
- **Activity log:** the full FFmpeg command line and output for each job, which you can copy and export.

## Later

- Watch folders (automatically process new files).
- Command-line interface that shares the same core crate (`langmux-core`) for scripting and servers.
- Split the core logic into its own crate so the CLI, GUI and any future server share it.
