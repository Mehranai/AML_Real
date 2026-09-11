#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  echo "Usage: $0 DIRECTORY"; exit 0
fi
[[ $# -eq 1 ]] || { echo "Usage: $0 DIRECTORY" >&2; exit 2; }
for tool in docker jq sha256sum; do command -v "$tool" >/dev/null || { echo "Required: $tool" >&2; exit 1; }; done
directory=$(cd -- "$1" && pwd)
manifest=$directory/manifest.json
[[ -f $manifest ]] || { echo "Missing manifest.json" >&2; exit 1; }
jq -e '
  type == "array" and length > 0 and
  all(.[];
    (.image | type == "string" and test("^[A-Za-z0-9][A-Za-z0-9._:/@-]*$")) and
    (.file | type == "string" and test("^[A-Za-z0-9][A-Za-z0-9_.-]*\\.tar$")) and
    (.sha256 | type == "string" and test("^[a-f0-9]{64}$"))) and
  ((map(.file) | unique | length) == length) and
  ((map(.image) | unique | length) == length)
' "$manifest" >/dev/null || { echo "Invalid image manifest" >&2; exit 1; }
mapfile -t entries < <(jq -r '.[] | [.file,.sha256,.image] | @tsv' "$manifest")
# Verify the entire bundle before loading any image.
for entry in "${entries[@]}"; do
  IFS=$'\t' read -r file expected image <<< "$entry"
  path=$directory/$file
  [[ -f $path && ! -L $path ]] || { echo "Missing or symlinked archive: $file" >&2; exit 1; }
  actual=$(sha256sum "$path")
  [[ ${actual%% *} == "$expected" ]] || { echo "Checksum mismatch: $file" >&2; exit 1; }
done
for entry in "${entries[@]}"; do
  IFS=$'\t' read -r file expected image <<< "$entry"
  docker image load --input "$directory/$file"
  docker image inspect "$image" >/dev/null
done
echo "Verified and imported ${#entries[@]} images."
