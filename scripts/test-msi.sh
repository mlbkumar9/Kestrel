#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
stage_directory="$temporary_directory/stage"
output_msi="$temporary_directory/LangMux-test.msi"

if ! command -v msiinfo >/dev/null 2>&1; then
  echo "msiinfo is required; install msitools." >&2
  exit 1
fi

standard_files=(
  langmux.exe
  LICENSE
  README.md
  THIRD_PARTY_NOTICES.md
  logo.ico
  tools/ffmpeg/bin/ffmpeg.exe
  tools/ffmpeg/bin/ffprobe.exe
  tools/ffmpeg/LICENSE
  tools/mkvtoolnix/mkvmerge.exe
  tools/mkvtoolnix/COPYING.txt
)

for relative_path in "${standard_files[@]}"; do
  file_path="$stage_directory/$relative_path"
  mkdir -p "$(dirname "$file_path")"
  if [[ "$relative_path" == "logo.ico" ]]; then
    cp "$repo_root/logo.ico" "$file_path"
  else
    printf 'fixture: %s\n' "$relative_path" > "$file_path"
  fi
done
printf 'fixture DLL\n' > "$stage_directory/tools/mkvtoolnix/libfixture.dll"
printf 'fixture DLL\n' > "$stage_directory/tools/mkvtoolnix/LIBUPPER.DLL"

"$repo_root/scripts/build-msi.sh" "$stage_directory" 2.0.0 "$output_msi"

python3 - "$output_msi" <<'PY'
from collections import Counter
import subprocess
import sys

msi_path = sys.argv[1]


def exported_rows(table):
    result = subprocess.run(
        ["msiinfo", "export", msi_path, table],
        check=True,
        capture_output=True,
        text=True,
    )
    return [
        line.split("\t")
        for line in result.stdout.splitlines()[3:]
        if line.strip()
    ]


file_rows = exported_rows("File")
actual_files = Counter(row[2] for row in file_rows)
expected_files = Counter(
    [
        "langmux.exe",
        "LICENSE",
        "README.md",
        "THIRD_PARTY_NOTICES.md",
        "logo.ico",
        "ffmpeg.exe",
        "ffprobe.exe",
        "LICENSE",
        "mkvmerge.exe",
        "COPYING.txt",
        "libfixture.dll",
        "LIBUPPER.DLL",
    ]
)
if actual_files != expected_files:
    missing = expected_files - actual_files
    unexpected = actual_files - expected_files
    raise SystemExit(
        f"File table mismatch; missing={dict(missing)}, unexpected={dict(unexpected)}"
    )

shortcuts = exported_rows("Shortcut")
if not any(row[0] == "LangMuxStartMenuShortcut" for row in shortcuts):
    raise SystemExit("LangMux Start Menu shortcut is missing.")

print("PASS: MSI contains 10 standard files, libfixture.dll, and LIBUPPER.DLL (12 files total).")
print("PASS: LangMux Start Menu shortcut is present.")
PY
