#!/usr/bin/env bash
# Writes the CHANGELOG.md section for a release tag and checks the tag against the crate version.
# The section heading must start with "## [<version>]", where <version> is the tag without its
# leading "v" (for v4.0.0: "## [4.0.0] - 2026-11-20"). Fails if the section is missing or empty,
# or if the kestrel-gpui crate version differs from the tag's major.minor.patch.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <tag> <output-file>" >&2
  exit 2
fi

tag="$1"
output="$2"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="${tag#v}"
core_version="${version%%-*}"

crate_version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repo_root/crates/kestrel-gpui/Cargo.toml" | head -n 1)"
if [[ "$crate_version" != "$core_version" ]]; then
  echo "::error::Tag ${tag} does not match the kestrel-gpui crate version ${crate_version}." >&2
  exit 1
fi

escaped_version="$(printf '%s' "$version" | sed 's/\./\\./g')"
CHANGELOG_VERSION_REGEX="$escaped_version" awk '
  $0 ~ ("^## \\[" ENVIRON["CHANGELOG_VERSION_REGEX"] "\\]") { capture = 1; found = 1; next }
  /^## \[/ && capture { exit }
  capture { print }
  END { if (!found) exit 2 }
' "$repo_root/CHANGELOG.md" > "$output" || {
  echo "::error::No CHANGELOG.md section found for tag ${tag}; add a heading starting with '## [${version}]'." >&2
  exit 1
}
if ! grep -q '[^[:space:]]' "$output"; then
  echo "::error::CHANGELOG.md section for tag ${tag} is empty." >&2
  exit 1
fi
printf 'Release notes for %s: %s lines\n' "$tag" "$(wc -l < "$output")"
