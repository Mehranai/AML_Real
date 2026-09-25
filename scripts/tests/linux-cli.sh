#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
for tool in bash jq openssl tar sha256sum; do command -v "$tool" >/dev/null || { echo "Required: $tool" >&2; exit 1; }; done
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
project=$work/project\ with\ spaces
mkdir -p "$project/scripts" "$project/dockerizd_tron/app" "$project/dockerizd_ethereum/scripts" "$project/dockerizd_bsc/scripts" "$work/bin"
cp "$root"/scripts/*.sh "$project/scripts/"
cp "$root"/dockerizd_ethereum/scripts/*.sh "$project/dockerizd_ethereum/scripts/"
cp "$root"/dockerizd_bsc/scripts/*.sh "$project/dockerizd_bsc/scripts/"
export MOCK_LOG=$work/commands.jsonl MOCK_HTTP_LOG=$work/http.jsonl
export CARGO_HOME=$work/cargo
export PATH=$work/bin:$PATH
cat > "$work/bin/docker" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
jq -cn --args '$ARGS.positional' -- "$@" >> "$MOCK_LOG"
[[ ${MOCK_DOCKER_EXIT:-0} == 0 ]] || exit "$MOCK_DOCKER_EXIT"
if [[ $1 == image && $2 == save ]]; then
  [[ $3 == --output ]]
  printf 'test image: %s\n' "$5" > "$4"
fi
if [[ $1 == compose && " $* " == *' ps --all --quiet '* ]]; then
  [[ ${MOCK_MISSING_SERVICE:-} != "${!#}" ]] || exit 0
  printf 'abc123\n'
fi
if [[ $1 == inspect ]]; then printf '%s\n' "${MOCK_RUNTIME_STATE:-running healthy}"; fi
MOCK
cat > "$work/bin/curl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ ${MOCK_HTTP_EXIT:-0} == 0 ]] || exit "$MOCK_HTTP_EXIT"
output=
config=
url=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output) output=$2; shift 2 ;;
    --config) config=$2; shift 2 ;;
    --connect-timeout|--max-time) shift 2 ;;
    --fail|--silent|--show-error) shift ;;
    *) url=$1; shift ;;
  esac
done
if [[ ${MOCK_AUTH_REQUIRED:-false} == true ]]; then
  [[ -n $config && -f $config ]]
  grep -q 'user = "analyst:demo-password"' "$config"
fi
jq -cn --arg url "$url" '{url:$url}' >> "$MOCK_HTTP_LOG"
case "$url" in
  */health) printf '{"status":"alive"}' > "$output" ;;
  */ready) printf '{"status":"%s","dependencies":{"neo4j":"ready"}}' "${MOCK_READY:-ready}" > "$output" ;;
  */investigation)
    address=${url%/investigation}; address=${address##*/}
    jq -cn --arg address "${MOCK_ADDRESS:-$address}" '{address:$address,investigation:{state:"temporary"},risk_engine:{probability_claimed:false}}' > "$output" ;;
  */paths/*)
    if [[ ${MOCK_BAD_PATHS:-false} == true ]]; then echo '{"paths":null}' > "$output"; else echo '{"paths":[]}' > "$output"; fi ;;
  *) exit 22 ;;
esac
MOCK
cat > "$work/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
jq -cn --args '$ARGS.positional' -- cargo "$@" >> "$MOCK_LOG"
[[ ${MOCK_CARGO_EXIT:-0} == 0 ]] || exit "$MOCK_CARGO_EXIT"
case "$1" in
  fetch)
    mkdir -p "$CARGO_HOME/registry/cache" "$CARGO_HOME/registry/index"
    printf 'cached crate\n' > "$CARGO_HOME/registry/cache/example.crate"
    printf 'index\n' > "$CARGO_HOME/registry/index/config.json" ;;
  vendor)
    destination=${!#}
    mkdir -p "$destination/example"
    printf 'crate\n' > "$destination/example/Cargo.toml" ;;
  test)
    if [[ -n ${BSC_TEST_CLICKHOUSE_URL:-} ]]; then
      [[ ${BSC_TEST_CLICKHOUSE_PASSWORD:-} == demo-db-password ]]
      [[ ${BSC_TEST_CLICKHOUSE_URL:-} == http://127.0.0.1:38123 ]]
    fi ;;
esac
MOCK
printf '#!/usr/bin/env bash\nexit 0\n' > "$work/bin/cargo-audit"
chmod +x "$work/bin/"*
reset_log() { : > "$MOCK_LOG"; : > "$MOCK_HTTP_LOG"; }
assert_log() { jq -es "$1" "$MOCK_LOG" >/dev/null; }
expect_failure() {
  if "$@" > "$work/failure.txt" 2>&1; then
    echo "Expected failure: $*" >&2; exit 1
  fi
}
reset_log
for file in "$project"/scripts/*.sh "$project"/dockerizd_{ethereum,bsc}/scripts/*.sh; do
  bash -n "$file"
  bash "$file" --help >/dev/null
done
expect_failure bash "$project/scripts/vm.sh" tron up
[[ ! -s $MOCK_LOG ]]
for directory in "$project" "$project/dockerizd_tron/app" "$project/dockerizd_ethereum" "$project/dockerizd_bsc"; do
  printf '# Test configuration\n' > "$directory/.env"
done

bash "$project/scripts/vm.sh" tron up --build --api-only >/dev/null
assert_log 'length == 1 and (.[0] | index("--build") != null and index("tron-api") != null and index("tron-ingestion") == null)'
reset_log
bash "$project/scripts/vm.sh" bsc up --api-only >/dev/null
assert_log '.[0] | index("bsc-api") != null and index("bsc-follow") == null'
reset_log
bash "$project/scripts/vm.sh" bsc check >/dev/null
assert_log '.[0][-4:] == ["-T","bsc-api","bsc_api","--healthcheck"]'
reset_log
bash "$project/scripts/vm.sh" ethereum up --offline --api-only --pull-never --project custom-eth >/dev/null
assert_log '.[0] | index("custom-eth") != null and index("--build") != null and index("never") != null and any(.[]; endswith("docker-compose.offline.yml")) and index("ethereum-ingestion") == null'
reset_log
bash "$project/scripts/vm.sh" main up --pull-never >/dev/null
assert_log '.[0] | index("--no-build") != null and index("never") != null and index("gateway") != null'
reset_log
bash "$project/scripts/vm.sh" ethereum check >/dev/null
assert_log '.[0][-4:] == ["exec","-T","ethereum-api","ethereum_healthcheck"]'
reset_log
for role in tron ethereum main; do bash "$project/scripts/vm.sh" "$role" down >/dev/null; done
assert_log 'length == 3 and all(.[]; .[-1] == "down" and index("-v") == null and index("--volumes") == null)'
reset_log
expect_failure bash "$project/scripts/vm.sh" tron up --typo
expect_failure bash "$project/scripts/vm.sh" tron up --offline
expect_failure bash "$project/scripts/vm.sh" main up --api-only
expect_failure bash "$project/scripts/vm.sh" tron logs --build
expect_failure bash "$project/scripts/vm.sh" tron up --project
[[ ! -s $MOCK_LOG ]]
MOCK_DOCKER_EXIT=23 expect_failure bash "$project/scripts/vm.sh" tron up
echo "PASS VM roles, quoting, option combinations, errors and volume preservation"

for role in tron ethereum bsc; do
  reset_log
  bash "$project/scripts/vm.sh" "$role" pause >/dev/null
  assert_log 'length == 1 and (.[0] | index("stop") != null and index("--timeout") != null and index("150") != null and index("clickhouse") == null and index("down") == null)'
  jq -e --arg role "$role" 'index("aml-" + $role) != null and index($role + "-api") != null' "$MOCK_LOG" >/dev/null
  if [[ $role == bsc ]]; then assert_log '.[0] | index("--profile") != null and index("runtime") != null'; fi
  reset_log
  bash "$project/scripts/vm.sh" "$role" pause-ingestion >/dev/null
  jq -e --arg role "$role" 'index("stop") != null and index($role + "-api") == null and index("clickhouse") == null' "$MOCK_LOG" >/dev/null
  reset_log
  bash "$project/scripts/vm.sh" "$role" resume >/dev/null
  assert_log 'length == 2 and (.[0] | index("exec") != null and index("clickhouse") != null) and (.[1] | index("up") != null and index("--no-deps") != null and index("--no-build") != null and index("never") != null and index("clickhouse") == null and index("down") == null)'
done
reset_log
sql="SELECT 'a b; \$HOME' AS value"
bash "$project/scripts/vm.sh" ethereum db --query "$sql" >/dev/null
jq -e --arg sql "$sql" '.[-2:] == ["--query", $sql] and index("ethereum_aml") != null and any(.[]; contains("--readonly 1")) and any(.[]; contains("$CLICKHOUSE_PASSWORD"))' "$MOCK_LOG" >/dev/null
assert_log 'length == 1 and (.[0] | index("exec") != null and index("-T") != null and index("clickhouse") != null)'
reset_log
bash "$project/scripts/vm.sh" tron db </dev/null >/dev/null
assert_log 'length == 1 and (.[0] | index("tron_db") != null and index("--query") == null)'
reset_log
for action in pause pause-ingestion resume db; do expect_failure bash "$project/scripts/vm.sh" main "$action"; done
expect_failure bash "$project/scripts/vm.sh" tron pause --api-only
expect_failure bash "$project/scripts/vm.sh" tron resume --build
expect_failure bash "$project/scripts/vm.sh" tron ps --query 'SELECT 1'
expect_failure bash "$project/scripts/vm.sh" bsc db --query
[[ ! -s $MOCK_LOG ]]
MOCK_DOCKER_EXIT=23 expect_failure bash "$project/scripts/vm.sh" ethereum resume
assert_log 'length == 1 and (.[0] | index("up") == null)'
echo "PASS independent app controls, retained databases, read-only SQL and query quoting"
for action in up pause pause-ingestion resume; do
  reset_log
  bash "$project/scripts/vm.sh" tron "$action" >/dev/null
  assert_log '.[-1] | index("tron-analytics") != null'
done
echo "PASS TRON analytics participates in full startup, pause and resume"

for role in main tron ethereum bsc; do
  reset_log
  bash "$project/scripts/vm.sh" "$role" check-runtime >/dev/null
  assert_log 'any(.[]; index("inspect") != null) and any(.[]; index("exec") != null)'
done
MOCK_MISSING_SERVICE=tron-analytics expect_failure bash "$project/scripts/vm.sh" tron check-runtime
MOCK_RUNTIME_STATE='exited unhealthy' expect_failure bash "$project/scripts/vm.sh" ethereum check-runtime
MOCK_RUNTIME_STATE='running starting' expect_failure bash "$project/scripts/vm.sh" bsc check-runtime
echo "PASS full runtime checks reject missing, stopped and unhealthy workers"

reset_log
bash "$project/scripts/aml.sh" up --build >/dev/null
assert_log 'length == 3 and (.[0] | index("app") != null and index("tron-ingestion") == null) and (.[1] | index("dockerizd_ethereum") != null) and (.[2] | index("aml-whole") != null)'
reset_log
bash "$project/scripts/aml.sh" down >/dev/null
assert_log '(.[0] | index("aml-whole") != null) and (.[1] | index("dockerizd_ethereum") != null) and (.[2] | index("app") != null)'
echo "PASS single-host launcher preserves existing Compose project identities"

key=$(bash "$project/scripts/new-service-key.sh")
[[ $key =~ ^[a-f0-9]{96}$ ]]
[[ $key != "$(bash "$project/scripts/new-service-key.sh")" ]]
expect_failure bash "$project/scripts/new-service-key.sh" 31
expect_failure bash "$project/scripts/new-service-key.sh" invalid
echo "PASS service key generation and validation"

reset_log
bundle=$work/image\ bundle
bash "$project/scripts/export-images.sh" --output "$bundle" >/dev/null
jq -e 'length == 6' "$bundle/manifest.json" >/dev/null
bash "$project/scripts/import-images.sh" "$bundle" >/dev/null
assert_log '[.[] | select(.[0:2] == ["image","load"])] | length == 6'
cp "$bundle/manifest.json" "$work/good-manifest.json"
printf 'corrupt\n' >> "$bundle/neo4j_5.26-community.tar"
reset_log
expect_failure bash "$project/scripts/import-images.sh" "$bundle"
[[ ! -s $MOCK_LOG ]]
jq '.[0].file = "../outside.tar"' "$work/good-manifest.json" > "$bundle/manifest.json"
expect_failure bash "$project/scripts/import-images.sh" "$bundle"
jq '. + [.[0]]' "$work/good-manifest.json" > "$bundle/manifest.json"
expect_failure bash "$project/scripts/import-images.sh" "$bundle"
[[ ! -s $MOCK_LOG ]]
echo "PASS bundle round-trip, full preflight checksums, duplicate/path rejection"

reset_log
bash "$project/scripts/smoke-test.sh" --main-url http://test.invalid --tron-address TSource --tron-path-target TTarget \
  --ethereum-address 0xABC --ethereum-path-target 0xDEF >/dev/null
jq -se 'length == 8 and any(.[]; .url | endswith("TSource/paths/TTarget?max_depth=10")) and any(.[]; .url | endswith("0xabc/paths/0xdef?max_hops=10"))' "$MOCK_HTTP_LOG" >/dev/null
printf 'demo-password\n' | MOCK_AUTH_REQUIRED=true bash "$project/scripts/smoke-test.sh" --user analyst >/dev/null
MOCK_READY=unready expect_failure bash "$project/scripts/smoke-test.sh"
MOCK_HTTP_EXIT=22 expect_failure bash "$project/scripts/smoke-test.sh"
MOCK_ADDRESS=other expect_failure bash "$project/scripts/smoke-test.sh" --tron-address TSource
MOCK_BAD_PATHS=true expect_failure bash "$project/scripts/smoke-test.sh" --tron-address TSource --tron-path-target TTarget
expect_failure bash "$project/scripts/smoke-test.sh" --tron-path-target TTarget
echo "PASS smoke-test identity, ten-hop queries, authentication and failure handling"

reset_log
bash "$project/dockerizd_ethereum/scripts/refresh-linux-cache.sh" >/dev/null
(cd "$project/dockerizd_ethereum" && sha256sum --check cargo-registry-linux.sha256 >/dev/null)
tar -tzf "$project/dockerizd_ethereum/cargo-registry-linux.tar.gz" | grep -q 'registry/cache/example.crate'
bash "$project/dockerizd_bsc/scripts/refresh-linux-vendor.sh" >/dev/null
(cd "$project/dockerizd_bsc" && sha256sum --check vendor-linux.sha256 >/dev/null)
tar -tzf "$project/dockerizd_bsc/vendor-linux.tar.gz" | grep -q 'example/Cargo.toml'
bash "$project/dockerizd_bsc/scripts/check.sh" --audit
expect_failure env -u BSC_CLICKHOUSE_PASSWORD bash "$project/dockerizd_bsc/scripts/test-clickhouse.sh"
BSC_CLICKHOUSE_PASSWORD=demo-db-password bash "$project/dockerizd_bsc/scripts/test-clickhouse.sh"
MOCK_CARGO_EXIT=42 expect_failure bash "$project/dockerizd_bsc/scripts/check.sh"
echo "PASS Cargo cache/vendor checksums and BSC helper error propagation"
echo "All Linux CLI contract tests passed (Docker, HTTP and Cargo are isolated test doubles)."
