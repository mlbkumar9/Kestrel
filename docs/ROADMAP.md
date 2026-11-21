# Kestrel roadmap

What shipped, what comes next. Back to the [README](../README.md).

## 4.0 (released 2026-11-19)

Kestrel 4.0 is released as [v4.0.0](https://github.com/mlbkumar9/Kestrel/releases/tag/v4.0.0). The 4.0 app is the GPUI desktop app in `crates/kestrel-gpui`, on top of a reworked `kestrel-core`.

- **New app.** A GPUI desktop app with a Library, Remux, Transcode, Queue, Activity, Metrics (placeholder) and Settings page, dark, light and system themes, a command palette (`Ctrl K`) and keyboard shortcuts.
- **Remux track editor.** Per file: keep or drop each track, edit language, title, default and forced flags. Language chips for batches. Output to MKV, MP4, MOV or WebM with warnings before anything is dropped. MKVToolNix and FFmpeg engines. Dry run and collision policy (rename, skip, overwrite).
- **Add tracks from files.** External SRT, ASS, SSA, VTT and SUP subtitles and external audio files muxed into a remux, with language guessed from the file name. Each added file has a timing offset and, for text subtitles, a character set. Source and added tracks can be reordered.
- **HandBrake-level Transcode.** Eight tabs: Summary, Dimensions, Filters, Video, Audio, Subtitles, Chapters, Tags. Encoders: x264, x265 (8, 10, 12-bit), SVT-AV1, libaom, VP9, and NVENC, QSV, AMF, VideoToolbox and VAAPI where the FFmpeg build and the device allow. Constant quality or average bitrate with two-pass. Crop, scale, borders, rotation, deinterlace, detelecine, denoise, sharpen, deblock, colourspace, HDR10 keep or tone-map to SDR. Per-track audio with passthru rules. Subtitle import and burn-in. Chapter renaming. Tags.
- **Queue, Activity and presets.** Job snapshots, retry, reveal output, a command log you can copy or export, five built-in presets plus a hardware preset, and user presets you can rename, duplicate, delete, import and export.
- **Transcode preview.** A 15, 30 or 60 second segment encoded with the current settings, with size, bitrate, extrapolated full size, playback and source against encoded stills.
- **Top bar.** Sidebar toggle, history buttons, search, queue pill, activity bell, theme toggle and the Add media and Start queue buttons.
- **Settings that act.** Parallel jobs, process priority and keep source dates are applied.
- **Release packaging.** Windows zip and MSI with bundled FFmpeg and mkvmerge, a standalone exe, macOS zips for Apple silicon and Intel, a Linux tar.gz, the legacy egui zip and `SHA256SUMS.txt`.
- **Hardening.** 260 tests pass on Windows, including real FFmpeg integration tests for the transcode controls and remux edits.

### Known gaps in 4.0

- macOS and Linux builds are untested. The macOS zips are ad-hoc signed, so Gatekeeper prompts on first launch.
- Tool paths cannot be overridden in Settings.
- The egui app is still in the repository (removal was deferred).

## 4.1: quality metrics

The Metrics page becomes real. It measures an encode against its source with FFmpeg's filters. The 4.0 placeholder looks like this:

![Metrics page placeholder](images/metrics-preview.png)

- Metrics: VMAF, PSNR, SSIM and XPSNR.
- One reference compared with many encodes in one run.
- Per-frame graphs you can zoom (not in the first 4.1 build; the per-frame numbers are available as CSV).
- VMAF model chosen automatically from the resolution.
- Analysis of a partial range of the video.
- Export of the worst frames as PNG images.
- CSV export, per frame and as a summary.
- A command log for every analysis.

Also planned for 4.1:

- A type and font rework across the UI, designed in the canvas first.
- A light-theme top bar screenshot for the docs.

## Later

Candidates, not scheduled.

- Tool path overrides.
- Watch folders that process new files automatically.
- Signed and notarised macOS builds, Linux packages, and a signed Windows installer.
- Removal of the legacy egui app.
- A command-line interface that shares `kestrel-core`.
