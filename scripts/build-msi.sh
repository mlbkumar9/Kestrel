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
  kestrel.exe \
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

if [[ ! -f "$stage_dir/kestrel.ico" ]]; then
  cp "$repo_root/LangMux_Native/assets/kestrel.ico" "$stage_dir/kestrel.ico"
fi

output_directory="$(dirname "$output_msi")"
mkdir -p "$output_directory"
output_msi="$(cd "$output_directory" && pwd)/$(basename "$output_msi")"
wxs_directory="$repo_root/packaging/windows"
stage_relative="$(realpath --relative-to="$wxs_directory" "$stage_dir")"

if ! command -v wixl >/dev/null 2>&1; then
  echo "wixl is required; install msitools." >&2
  exit 1
fi

temp_wxs="$(mktemp "$wxs_directory/langmux.XXXXXX")"
trap 'rm -f "$temp_wxs"' EXIT
python3 - "$wxs_directory/langmux.wxs" "$temp_wxs" "$stage_dir" <<'PY'
from hashlib import sha256
from pathlib import Path
import re
import subprocess
import sys
import uuid
from xml.sax.saxutils import quoteattr

source_path, output_path, stage_path = map(Path, sys.argv[1:])
source = source_path.read_text(encoding="utf-8")
stage = Path(stage_path)
component_marker = "<!-- MKVTOOLNIX_DLLS -->"
reference_marker = "<!-- MKVTOOLNIX_DLL_REFS -->"

if source.count(component_marker) != 1 or source.count(reference_marker) != 1:
    raise SystemExit("WiX template is missing or duplicates an MKVToolNix DLL marker.")

dll_directory = stage / "tools" / "mkvtoolnix"
dlls = sorted(
    (
        path
        for path in dll_directory.iterdir()
        if path.is_file() and path.suffix.casefold() == ".dll"
    ),
    key=lambda path: (path.name.casefold(), path.name),
)
components = []
references = []

for dll in dlls:
    name = dll.name
    seed = f"langmux/mkvtoolnix/{name}"
    try:
        result = subprocess.run(
            ["uuidgen", "--sha1", "--namespace", "@url", "--name", seed],
            check=True,
            capture_output=True,
            text=True,
        )
        component_guid = "{" + str(uuid.UUID(result.stdout.strip())).upper() + "}"
    except (FileNotFoundError, subprocess.CalledProcessError, ValueError):
        component_guid = "{" + str(uuid.uuid5(uuid.NAMESPACE_URL, seed)).upper() + "}"

    safe_name = re.sub(r"[^A-Za-z0-9_]", "_", name)
    suffix = sha256(name.encode("utf-8")).hexdigest()[:8]
    component_id = f"MkvToolnixDll_{safe_name[:42]}_{suffix}"
    file_id = f"MkvToolnixDllFile_{safe_name[:38]}_{suffix}"
    source_file = f"$(var.StageDir)/tools/mkvtoolnix/{name}"
    components.append(
        f'              <Component Id="{component_id}" Guid="{component_guid}">\n'
        f'                <File Id="{file_id}" Name={quoteattr(name)} '
        f'Source={quoteattr(source_file)} KeyPath="yes" />\n'
        "              </Component>"
    )
    references.append(f'      <ComponentRef Id="{component_id}" />')


def replace_marker(text: str, marker: str, replacement: str) -> str:
    pattern = rf"(?m)^[ \t]*{re.escape(marker)}[ \t]*$"
    return re.sub(pattern, lambda _: replacement, text)


source = replace_marker(source, component_marker, "\n".join(components))
source = replace_marker(source, reference_marker, "\n".join(references))
output_path.write_text(source, encoding="utf-8")
PY

wixl -a x64 \
  -D "StageDir=$stage_relative" \
  -D "Version=$version" \
  -o "$output_msi" \
  "$temp_wxs"

# wixl does not understand <RemoveFile>, so add the rows that delete the shortcuts
# the LangMux 1.0.0 MSI leaves behind when a major upgrade removes it.
remove_file_idt="$(mktemp --suffix=.idt)"
trap 'rm -f "$temp_wxs" "$remove_file_idt"' EXIT
msiinfo export "$output_msi" RemoveFile > "$remove_file_idt"
printf '%s\t%s\t%s\t%s\t%s\n' \
  RemoveLegacyUninstallShortcut LegacyLangMuxCleanup 'UNINST~1.LNK|Uninstall LangMux.lnk' LegacyLangMuxFolder 1 \
  RemoveLegacyLangMuxFolder LegacyLangMuxCleanup '' LegacyLangMuxFolder 1 \
  RemoveLegacyMenuShortcut LegacyLangMuxCleanup 'LangMux.lnk' LegacyLangMuxMenuFolder 1 \
  RemoveLegacyLangMuxMenuFolder LegacyLangMuxCleanup '' LegacyLangMuxMenuFolder 1 \
  RemoveLegacyDesktopShortcut LegacyLangMuxCleanup 'LangMux.lnk' DesktopFolder 1 \
  >> "$remove_file_idt"
msibuild "$output_msi" -i "$remove_file_idt"
