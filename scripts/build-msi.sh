#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "Usage: $0 <stage-dir> <version> <out.msi>" >&2
  exit 2
fi

stage_dir="$(cd "$1" && pwd)"
version="$2"
output_msi="$3"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Version must be in major.minor.patch form: $version" >&2
  exit 2
fi

for file in \
  langmux.exe \
  LICENSE \
  README.md \
  THIRD_PARTY_NOTICES.md \
  tools/ffmpeg/bin/ffmpeg.exe \
  tools/ffmpeg/bin/ffprobe.exe \
  tools/ffmpeg/LICENSE \
  tools/mkvtoolnix/mkvmerge.exe \
  tools/mkvtoolnix/COPYING.txt; do
  if [[ ! -f "$stage_dir/$file" ]]; then
    echo "Staging file is missing: $stage_dir/$file" >&2
    exit 1
  fi
done

if [[ ! -f "$stage_dir/logo.ico" ]]; then
  cp "$repo_root/logo.ico" "$stage_dir/logo.ico"
fi

output_directory="$(dirname "$output_msi")"
mkdir -p "$output_directory"
output_msi="$(cd "$output_directory" && pwd)/$(basename "$output_msi")"
stage_relative="$(realpath --relative-to="$repo_root/packaging/windows" "$stage_dir")"

if ! command -v wixl >/dev/null 2>&1; then
  echo "wixl is required; install msitools." >&2
  exit 1
fi

wixl -a x64 \
  -D "StageDir=$stage_relative" \
  -D "Version=$version" \
  -o "$output_msi" \
  "$repo_root/packaging/windows/langmux.wxs"
