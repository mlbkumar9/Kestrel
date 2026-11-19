# Frequently asked questions

Answers about Kestrel 4.0. Back to the [README](../README.md).

## Is there a 4.0 download?
Yes. Download it from the [v4.0.0 release](https://github.com/mlbkumar9/Kestrel/releases/tag/v4.0.0). The README says [which file to pick](../README.md#download). You can also build from source: see [Installation](INSTALLATION.md).

## Do I need Python?
No. Kestrel 4.0 is a Rust app. The Python scripts are legacy tools.

## Which tools do I need?
FFmpeg and FFprobe for transcodes and for any file that is not Matroska. MKVToolNix (`mkvmerge`) is optional and used for MKV to MKV remuxes. Settings shows what Kestrel found.

## Where does Kestrel look for FFmpeg and MKVToolNix?
Next to the executable (and in a `tools` folder beside it), then on `PATH`, then in platform defaults. The full order is in [Settings](guides/settings.md#tools). You cannot enter a custom path in 4.0.

## Does a remux change quality?
No. A remux copies the video and keeps audio and subtitle streams as they are. The exception is text subtitles, which convert to the container's text format when needed. A remux never converts audio. A track the container cannot hold is dropped, and Kestrel warns you first.

## What formats can I use?
Input: `mkv`, `mk3d`, `webm`, `mp4`, `m4v`, `mov`, `avi`, `ts`, `m2ts`, `mts`, `wmv`, `flv`, `mpg`, `mpeg`, `vob`, `3gp` and `ogv`. Output: MKV, MP4, MOV and WebM. What each container holds is in the [README](../README.md#supported-formats).

## Can I add subtitles or audio from a separate file?
Yes. On Remux, use **Add subtitle file** or **Add audio file**. On Transcode, import subtitles and optionally burn them in. See [Adding subtitles and audio](guides/adding-subtitles.md).

## Can I change the track order?
Yes, on the Remux page. Each source and added row has up and down buttons in the Order column. See [Remux](guides/remux.md#the-track-editor).

## How are languages selected?
Audio and subtitles are separate. Click the language chips on Remux, or enter codes on the Transcode Audio tab. No selection keeps everything. If you select audio languages and none match a file, Kestrel keeps all audio tracks of that file and warns. Tracks without a tag count as `und`.

## Why is an encoder unavailable?
FFmpeg builds differ, and hardware encoders also need a working device and driver. Kestrel lists only encoders that FFmpeg reports, and hardware encoders must also pass a one-frame test encode. See [Troubleshooting](guides/troubleshooting.md#an-encoder-is-missing-or-unavailable).

## What quality value should I use?
For x264 and x265, CRF 18 to 24 covers most HD sources, and the default is 22. For AV1 and VP9, 25 to 34. See [Choosing a quality value](guides/transcode.md#choosing-a-quality-value).

## Can Kestrel measure VMAF, PSNR or SSIM?
Not in 4.0. The Metrics page is a placeholder. Quality metrics are the main feature of 4.1. See the [roadmap](ROADMAP.md).

## What do Parallel jobs and Process priority in Settings do?
Parallel jobs sets how many queue jobs run at once, from 1 to 4. Process priority sets the scheduling priority of every FFmpeg, FFprobe and mkvmerge process Kestrel starts (Normal, Below normal or Low). Both are applied. See [Settings](guides/settings.md#behaviour).

## Where should I look when a job fails?
Open Activity (`Ctrl 5`). It shows the command and the tool's error. Check that the tools exist, that the output folder is writable, that there is free space and that the container fits the tracks. Stop cancels the running job and removes its temporary file. Finished outputs stay.

## Does Kestrel upload my files?
No. FFmpeg and mkvmerge run on your machine. Kestrel does not send media or metadata anywhere.

## Is HandBrake code used?
No. Kestrel follows HandBrake's control names and built-in preset values so the Transcode tabs feel familiar. All code and FFmpeg arguments are Kestrel's own. HandBrake is GPL and Kestrel is MIT.
