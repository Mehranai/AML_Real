#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output=$root/deployment-artifacts
include_bsc=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output) [[ $# -ge 2 && -n $2 ]] || exit 2; output=$2; shift 2 ;;
    --include-bsc) include_bsc=true; shift ;;
    --help|-h) echo "Usage: $0 [--output DIRECTORY] [--include-bsc]"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done
for tool in docker jq sha256sum; do command -v "$tool" >/dev/null || { echo "Required: $tool" >&2; exit 1; }; done
images=(aml-whole-gateway:local aml-analytical-node:local tron-aml-service:local ethereum-aml-service:local clickhouse/clickhouse-server:23.8 neo4j:5.26-community)
files=(aml-whole-gateway_local.tar aml-analytical-node_local.tar tron-aml-service_local.tar ethereum-aml-service_local.tar clickhouse-server_23.8.tar neo4j_5.26-community.tar)
if $include_bsc; then images+=(bsc-aml-service:local); files+=(bsc-aml-service_local.tar); fi
for image in "${images[@]}"; do docker image inspect "$image" >/dev/null; done
mkdir -p -- "$output"
output=$(cd -- "$output" && pwd)
work=$(mktemp -d "$output/.export.XXXXXXXX")
trap 'rm -rf -- "$work"' EXIT
for index in "${!images[@]}"; do
  image=${images[$index]}
  file=${files[$index]}
  docker image save --output "$work/$file" "$image"
  checksum=$(sha256sum "$work/$file")
  checksum=${checksum%% *}
  jq -cn --arg image "$image" --arg file "$file" --arg sha256 "$checksum" \
    '{image:$image,file:$file,sha256:$sha256}' >> "$work/entries.jsonl"
done
jq -s . "$work/entries.jsonl" > "$work/manifest.json"
for file in "${files[@]}"; do mv -f -- "$work/$file" "$output/$file"; done
mv -f -- "$work/manifest.json" "$output/manifest.json"
echo "Exported and checksummed ${#images[@]} images to $output"
