#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 {main|tron|ethereum|bsc} {up|down|ps|logs|check} [--build] [--api-only] [--offline] [--pull-never] [--project NAME]"
}
fail() { echo "$*" >&2; exit 2; }
if [[ ${1:-} == --help || ${1:-} == -h ]]; then usage; exit 0; fi
[[ $# -ge 1 ]] || { usage >&2; exit 2; }
role=$1
shift
action=up
if [[ $# -gt 0 && $1 != --* ]]; then action=$1; shift; fi
build=false
api_only=false
offline=false
pull_never=false
project_override=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --build) build=true; shift ;;
    --api-only) api_only=true; shift ;;
    --offline) offline=true; build=true; shift ;;
    --pull-never) pull_never=true; shift ;;
    --project)
      [[ $# -ge 2 && $2 =~ ^[a-z0-9][a-z0-9_-]*$ ]] || fail "--project requires a valid Compose project name"
      project_override=$2; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) fail "Unknown option: $1" ;;
  esac
done
case "$action" in up|down|ps|logs|check) ;; *) fail "Unknown action: $action" ;; esac
if [[ $action != up ]] && { $build || $api_only || $pull_never; }; then
  fail "Build, API-only and pull options are valid only with up"
fi
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
case "$role" in
  main)
    directory=$root; file=compose.yaml; project=aml-main
    services=(gateway); probe_service=gateway
    probe=(wget -q -O - http://analytical-node:7001/ready) ;;
  tron)
    directory=$root/dockerizd_tron/app; file=docker-compose.yml; project=aml-tron
    services=(tron-api tron-ingestion tron-token-metadata-worker); probe_service=tron-api
    probe=(curl --fail --silent --show-error http://127.0.0.1:4001/ready)
    if $api_only; then services=(tron-api); fi ;;
  ethereum)
    directory=$root/dockerizd_ethereum; file=docker-compose.yml; project=aml-ethereum
    services=(ethereum-api ethereum-ingestion ethereum-token-metadata ethereum-analytics); probe_service=ethereum-api
    probe=(ethereum_healthcheck)
    if $api_only; then services=(ethereum-api); fi ;;
  bsc)
    directory=$root/dockerizd_bsc; file=docker-compose.yml; project=aml-bsc
    services=(bsc-api bsc-follow bsc-token-metadata); probe_service=bsc-api
    probe=(bsc_api --healthcheck)
    if $api_only; then services=(bsc-api); fi ;;
  *) fail "Unknown role: $role" ;;
esac
[[ $role != main || $api_only == false ]] || fail "--api-only requires a chain role"
[[ $offline == false || $role == ethereum ]] || fail "--offline is supported only for Ethereum source builds"
[[ -z $project_override ]] || project=$project_override
env_file=$directory/.env
[[ -f $env_file ]] || fail "Missing $env_file; copy .env.example and configure this VM first"
command -v docker >/dev/null || fail "Docker with Compose v2 is required"
compose=(docker compose --project-directory "$directory" --project-name "$project" --file "$directory/$file" --env-file "$env_file")
if $offline; then compose+=(--file "$directory/docker-compose.offline.yml"); fi
case "$action" in
  up)
    arguments=(up -d)
    if $build; then arguments+=(--build); else arguments+=(--no-build); fi
    if $pull_never; then arguments+=(--pull never); fi
    "${compose[@]}" "${arguments[@]}" "${services[@]}"
    echo "$role started. Verify with: bash scripts/vm.sh $role check --project $project" ;;
  down) "${compose[@]}" down ;;
  ps) "${compose[@]}" ps -a ;;
  logs) "${compose[@]}" logs --tail 150 ;;
  check)
    "${compose[@]}" exec -T "$probe_service" "${probe[@]}"
    echo "$role VM readiness check passed." ;;
esac
