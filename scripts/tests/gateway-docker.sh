#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  echo "Usage: $0 (requires the built/imported aml-whole-gateway:local image)"; exit 0
fi
[[ $# -eq 0 ]] || exit 2
for tool in docker curl; do command -v "$tool" >/dev/null || { echo "Required: $tool" >&2; exit 1; }; done
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
work=$(mktemp -d)
project=aml-bash-gateway-$$
mkdir -p "$work/scripts" "$work/gateway/auth"
cp "$root/scripts/vm.sh" "$work/scripts/"
cp "$root/compose.yaml" "$work/"
cp "$root/.env.example" "$work/.env"
cp "$root/gateway/auth/disabled.htpasswd" "$work/gateway/auth/"
export AML_PORT=0 AML_BIND_ADDRESS=127.0.0.1 AML_BASIC_AUTH_REALM=off
export AML_HTPASSWD_FILE=./gateway/auth/disabled.htpasswd
export AML_TRON_UPSTREAM=http://127.0.0.1:4001 AML_ETHEREUM_UPSTREAM=http://127.0.0.1:5001
export AML_SERVICE_KEY=test-only-service-key-0123456789abcdef NEO4J_PASSWORD=test-only-neo4j-password
export AML_NEO4J_HTTP_PORT=0 AML_NEO4J_BOLT_PORT=0
cleanup() {
  result=$?
  bash "$work/scripts/vm.sh" main down --project "$project" >/dev/null 2>&1 || true
  rm -rf -- "$work"
  exit "$result"
}
trap cleanup EXIT
bash "$work/scripts/vm.sh" main up --pull-never --project "$project"
ready=false
for ((attempt=0; attempt<120; attempt++)); do
  if bash "$work/scripts/vm.sh" main check --project "$project" >/dev/null 2>&1; then ready=true; break; fi
  sleep 1
done
$ready || { bash "$work/scripts/vm.sh" main logs --project "$project"; exit 1; }
endpoint=$(docker compose --project-directory "$work" --project-name "$project" --file "$work/compose.yaml" --env-file "$work/.env" port gateway 8080)
endpoint=${endpoint//$'\r'/}
curl --fail --silent --show-error "http://$endpoint/health"
curl --fail --silent --show-error --output /dev/null "http://$endpoint/"
echo
echo "PASS real Gateway image: Bash startup, readiness, HTTP/UI and isolated cleanup."
