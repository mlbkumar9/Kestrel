# Installation

How to get Kestrel 4.0 running, and which tools it needs. Back to the [README](../README.md).

## Kestrel 4.0

Download Kestrel 4.1 from the [v4.1.1 release](https://github.com/mlbkumar9/Kestrel/releases/tag/v4.1.1), or build it from source (below). Each tag publishes these files:

| File | What it is |
|---|---|
| `Kestrel-v4.1.1-windows-x64.zip` | Unzip and run `Kestrel.exe`. Bundles FFmpeg and mkvmerge in `tools`. |
| `Kestrel-v4.1.1-windows-x64.msi` | Installer with the same bundled tools. |
| `Kestrel-v4.1.1-windows-x64.exe` | The program alone, without tools. |
| `Kestrel-v4.1.1-macos-aarch64.zip`, `-macos-x86_64.zip` | Ad-hoc signed, so Gatekeeper prompts on first launch. FFmpeg and MKVToolNix come from the system. |
| `Kestrel-v4.1.1-linux-x64.tar.gz` | FFmpeg and MKVToolNix come from the system. |
| `Kestrel-v4.1.1-legacy-egui-windows-x64.zip` | The earlier egui app. |
| `SHA256SUMS.txt` | Checksums for every file above. |

The Windows zip and MSI need no separate FFmpeg or MKVToolNix install. The other downloads need them on `PATH`.

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

Building from source is untested for 4.0. Use the release zip.

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

## Installer upgrade test

The `msi-upgrade` job in `.github/workflows/ci.yml` runs `scripts/test-msi-upgrade.ps1` on a `windows-latest` runner. It installs the published v4.0.0 MSI, writes a settings file and a user preset to `%APPDATA%\kestrel\gpui.ron`, installs the MSI built from the current commit over it, and checks that:

- Programs and Features lists exactly one Kestrel, at the new version;
- `Kestrel.exe` has the new file and product version;
- the Start menu shortcut targets the installed `Kestrel.exe`;
- the settings and preset are unchanged;
- `ffmpeg.exe` and `mkvmerge.exe` are installed.

The script installs and upgrades Kestrel for real, so run it only on a throwaway machine. The MSI UpgradeCode stays `{E110F64D-5C3D-5E2D-9B02-53217F2CAEA5}`.

## Earlier releases (3.x)

The v3.0.x releases on <https://github.com/mlbkumar9/Kestrel/releases> ship the egui app:

- **Windows MSI**: installs Kestrel with bundled FFmpeg and MKVToolNix for all users. It may ask for administrator approval.
- **Windows ZIP**: extract the whole archive and keep the `tools` folder beside `kestrel.exe`.
- **Windows EXE**: install FFmpeg and MKVToolNix separately.
- **macOS and Linux**: binaries that use tools installed on the system.

See [Legacy tools](LEGACY.md) for the egui app and the Python scripts.
