# Third-party tool notices

The Windows release bundle includes the following third-party command-line tools. Their complete license texts are included alongside the binaries.

## FFmpeg

- Version: `9.1` (gyan.dev full build, `9.1-full_build-www.gyan.dev`)
- Source project: <https://ffmpeg.org/>; source for this release: <https://github.com/FFmpeg/FFmpeg/tree/n9.1>
- Build source and pinned Windows asset: <https://github.com/GyanD/codexffmpeg/releases/download/9.1/ffmpeg-9.1-full_build.7z> (SHA-256 `1a86f2c8b51b4990ae9a0b0f8bd4f345f5c79389d293ac3fda128a58ec507b16`)
- License: GPL v3; see `tools/ffmpeg/LICENSE` in the Windows package.

## MKVToolNix

- Version: `103.0`
- Source project: <https://mkvtoolnix.download/>
- Pinned Windows archive: <https://mkvtoolnix.download/windows/releases/103.0/mkvtoolnix-64-bit-103.0.7z> (SHA-256 `01d76dbfdbd5615d9e02f69b0710d1429a902d47a98c8eee33bd884c4fcb3cbf`)
- License: GPL-2.0; see `tools/mkvtoolnix/COPYING.txt` in the Windows package.

Kestrel does not modify these tools. The bundled copies are redistributed with their accompanying license texts and the source URLs above.

## Inter (bundled font)

- Version: `4.1` (static Regular, Medium, SemiBold and Bold faces from `extras/ttf/`), embedded in the Kestrel binary
- Source project: <https://rsms.me/inter/>; release: <https://github.com/rsms/inter/releases/tag/v4.1>
- Release archive: <https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip> (SHA-256 `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`)
- License: SIL Open Font License 1.1; the text is in `crates/kestrel-gpui/assets/Inter-LICENSE.txt`.
