# Troubleshooting

This guide lists the problems you are most likely to hit and what to do about each. Back to the [README](../../README.md).

Start with **Activity** (`Ctrl 5`). It shows the command Kestrel ran and the message the tool returned.

## Tools are not found

**Symptom.** Settings shows `missing` for FFmpeg, FFprobe or mkvmerge, or a job fails with "FFmpeg is unavailable" or "MKVToolNix is unavailable".

1. Run `ffmpeg -version`, `ffprobe -version` and `mkvmerge --version` in a terminal. If they fail there, install them first.
2. Make sure the folder holding them is on `PATH` for the account that starts Kestrel. A terminal opened before you changed `PATH` has the old value, and so does an app launched from it.
3. Or copy the executables next to the Kestrel executable, or into a `tools` folder beside it.
4. Restart Kestrel, or press **Re-detect tools** on Settings.

The search order is in [Settings](settings.md#tools). The 4.0 interface cannot point to a tool at a custom path. The Windows packages of the 3.x line bundle the tools, but 4.0 does not.

## A file will not load or shows no tracks

Kestrel needs FFprobe for any file that is not Matroska. Check that Settings lists FFprobe. If a file lists no duration or tracks, run `ffprobe` on it by hand. A truncated or damaged file fails there too.

## "Video must be re-encoded for MP4" (or MOV, WebM)

A remux copies video, so the codec has to fit the container. VP9 fits MP4 and WebM, but H.264 does not fit WebM. Choose MKV, or use Transcode and pick an encoder the container takes. See the [container table](remux.md#output-container).

## Audio or subtitle tracks were dropped

Kestrel warns about this before the job starts, under the container menu on Remux.

- **Audio dropped.** A remux never converts audio. DTS, TrueHD and similar tracks do not fit MP4, MOV or WebM. Pick MKV, or use Transcode, which converts audio the container cannot hold (to AAC, or Opus for WebM).
- **All audio dropped.** The job fails with "... audio cannot be stored in MP4; choose MKV or use Transcode".
- **Picture subtitles dropped.** PGS, VobSub and similar formats go into MKV only. Choose MKV, or burn the subtitle into the video with Transcode.

### MP4 and WebM limits at a glance

| | MP4 | MOV | WebM |
|---|---|---|---|
| Picture subtitles | no | no | no |
| Text subtitles | converted to `mov_text` | converted to `mov_text` | converted to WebVTT |
| DTS or TrueHD audio | no | no | no |
| H.264 or HEVC video | yes | yes | no |

## An encoder is missing or unavailable

**Symptom.** The Video tab shows "(not available)" next to an encoder, or a job fails with "Video encoder ... is unavailable".

Kestrel lists only encoders that your FFmpeg build reports. Builds differ. For example `libx265`, `libsvtav1`, `libaom-av1` and `libvpx-vp9` may be absent from a minimal build.

- Check with `ffmpeg -hide_banner -encoders`.
- Install a full build of FFmpeg (a build with the GPL libraries) and press **Re-detect** on Settings.
- A job that asks for an unavailable encoder uses an available encoder of the same codec when one exists (software first). If none exists, it reports the missing encoder and does not run.
- **HE-AAC** needs an FFmpeg built with `libfdk_aac`. It is listed only when found.

If detection itself fails, Settings shows a red message under Hardware encoders. A slow disk or a locked-down FFmpeg can hit the 15 second limit on `ffmpeg -encoders`.

## A hardware encoder does not show up

Kestrel lists a hardware encoder only after a test encode of one small frame succeeds.

1. Confirm that FFmpeg lists it: `ffmpeg -hide_banner -encoders` should show `h264_nvenc`, `hevc_qsv`, `hevc_amf` or similar.
2. Install or update the graphics driver. NVENC needs an NVIDIA GPU with a current driver. QSV needs an Intel GPU. AMF needs an AMD GPU. VAAPI is Linux only. VideoToolbox is macOS only.
3. Run the test yourself: `ffmpeg -f lavfi -i color=black:s=256x256:d=0.1 -frames:v 1 -c:v hevc_nvenc -f null -` (replace the encoder). Any error it prints is the reason.
4. Press **Re-detect** on Settings.

Hardware encoders do not support two-pass, advanced options, or fast decode in Kestrel, and they use their own quality number (CQ). Use a lower value than you would with x265.

## Subtitles show wrong characters

Text subtitle files from older sources are often not UTF-8. Accents or kana show as garbage.

- In Transcode, set **Import character set** on the Subtitles tab to Windows-1252, ISO-8859-1 or Shift_JIS, then encode.
- In Remux, pick the file's encoding in the **Charset** menu under the added subtitle row. It applies to text subtitles only.

## A subtitle is out of sync

Remux and Transcode both have an **Offset (ms)** field for an added subtitle. A positive value delays the subtitle, and a negative one advances it.

## Two-pass or advanced options are greyed out or warned about

Two-pass needs **Average bitrate** and a software encoder. Advanced options exist for x264, x265 and SVT-AV1. Fast decode exists for the same three. Kestrel shows the reason as a warning, and turns the control off for other encoders.

## 12-bit, HDR or tone-mapping warnings

- "12-bit output is only available with x265": choose x265 or a lower bit depth.
- "8-bit output of HDR video will show banding": pick a 10-bit encoder, or tone-map to SDR.
- "The BT.2390 tone-map operator needs FFmpeg's libplacebo filter and a Vulkan device": pick Hable, Mobius or Reinhard.

## The output already exists

On Remux, **If the output exists** chooses between Rename (adds `(2)`), Skip and Overwrite. A skipped job shows Skipped on the Queue. A queued job keeps the policy it had when you queued it, so change the policy on Remux and queue the job again.

## The Preview fails

The preview is a short encode, so it needs the same things a transcode needs: a working FFmpeg and an available encoder. Read the error under the **Encode preview** button, then check Settings. If the source is shorter than the chosen length, Kestrel shortens the segment. See [Transcode](transcode.md#preview).

## Metrics fails or shows no scores

Read the message on the page, then open **FFmpeg commands** or Activity for the exact command. "This FFmpeg build has no libvmaf filter" means the FFmpeg Kestrel found lacks that filter: use the bundled build or a full build. A file that is not video, or an encode with no video stream, fails on its own row while the others still run. A very low score for a good encode usually means the two files are not the same cut or frame rate. See [Usage](../USAGE.md#metrics).

## Settings or window do not behave

Run Kestrel from a terminal. GPUI prints startup errors to it, for example if the window cannot open. On Linux without a GPU, see the software rendering variables in `AGENTS.md`.

## Still stuck

Copy the relevant lines from Activity (**Copy** or **Export...**) and open an issue at <https://github.com/mlbkumar9/Kestrel/issues>.
