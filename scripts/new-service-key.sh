#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  echo "Usage: $0 [BYTES: 32..128]"; exit 0
fi
bytes=${1:-48}
if ! [[ $# -le 1 && $bytes =~ ^[1-9][0-9]{1,2}$ ]] || ! (( bytes >= 32 && bytes <= 128 )); then
  echo "Expected an integer byte count between 32 and 128" >&2; exit 2
fi
command -v openssl >/dev/null || { echo "Install openssl first" >&2; exit 1; }
openssl rand -hex "$bytes"
