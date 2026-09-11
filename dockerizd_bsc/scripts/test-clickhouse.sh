#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  echo "Usage: BSC_CLICKHOUSE_PASSWORD=... $0"; exit 0
fi
[[ $# -eq 0 ]] || exit 2
: "${BSC_CLICKHOUSE_PASSWORD:?Set the password for the disposable ClickHouse test service}"
export BSC_TEST_CLICKHOUSE_URL=${BSC_CLICKHOUSE_URL:-http://127.0.0.1:38123}
export BSC_TEST_CLICKHOUSE_USER=${BSC_CLICKHOUSE_USER:-bsc_admin}
export BSC_TEST_CLICKHOUSE_PASSWORD=$BSC_CLICKHOUSE_PASSWORD
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo test --locked --offline -- --ignored
