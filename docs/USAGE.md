# Usage

A short map of Kestrel 4.0. Each section links to the guide with the details. Back to the [README](../README.md).

## Workflow

1. **Library** (`Ctrl 1`): add files and folders with drag and drop, `Ctrl O`, **Add files** or **Add folder**. Kestrel probes each file in the background. Tick the files you want to process.
2. **Remux** (`Ctrl 2`) to change tracks without re-encoding, or **Transcode** (`Ctrl 3`) to re-encode.
3. **Queue** (`Ctrl 4`) to start, pause or stop jobs. **Activity** (`Ctrl 5`) to read commands and errors.
4. **Metrics** (`Ctrl 6`) to score finished encodes against their source.

`Ctrl K` opens the command palette for actions, pages and presets. `Ctrl Enter` queues the current job.

## Metrics

The Metrics page measures how close an encode is to its source, frame by frame, with FFmpeg's `libvmaf`, `psnr`, `ssim` and `xpsnr` filters. It needs an FFmpeg build that has those filters; the build Kestrel bundles on Windows does.

1. Choose the **Reference (source)** with **Browse...**, then **Add files...** for one or more encodes. Both sides can have a different size: Kestrel scales each encode to the reference with bicubic scaling before measuring.
2. Tick the metrics to run (VMAF, PSNR, SSIM, XPSNR; all on by default) and pick the **VMAF model**. **Auto** uses `vmaf_4k_v0.6.1` when the reference is 2160p or 3840 wide or more, and `vmaf_v0.6.1` otherwise.
3. To measure only part of the clips, enter **Range start (s)** and **Range length (s)**. Leave them empty for the whole clip.
4. Press **Run**. Each file shows its own progress bar, and **Cancel** stops the run.
5. The table lists VMAF (mean, minimum and 1% low), PSNR, SSIM and XPSNR per file. PSNR and XPSNR are in dB, with identical frames held at 100 dB. XPSNR combines the Y, U and V planes as (4Y + U + V) / 6 in the error domain, because FFmpeg logs the planes separately.
6. **Export per-frame CSV** writes one file per encode into a folder you choose, **Export summary CSV** one row per encode, and **Export worst frames (PNG)** the lowest-VMAF frames (lowest PSNR when VMAF is off) as `<name>_frame<N>.png`. **Worst frames** sets how many. Frame numbers start at 0 and count from the start of the range.
7. **FFmpeg commands** shows every command that ran. The same lines go to Activity.

Reference and encode should have the same frame rate and cover the same part of the video. Kestrel resets both timelines to start at zero and lines up frames by timestamp, so an encode with extra leading frames or a different cut scores low.

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
