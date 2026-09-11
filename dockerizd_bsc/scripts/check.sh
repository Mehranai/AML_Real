#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  echo "Usage: $0 [--audit]"; exit 0
fi
[[ $# -eq 0 || ( $# -eq 1 && $1 == --audit ) ]] || { echo "Expected --audit or no arguments" >&2; exit 2; }
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
if [[ ${1:-} == --audit ]]; then
  command -v cargo-audit >/dev/null || { echo "Run: cargo install cargo-audit --locked" >&2; exit 1; }
  cargo audit --locked
fi
