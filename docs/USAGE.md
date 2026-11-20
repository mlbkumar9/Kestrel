# Usage

A short map of Kestrel 4.0. Each section links to the guide with the details. Back to the [README](../README.md).

## Workflow

1. **Library** (`Ctrl 1`): add files and folders with drag and drop, `Ctrl O`, **Add files** or **Add folder**. Kestrel probes each file in the background. Tick the files you want to process.
2. **Remux** (`Ctrl 2`) to change tracks without re-encoding, or **Transcode** (`Ctrl 3`) to re-encode.
3. **Queue** (`Ctrl 4`) to start, pause or stop jobs. **Activity** (`Ctrl 5`) to read commands and errors.

`Ctrl K` opens the command palette for actions, pages and presets. `Ctrl Enter` queues the current job.

## Guides

| Task | Guide |
|---|---|
| First run | [Getting started](guides/getting-started.md) |
| Keep or drop languages, edit track names and flags, change container | [Remux and the track editor](guides/remux.md) |
| Add an SRT, ASS or audio file, burn subtitles in | [Adding subtitles and audio](guides/adding-subtitles.md) |
| Re-encode video, filters, HDR, audio tracks, chapters, tags | [Transcode, tab by tab](guides/transcode.md) |
| Built-in and saved presets | [Presets](guides/presets.md) |
| Run jobs and read the log | [Queue and Activity](guides/queue-and-activity.md) |
| Tools, hardware encoders, appearance | [Settings](guides/settings.md) |
| Something went wrong | [Troubleshooting](guides/troubleshooting.md) |

## Tools

Kestrel drives FFmpeg and FFprobe for transcodes and for every file that is not Matroska, and MKVToolNix for MKV to MKV remuxes. See [Installation](INSTALLATION.md) for where Kestrel looks for them.

## Older interfaces

The egui app and the Python scripts are described in [Legacy tools](LEGACY.md).
