# Changelog

## [3.0.0] - 2026-11-02

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