# Installation

How to get Kestrel 4.0 running, and which tools it needs. Back to the [README](../README.md).

## Kestrel 4.0

No 4.0 release has been tagged yet. The latest GitHub releases (v3.0.x) ship the earlier egui app, not the 4.0 app. To run 4.0, build it from source. When a 4.0 release is tagged, the release workflow also publishes `kestrel-gpui.exe` (Windows x64) with a `SHA256SUMS.txt`. That executable does not bundle FFmpeg or MKVToolNix, so you install them yourself.

## Tools

| Tool | Needed for |
|---|---|
| FFmpeg and FFprobe | Transcodes, MP4, MOV and WebM output, and probing every file that is not Matroska |
| MKVToolNix (`mkvmerge`) | Optional engine for MKV to MKV remuxes |

Install them with your package manager or from the project sites:

- Windows: an FFmpeg build such as the ones on <https://www.gyan.dev/ffmpeg/builds/>, and MKVToolNix from <https://mkvtoolnix.download>.
- macOS: `brew install ffmpeg mkvtoolnix`.
- Ubuntu or Debian: `sudo apt install ffmpeg mkvtoolnix`.

FFmpeg builds differ in which encoders they include. Use a full build if you want x265, SVT-AV1, libaom, VP9 or hardware encoders. See [Troubleshooting](guides/troubleshooting.md#an-encoder-is-missing-or-unavailable).

### Where Kestrel looks

For each of `ffmpeg`, `ffprobe` and `mkvmerge`:

1. Next to the Kestrel executable, then in a `tools` folder beside it, then one and two folder levels below `tools`.
2. Every folder on `PATH`.
3. Platform defaults:
   - Windows: `C:\Program Files\MKVToolNix` (mkvmerge only).
   - macOS: `/Applications/MKVToolNix-*.app` (mkvmerge only), then `/opt/homebrew/bin` and `/usr/local/bin`.
   - Linux: `/usr/bin` and `/usr/local/bin`.

Settings shows what it found. Restart Kestrel, or press **Re-detect tools**, after you install something. Verify from a terminal:

```sh
ffmpeg -version
ffprobe -version
mkvmerge --version
```

## Build from source

Requirements: Git, Rust 1.92 or newer (install with rustup), and the platform toolchain below.

### Windows 11

- The MSVC Rust toolchain (`stable-x86_64-pc-windows-msvc`).
- Visual Studio C++ build tools and the Windows SDK. GPUI's build script needs the SDK's `fxc.exe` (release shaders) and `rc.exe` (the default Windows manifest).

```sh
git clone https://github.com/mlbkumar9/Kestrel.git
cd Kestrel
cargo build --release -p kestrel-gpui
target\release\kestrel-gpui.exe
```

### Linux

Install `libxkbcommon-dev`, `libwayland-dev`, `libxkbcommon-x11-dev`, `libfontconfig1-dev` and `mesa-vulkan-drivers`. Run in a graphical session with a user-owned `XDG_RUNTIME_DIR` (mode 0700) and `DISPLAY` set. Without a GPU, set `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json`.

```sh
cargo run --release -p kestrel-gpui
```

You cannot cross-build the Windows GPUI binary from Linux: the build fails with a missing `shaders_bytes.rs`, because the shader compiler is a Windows tool. Kestrel uses upstream GPUI, not a fork. Linux builds are untested for 4.0.

### macOS

Untested for 4.0.

### Opening files from the command line

```sh
kestrel-gpui movie.mkv other.mp4
```

### Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Earlier releases (3.x)

The v3.0.x releases on <https://github.com/mlbkumar9/Kestrel/releases> ship the egui app:

- **Windows MSI**: installs Kestrel with bundled FFmpeg and MKVToolNix for all users. It may ask for administrator approval.
- **Windows ZIP**: extract the whole archive and keep the `tools` folder beside `kestrel.exe`.
- **Windows EXE**: install FFmpeg and MKVToolNix separately.
- **macOS and Linux**: binaries that use tools installed on the system.

See [Legacy tools](LEGACY.md) for the egui app, the Tauri app and the Python scripts.
