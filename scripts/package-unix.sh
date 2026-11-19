#!/usr/bin/env bash
# Packages the GPUI app (kestrel-gpui) for macOS and Linux.
#   macos: Kestrel-<tag>-macos-<arch>.zip containing Kestrel.app (icon, Info.plist), README, LICENSE
#   linux: Kestrel-<tag>-linux-x64.tar.gz containing kestrel, a .desktop file and the 256 px icon
# Tools (mkvmerge, ffmpeg, ffprobe) are expected on the system, as before.
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
iconset="$repo_root/crates/kestrel-app/assets/kestrel.iconset"
mkdir -p "$output_directory"
output_directory="$(cd "$output_directory" && pwd)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

if [[ "$platform" == "macos" ]]; then
  # CFBundleVersion fields must be numeric: v4.0.0-rc1 becomes 4.0.0.
  version="${tag#v}"
  version="${version%%-*}"
  app="$stage/Kestrel.app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cp "$binary" "$app/Contents/MacOS/Kestrel"
  chmod 755 "$app/Contents/MacOS/Kestrel"

  if ! command -v iconutil >/dev/null 2>&1; then
    echo "iconutil is required to build Kestrel.icns; run this on macOS." >&2
    exit 1
  fi
  iconutil --convert icns --output "$app/Contents/Resources/Kestrel.icns" "$iconset"

  cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>Kestrel</string>
  <key>CFBundleIconFile</key>
  <string>Kestrel</string>
  <key>CFBundleIdentifier</key>
  <string>io.github.mlbkumar9.kestrel</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Kestrel</string>
  <key>CFBundleDisplayName</key>
  <string>Kestrel</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.video</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
PLIST
  if command -v plutil >/dev/null 2>&1; then
    plutil -lint "$app/Contents/Info.plist"
  fi

  # Ad-hoc signature: unsigned arm64 bundles are refused by Gatekeeper's stricter path.
  if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "$app"
  fi

  cp "$repo_root/README.md" "$stage/README.md"
  cp "$repo_root/LICENSE" "$stage/LICENSE"
  cp "$repo_root/THIRD_PARTY_NOTICES.md" "$stage/THIRD_PARTY_NOTICES.md"

  archive="$output_directory/Kestrel-$tag-macos-$arch.zip"
  rm -f "$archive"
  if command -v ditto >/dev/null 2>&1; then
    ditto -c -k --sequesterRsrc "$stage" "$archive"
  else
    (cd "$stage" && zip -qry "$archive" .)
  fi
else
  cp "$binary" "$stage/kestrel"
  chmod 755 "$stage/kestrel"
  cp "$repo_root/README.md" "$stage/README.md"
  cp "$repo_root/LICENSE" "$stage/LICENSE"
  cp "$repo_root/THIRD_PARTY_NOTICES.md" "$stage/THIRD_PARTY_NOTICES.md"
  mkdir -p "$stage/share/applications" "$stage/share/icons/hicolor/256x256/apps"
  cp "$repo_root/packaging/linux/kestrel.desktop" "$stage/share/applications/kestrel.desktop"
  cp "$iconset/icon_256x256.png" "$stage/share/icons/hicolor/256x256/apps/kestrel.png"

  if [[ "$arch" == "x64" ]]; then
    archive="$output_directory/Kestrel-$tag-linux-x64.tar.gz"
  else
    archive="$output_directory/Kestrel-$tag-linux-$arch.tar.gz"
  fi
  tar -czf "$archive" -C "$stage" .
fi
printf 'Created %s\n' "$archive"
