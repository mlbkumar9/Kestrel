# Installation

Kestrel's working desktop application uses egui. The GPUI application in this
repository is a shell under development, not a replacement for the media workflow yet.

## Windows x64

Download a release from [GitHub Releases](https://github.com/mlbkumar9/Kestrel/releases):

- **MSI:** installs Kestrel and bundled FFmpeg/FFprobe and MKVToolNix for all users.
  Installation may request administrator approval.
- **Portable ZIP:** extract the entire archive, keeping the `tools` directory
  next to `kestrel.exe`, then run the executable.
- **Standalone EXE:** install FFmpeg/FFprobe and MKVToolNix separately and make
  their executables discoverable on `PATH` or in the supported installation paths.

## Linux and macOS

Download and extract the binary for your platform and architecture, or build
from source below. Install the external tools using your system package manager.

Ubuntu/Debian:
```sh
sudo apt install ffmpeg mkvtoolnix
```

macOS with Homebrew:
```sh
brew install ffmpeg mkvtoolnix
```

Launch `kestrel` from the extracted directory. On Linux you may need to mark it
executable with `chmod +x kestrel`. A graphical desktop session is required.

## Build from source

Install Rust with rustup (Rust 1.92 or newer), Git and your platform's native
compiler/linker. Linux GUI development packages are listed in the CI workflow;
the GPUI shell's additional requirements are documented in `AGENTS.md`.

```sh
git clone https://github.com/mlbkumar9/Kestrel.git
cd Kestrel
cargo build --release -p kestrel-media
cargo run --release -p kestrel-media
```

The binary is `target/release/kestrel` (`kestrel.exe` on Windows).
Source builds do not bundle external media tools. Building the egui app does
not require building the separate GPUI shell.

## Verify tools

```sh
mkvmerge --version
ffmpeg -version
ffprobe -version
```

Kestrel searches executable-adjacent bundled tools, then `PATH`, then platform
defaults. Restart the app after installing tools. FFmpeg builds need the
encoders used by your jobs; see [FAQ](FAQ.md) for fallback behavior.