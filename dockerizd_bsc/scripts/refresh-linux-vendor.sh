#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  echo "Usage: $0 [RUST_TARGET]"; exit 0
fi
[[ $# -le 1 ]] || exit 2
target=${1:-x86_64-unknown-linux-gnu}
project=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
cd -- "$project"
cargo fetch --locked --target "$target"
cargo vendor --locked --versioned-dirs "$work/vendor-linux" >/dev/null
tar -czf "$work/vendor-linux.tar.gz" -C "$work/vendor-linux" .
(cd -- "$work" && sha256sum vendor-linux.tar.gz > vendor-linux.sha256)
mv -f -- "$work/vendor-linux.tar.gz" "$project/vendor-linux.tar.gz"
mv -f -- "$work/vendor-linux.sha256" "$project/vendor-linux.sha256"
echo "Linux vendor archive and checksum refreshed."
