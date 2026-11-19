# Changelog

## [Unreleased]

### 4.0.0 (merged to main, not tagged)

Kestrel 4.0 is a new desktop app (`kestrel-gpui`, built with GPUI) on a reworked `kestrel-core`. The egui app, the Tauri app and the Python scripts stay in the repository as legacy entry points.

#### Added
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

#### Changed
- Remux jobs stay lossless: audio the container cannot hold is dropped with a warning instead of being converted
- Transcode outputs get unique names when sources share a file stem
- Encoder listing times out after 15 seconds and reports detection failures

#### Known limits
- No tagged release yet; build from source
- macOS and Linux builds are untested
- Parallel jobs, process priority and keep source dates are saved but not applied
- Transcode preview and the Metrics page are placeholders until 4.1

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