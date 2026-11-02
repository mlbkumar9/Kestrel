#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 5 ]]; then
  echo "Usage: $0 <tag> <platform> <arch> <binary> <output-directory>" >&2
  exit 2
fi

tag="$1"
platform="$2"
arch="$3"
binary="$4"
output_directory="$5"

if [[ ! "$tag" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "Tag contains unsupported characters: $tag" >&2
  exit 2
fi
if [[ "$platform" != "macos" && "$platform" != "linux" ]]; then
  echo "Unsupported platform: $platform" >&2
  exit 2
fi
if [[ ! -f "$binary" ]]; then
  echo "Release binary not found: $binary" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$output_directory"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

cp "$binary" "$stage/kestrel"
cp "$repo_root/README.md" "$stage/README.md"
cp "$repo_root/LICENSE" "$stage/LICENSE"
if [[ "$platform" == "linux" ]]; then
  mkdir -p "$stage/share/applications" "$stage/share/icons/hicolor/512x512/apps"
  cp "$repo_root/packaging/linux/kestrel.desktop" "$stage/share/applications/kestrel.desktop"
  cp "$repo_root/LangMux_Native/assets/kestrel.png" "$stage/share/icons/hicolor/512x512/apps/kestrel.png"
fi

if [[ "$platform" == "linux" && "$arch" == "x64" ]]; then
  archive="$output_directory/Kestrel-$tag-linux-x64.tar.gz"
else
  archive="$output_directory/Kestrel-$tag-$platform-$arch.tar.gz"
fi
tar -czf "$archive" -C "$stage" .
printf 'Created %s\n' "$archive"
