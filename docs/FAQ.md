# Frequently asked questions

## Do I need Python?
No. The native Kestrel desktop app is a Rust application with an egui interface.
The separate legacy Python entrypoints are not required.

## Which tools do I need?
MKVToolNix (`mkvmerge`) handles Matroska stream-copy remuxing. FFmpeg handles
transcoding and other remux operations; FFprobe reads source metadata and is the
fallback when MKVToolNix identification fails. Install FFmpeg/FFprobe for non-MKV
sources and transcoding. Windows MSI and portable ZIP releases bundle these
tools; the standalone EXE and Linux/macOS binaries use separately installed tools.

## Does remuxing change quality?
The Remux preset copies streams without re-encoding video. Container
compatibility can require audio/subtitle conversion or subtitle removal.
Review warnings before running. Encoding presets re-encode video and can change quality.

## What formats can I use?
Inputs include MKV, WebM, MP4, MOV, AVI, MPEG and transport-stream files.
Output containers are MKV, MP4, MOV and WebM. Not every codec fits every container;
choose MKV to retain more track types, or encode to a compatible format.

## How are languages selected?
Audio and subtitle selections are separate. Codes such as `eng`, `jpn` and
`spa` come from the source tags; `und` means undefined. An empty selection
keeps all tracks of that kind. If no requested audio language matches, all audio
tracks are retained. Disable subtitles in Settings to omit them entirely.

## Why is an encoder unavailable?
FFmpeg builds differ, and hardware encoders also need a working device/driver.
Kestrel detects encoders at startup. An unavailable video encoder falls back to
an available encoder for the same codec, preferring software, or the job reports
the missing encoder without starting the encode. AV1 supports SVT-AV1 and libaom.

## Where should I look when a job fails?
Open Logs and inspect the command and diagnostic. Check tool availability,
output permissions, free space and container compatibility. Existing outputs can
be skipped, overwritten or given a new name. Stop cancels the active process
and removes its temporary output; completed outputs remain.