#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 {main|tron|ethereum|bsc} {up|down|ps|logs|check|check-runtime|pause|pause-ingestion|resume|db} [--build] [--api-only] [--offline] [--pull-never] [--project NAME] [--query SQL]"
  echo "Chain controls: pause stops apps only; pause-ingestion stops background workers; resume starts apps without touching database containers; db is read-only."
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
query=
query_set=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --build) build=true; shift ;;
    --api-only) api_only=true; shift ;;
    --offline) offline=true; build=true; shift ;;
    --pull-never) pull_never=true; shift ;;
    --query)
      [[ $# -ge 2 && -n $2 ]] || fail "--query requires SQL"
      query=$2; query_set=true; shift 2 ;;
    --project)
      [[ $# -ge 2 && $2 =~ ^[a-z0-9][a-z0-9_-]*$ ]] || fail "--project requires a valid Compose project name"
      project_override=$2; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) fail "Unknown option: $1" ;;
  esac
done
case "$action" in up|down|ps|logs|check|check-runtime|pause|pause-ingestion|resume|db) ;; *) fail "Unknown action: $action" ;; esac
[[ $query_set == false || $action == db ]] || fail "--query is valid only with db"
case "$action" in
  pause|pause-ingestion|resume|db) [[ $role != main ]] || fail "$action requires a chain role" ;;
esac
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
    database=tron_db; workers=(tron-ingestion tron-token-metadata-worker tron-analytics)
    services=(tron-api tron-ingestion tron-token-metadata-worker tron-analytics); probe_service=tron-api
    probe=(curl --fail --silent --show-error http://127.0.0.1:4001/ready)
    if $api_only; then services=(tron-api); fi ;;
  ethereum)
    directory=$root/dockerizd_ethereum; file=docker-compose.yml; project=aml-ethereum
    database=ethereum_aml; workers=(ethereum-ingestion ethereum-token-metadata ethereum-analytics)
    services=(ethereum-api ethereum-ingestion ethereum-token-metadata ethereum-analytics); probe_service=ethereum-api
    probe=(ethereum_healthcheck)
    if $api_only; then services=(ethereum-api); fi ;;
  bsc)
    directory=$root/dockerizd_bsc; file=docker-compose.yml; project=aml-bsc
    database=bsc_aml; workers=(bsc-follow bsc-token-metadata)
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
if [[ $role == bsc ]]; then compose+=(--profile runtime); fi
if $offline; then compose+=(--file "$directory/docker-compose.offline.yml"); fi
db_client() {
  local options=(exec)
  if [[ ! -t 0 || ! -t 1 || $# -gt 0 ]]; then options+=(-T); fi
  # Credentials are expanded inside the existing container, not by the host shell.
  "${compose[@]}" "${options[@]}" clickhouse sh -c '
    database=$1
    shift
    exec clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" \
      --database "$database" --max_execution_time 30 --max_result_rows 1000 \
      --result_overflow_mode break --readonly 1 "$@"
  ' aml-db "$database" "$@"
}
case "$action" in
  up)
    arguments=(up -d)
    if $build; then arguments+=(--build); else arguments+=(--no-build); fi
    if $pull_never; then arguments+=(--pull never); fi
    "${compose[@]}" "${arguments[@]}" "${services[@]}"
    echo "$role started. Verify with: bash scripts/vm.sh $role check --project $project" ;;
  down) "${compose[@]}" down ;;
  pause)
    "${compose[@]}" stop --timeout 150 "${services[@]}"
    echo "$role apps stopped. VM, ClickHouse and database volumes are retained." ;;
  pause-ingestion)
    "${compose[@]}" stop --timeout 150 "${workers[@]}"
    echo "$role background workers stopped. API and ClickHouse were not stopped." ;;
  resume)
    db_client --query 'SELECT 1' >/dev/null
    "${compose[@]}" up -d --no-deps --no-build --pull never "${services[@]}"
    echo "$role apps started using existing images and checkpoints. Database containers were not restarted." ;;
  db)
    if $query_set; then db_client --query "$query"; else db_client; fi ;;
  ps) "${compose[@]}" ps -a ;;
  logs) "${compose[@]}" logs --tail 150 ;;
  check|check-runtime)
    if [[ $action == check-runtime ]]; then
      runtime_services=(clickhouse "${services[@]}")
      if [[ $role == main ]]; then runtime_services=(gateway analytical-node neo4j); fi
      for service in "${runtime_services[@]}"; do
        id=$("${compose[@]}" ps --all --quiet "$service")
        [[ $id =~ ^[a-f0-9]+$ ]] || fail "$role/$service is missing or has multiple replicas; inspect compose ps"
        state=$(docker inspect --format '{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$id")
        case "$state" in
          'running healthy'|'running none') ;;
          *) fail "$role/$service is not ready ($state); inspect its logs" ;;
        esac
      done
    fi
    "${compose[@]}" exec -T "$probe_service" "${probe[@]}"
    echo "$role $action passed. Process/API health does not prove complete or current blockchain coverage." ;;
esac
