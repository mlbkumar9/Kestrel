#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
stage_directory="$temporary_directory/stage"
output_msi="$temporary_directory/Kestrel-test.msi"

if ! command -v msiinfo >/dev/null 2>&1; then
  echo "msiinfo is required; install msitools." >&2
  exit 1
fi

standard_files=(
  kestrel.exe
  LICENSE
  README.md
  THIRD_PARTY_NOTICES.md
  kestrel.ico
  tools/ffmpeg/bin/ffmpeg.exe
  tools/ffmpeg/bin/ffprobe.exe
  tools/ffmpeg/LICENSE
  tools/mkvtoolnix/mkvmerge.exe
  tools/mkvtoolnix/COPYING.txt
)

for relative_path in "${standard_files[@]}"; do
  file_path="$stage_directory/$relative_path"
  mkdir -p "$(dirname "$file_path")"
  if [[ "$relative_path" == "kestrel.ico" ]]; then
    cp "$repo_root/crates/kestrel-app/assets/kestrel.ico" "$file_path"
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
        "kestrel.exe",
        "LICENSE",
        "README.md",
        "THIRD_PARTY_NOTICES.md",
        "kestrel.ico",
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
        f"File table mismatch; missing={dict(missing)}, "
        f"unexpected={dict(unexpected)}"
    )

shortcuts = exported_rows("Shortcut")
if not any(row[0] == "KestrelStartMenuShortcut" for row in shortcuts):
    raise SystemExit("Kestrel Start Menu shortcut is missing.")

remove_files = {
    (row[3], row[2].split("|")[-1]) for row in exported_rows("RemoveFile")
}
expected_cleanup = {
    ("LegacyLangMuxFolder", "Uninstall LangMux.lnk"),
    ("LegacyLangMuxMenuFolder", "LangMux.lnk"),
    ("DesktopFolder", "LangMux.lnk"),
}
if not expected_cleanup <= remove_files:
    raise SystemExit(
        "LangMux 1.0.0 shortcut cleanup rows are missing; "
        f"got {sorted(remove_files)}"
    )

version_one_product_code = "B8A9B972-0392-476D-91E5-99DE012FDFCB"
cleanup_component = next(
    (
        row
        for row in exported_rows("Component")
        if row[0] == "LegacyLangMuxCleanup"
    ),
    None,
)
if (
    cleanup_component is None
    or version_one_product_code not in cleanup_component[4]
):
    raise SystemExit(
        "LegacyLangMuxCleanup must be conditioned on the LangMux 1.0.0 MSI; "
        f"got {cleanup_component}"
    )

upgrade_rows = exported_rows("Upgrade")
version_one_code = "e110f64d-5c3d-5e2d-9b02-53217f2caea5"
version_two_code = "bba02aaf-78ca-41e3-8586-b84b471755ed"
if len(upgrade_rows) != 3:
    raise SystemExit(f"Expected three Upgrade rows; got {upgrade_rows}")
if any(
    row[0].strip("{}").casefold() != version_one_code
    for row in upgrade_rows[:2]
):
    raise SystemExit(
        "Upgrade rows 1 and 2 must use the v1.0.0 UpgradeCode; "
        f"got {upgrade_rows[:2]}"
    )
version_two = upgrade_rows[2]
if (
    version_two[0].strip("{}").casefold() != version_two_code
    or version_two[6] != "LEGACY_V2_FOUND"
):
    raise SystemExit(
        "Upgrade row 3 must use the v2.0.0 UpgradeCode and LEGACY_V2_FOUND; "
        f"got {version_two}"
    )

version_one_rows = [
    row for row in upgrade_rows
    if row[0].strip("{}").casefold() == version_one_code
]
if {int(row[4]) for row in version_one_rows} != {1, 2}:
    raise SystemExit(
        "v1.0.0 Upgrade rows should have attributes 1 and 2; "
        f"got {version_one_rows}"
    )
if int(version_two[4]) != 256:
    raise SystemExit(
        "v2.0.0 Upgrade row should use attributes 256 and LEGACY_V2_FOUND; "
        f"got {version_two}"
    )

print(
    "PASS: MSI contains 10 standard files, libfixture.dll, and LIBUPPER.DLL "
    "(12 files total)."
)
print("PASS: Kestrel Start Menu shortcut is present.")
print("PASS: Upgrade table contains v1.0.0 and v2.0.0 upgrade rows.")
PY
