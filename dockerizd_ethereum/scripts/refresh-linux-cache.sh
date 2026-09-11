#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  echo "Usage: $0 [RUST_TARGET]"; exit 0
fi
[[ $# -le 1 ]] || exit 2
target=${1:-x86_64-unknown-linux-gnu}
project=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$project"
cargo fetch --locked --target "$target"
cargo_home=${CARGO_HOME:-$HOME/.cargo}
cargo_home=$(cd -- "$cargo_home" && pwd)
for directory in cache index; do
  [[ -d $cargo_home/registry/$directory ]] || { echo "Missing Cargo registry $directory" >&2; exit 1; }
done
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
tar -czf "$work/cargo-registry-linux.tar.gz" -C "$cargo_home" registry/cache registry/index
(cd -- "$work" && sha256sum cargo-registry-linux.tar.gz > cargo-registry-linux.sha256)
mv -f -- "$work/cargo-registry-linux.tar.gz" "$project/cargo-registry-linux.tar.gz"
mv -f -- "$work/cargo-registry-linux.sha256" "$project/cargo-registry-linux.sha256"
echo "Linux Cargo cache and checksum refreshed."
