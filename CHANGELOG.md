# Changelog

## [Unreleased]

### Added
- CI installs the published 4.0.0 MSI, writes settings and a user preset, upgrades it with the MSI built from the current commit, and checks the result (`scripts/test-msi-upgrade.ps1`)

### Changed
- Inter 4.1 is now bundled (Regular, Medium, SemiBold, Bold), so weights render from real faces instead of synthesized ones
- Metadata chips and numbers use Inter with tabular figures, so digits line up
- Monospace (JetBrains Mono) is used only for command lines and logs

### Fixed
- The video codec column in the Library keeps enough width for labels such as `HEVC 10b 3840x2160 HDR` and shows the full label in a tooltip
- The Destination field (Remux) and Folder field (Transcode) show a long path as `start...end` while they are not focused, with the full path in a tooltip

## [4.0.2] - 2026-11-21

### Changed
- The Windows zip and installer bundle FFmpeg 9.1 (gyan.dev full build) instead of FFmpeg 9.0.2 (BtbN), and MKVToolNix 103.0 instead of 99.0
- Windows packaging checks the SHA-256 of both tool downloads before bundling them
- The Windows zip and installer are about 50 MB larger, because the gyan.dev build of FFmpeg includes more libraries

## [4.0.1] - 2026-11-20

### Fixed
- The Windows installer (.msi) no longer fails with error 2229; the 4.0.0 installer was built with wixl 0.106, whose Upgrade table Windows Installer cannot load

### Removed
- The legacy Tauri + React app (`REMUX_Tauri/`); no release built it, and its dependencies carried most of the repository's open security alerts

## [4.0.0] - 2026-11-19

Kestrel 4.0 is a new desktop app (`kestrel-gpui`, built with GPUI) on a reworked `kestrel-core`. The egui app, the Tauri app and the Python scripts stay in the repository as legacy entry points.

### Added
- GPUI desktop app with Library, Remux, Transcode, Queue, Activity, Metrics and Settings pages, dark, light and system themes, a command palette (`Ctrl K`) and shortcuts (`Ctrl O`, `Ctrl Enter`, `Ctrl 1` to `Ctrl 6`)
- Remux track editor: keep or drop tracks, edit language, title, default and forced flags per file, with language chips for batches
- External subtitle (SRT, ASS, SSA, VTT, SUP) and audio files added to a remux, with language guessed from the file name; timing offset and character set support in both engines
- Container compatibility checks for MKV, MP4, MOV and WebM with warnings before tracks are dropped
- Transcode controls modelled on HandBrake: Summary, Dimensions, Filters, Video, Audio, Subtitles, Chapters and Tags tabs
- Video encoders x264, x265 (8, 10 and 12-bit), SVT-AV1, libaom, VP9 and hardware NVENC, QSV, AMF, VideoToolbox and VAAPI, detected from the installed FFmpeg with a test encode for hardware
- Constant quality and average bitrate rate control, two-pass encoding, encoder presets, tune, profile, level, fast decode and advanced options
- Crop (automatic, conservative, custom), resolution limit, anamorphic, modulus, borders, rotation and flip
- Detelecine, deinterlace (Yadif, Bwdif, Decomb), denoise (NLMeans, HQDN3D), chroma smooth, sharpen, deblock, colourspace conversion and grayscale
- HDR10 pass-through with x265 and tone-mapping to SDR
- Per-track audio settings with passthru rules, mixdown, sample rate, bitrate, gain and DRC
- Subtitle import, burn-in behaviours (including a foreign audio approximation), default track choice and forced-only filtering
- Chapter renaming with CSV import and export, and descriptive tags
- Queue with job snapshots, pause, stop, retry and reveal output; Activity log with copy and export
- Built-in presets (Change tracks only, Same quality smaller file, Play anywhere, Archive AV1, Web WebM, and a hardware HEVC preset when available) and user presets
- Size estimate from sample encodes
- Transcode preview: a 15, 30 or 60 second segment encoded with the current settings, with its size, bitrate, extrapolated full size, a Play preview button and source against encoded stills
- Remux page: per added file Offset (ms), and a Charset menu with 11 charsets for text subtitles
- Remux page: up and down buttons to reorder source and added tracks
- Added audio and subtitle files are probed with ffprobe, so `.mka` and `.thd` files show their real codec
- Presets: rename, duplicate, delete (with confirmation), and import and export as JSON from the sidebar preset menu and the Save as area; built-in presets stay read-only
- Top bar: sidebar toggle (`Ctrl B`), back and forward (`Alt Left`, `Alt Right`, mouse buttons 4 and 5), breadcrumb, a search field that opens the command palette (`Ctrl K`), a queue pill with progress, file, percentage, ETA and pause (done or failed for 10 seconds afterwards), an activity bell with an unread badge for warnings and errors, a theme toggle, Add media, Start queue and Pause, and themed window controls
- Release packaging: per tag a Windows zip with bundled FFmpeg and mkvmerge, a Windows MSI, a standalone Windows exe, macOS zips for Apple silicon and Intel (ad-hoc signed), a Linux tar.gz, the legacy egui zip, and `SHA256SUMS.txt`
### Changed
- Remux jobs stay lossless: audio the container cannot hold is dropped with a warning instead of being converted
- Transcode outputs get unique names when sources share a file stem
- Encoder listing times out after 15 seconds and reports detection failures
- Settings Parallel jobs, Process priority and Keep the source file's dates are applied to the queue and the tools

### Known limits
- macOS and Linux builds are untested; the macOS zips are ad-hoc signed, so Gatekeeper prompts on first launch
- The Metrics page is a placeholder until 4.1
- The Library codec label can be truncated, and the Folder and Destination field can be clipped

## [3.0.2] - 2026-11-03

### Fixed
- The Windows installer (.msi) no longer fails with error 2229; the 3.0.0 and 3.0.1 installers could not be installed
- Upgrading from the LangMux 1.0.0 installer (.msi) no longer leaves its Start Menu, desktop and uninstall shortcuts behind
- The banner offering to remove a LangMux 1.0.0 setup.exe installation now disappears once the removal has finished, and the removal also clears the registry key that installer left behind

## [3.0.1] - 2026-11-02

### Fixed
- Adding a parent folder without subfolders no longer brings back files you removed from a subfolder source
- The Windows executable now shows the Kestrel icon in Explorer and the Start Menu

## [3.0.0] - 2026-11-02

LangMux is now Kestrel.

### Added
- Multi-step native workflow for source selection, conversion settings, and queue management
- Configurable video, audio, and subtitle processing with presets and per-job snapshots
- Frame preview, output-size estimates, and detailed queue progress reporting
- Windows installer upgrade support for earlier MSI releases and in-app cleanup of older per-user installs

## [2.0.0] - 2026-10-30

### Added
- Native Rust (eframe/egui) desktop app with a refined dark interface
- FFmpeg stream-copy remux engine alongside MKVToolNix
- FFprobe metadata fallback when MKVToolNix identification is unavailable
- Windows release bundles with FFmpeg and MKVToolNix command-line tools
- Cross-platform Linux, macOS, and Windows builds and GitHub releases

## [1.0.0] - 2025-10-18

### Added
- GUI version
- CLI version with Rich library
- Batch processing
- Language selection
- Auto-detect
- Dry run mode
- Real-time progress tracking
- Cross-platform support

### Technical Details
- Python 3.7+ support
- MKVToolNix integration
- Threading support

### Dependencies
- Rich library
- Tkinter

### Documentation Updates
- Improved documentation for better understanding

### Supported Platforms
- Windows
- macOS
- Linux

### Planned Future Enhancements
- Multi-threading support
- Presets for common configurations
- Standalone executables
- Support for additional formats
- Web interface for easier access
- Detailed logging features
- Track selection capabilities
- Codec conversion functionality