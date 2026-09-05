#!/usr/bin/env sh
set -eu

role="${1:-}"
action="${2:-up}"
option="${3:-}"
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

case "$role" in
  main) directory="$root"; file="compose.yaml"; project="aml-main"; services="gateway"; probe_service="gateway"; probe="wget -q -O - http://127.0.0.1:8080/health" ;;
  tron) directory="$root/dockerizd_tron/app"; file="docker-compose.yml"; project="aml-tron"; services="tron-api tron-ingestion tron-token-metadata-worker"; probe_service="tron-api"; probe="curl --fail --silent --show-error http://127.0.0.1:4001/ready" ;;
  ethereum) directory="$root/dockerizd_ethereum"; file="docker-compose.yml"; project="aml-ethereum"; services="ethereum-api ethereum-ingestion ethereum-token-metadata ethereum-analytics"; probe_service="ethereum-api"; probe="ethereum_healthcheck" ;;
  *) echo "usage: $0 {main|tron|ethereum} {up|down|ps|logs|check} [--build|--api-only]" >&2; exit 2 ;;
esac

if [ "$option" = "--api-only" ] && [ "$role" = "tron" ]; then services="tron-api"; fi
if [ "$option" = "--api-only" ] && [ "$role" = "ethereum" ]; then services="ethereum-api"; fi
env_file="$directory/.env"
[ -f "$env_file" ] || { echo "missing $env_file; configure .env.example first" >&2; exit 1; }

offline_file=""
if [ "$option" = "--offline" ]; then
  [ "$role" = "ethereum" ] || { echo "--offline is currently supported only for ethereum" >&2; exit 2; }
  offline_file="$directory/docker-compose.offline.yml"
fi

compose() {
  if [ -n "$offline_file" ]; then
    docker compose --project-directory "$directory" --project-name "$project" --file "$directory/$file" --env-file "$env_file" --file "$offline_file" "$@"
  else
    docker compose --project-directory "$directory" --project-name "$project" --file "$directory/$file" --env-file "$env_file" "$@"
  fi
}
case "$action" in
  up) if [ "$option" = "--build" ] || [ "$option" = "--offline" ]; then compose up -d --build $services; else compose up -d $services; fi ;;
  down) compose down ;;
  ps) compose ps -a ;;
  logs) compose logs --tail 150 ;;
  check) compose exec -T "$probe_service" sh -c "$probe"; echo "$role VM readiness check passed." ;;
  *) echo "unsupported action: $action" >&2; exit 2 ;;
esac
