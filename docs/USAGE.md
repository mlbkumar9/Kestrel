# Usage

Launch the native egui app (`kestrel.exe` on Windows, or
`cargo run --release -p kestrel-media` from the repository).

## 1. Source

Add files or folders using the Source page, or drag files onto the window.
Choose whether folders include subfolders. Kestrel scans supported media files
and probes metadata in the background. Select the sources to process; removing
a source does not delete the original file.

## 2. Settings

Choose a preset: **Change tracks only**, **Same quality, smaller file** (HEVC), **Play anywhere** (H.264),
**Archive (AV1)** or **Web (WebM)**. A hardware preset appears when a supported
HEVC hardware encoder is detected. You can also adjust settings and save user presets.

- **Summary:** choose the output container and destination. Defaults use a
  `converted` directory. Choose how to handle existing outputs.
- **Video:** copy the video or select an encoder, quality, speed and filters.
- **Audio:** choose language tracks, copy or encode audio, and adjust mixdown/bitrate.
- **Subtitles:** choose languages, include/omit subtitles, or burn a selected track
  into re-encoded video. Picture subtitles may require MKV or burn-in.
- **Metadata:** control chapters, metadata and title.

Audio and subtitle language choices are independent. Empty means all;
`eng` is English, `jpn` Japanese, `spa` Spanish and `und` undefined.
If no requested audio language matches, Kestrel retains all audio tracks.

The remux engine setting chooses MKVToolNix or FFmpeg. MKVToolNix is used for
eligible Matroska stream-copy jobs; jobs needing conversion use FFmpeg.
Transcoding requires FFmpeg; non-MKV probing requires FFprobe.

## 3. Queue

Add the selected sources to the queue. Each job stores a snapshot of its settings,
so later changes do not alter jobs already queued. Start the queue, pause between
jobs, stop the active process, reorder pending work or retry failed jobs.
Review warnings when changing containers: some tracks must be converted or dropped.

Open **Logs** for commands and diagnostics. **Dry run** previews the planned
commands without writing final media outputs; it does not verify a full encode.
Use **More** to import/export user presets as JSON.

Completed outputs remain when the queue is stopped. Kestrel uses temporary
outputs during processing and removes them on failure or cancellation.