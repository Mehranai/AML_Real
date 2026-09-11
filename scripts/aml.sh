#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 {up|down|ps|logs|check} [--build] [--with-ingestion] [--with-bsc] [--gateway-only] [--pull-never] [--tron-project NAME] [--ethereum-project NAME] [--main-project NAME]"
}
if [[ ${1:-} == --help || ${1:-} == -h ]]; then usage; exit 0; fi
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
action=${1:-up}
[[ $# -eq 0 ]] || shift
build=false
with_ingestion=false
with_bsc=false
gateway_only=false
pull_never=false
# Preserve the project identities of the original single-host launcher.
tron_project=app
ethereum_project=dockerizd_ethereum
main_project=aml-whole
while [[ $# -gt 0 ]]; do
  case "$1" in
    --build) build=true; shift ;;
    --with-ingestion) with_ingestion=true; shift ;;
    --with-bsc) with_bsc=true; shift ;;
    --gateway-only) gateway_only=true; shift ;;
    --pull-never) pull_never=true; shift ;;
    --tron-project|--ethereum-project|--main-project)
      [[ $# -ge 2 && $2 =~ ^[a-z0-9][a-z0-9_-]*$ ]] || { echo "Invalid project name" >&2; exit 2; }
      case "$1" in
        --tron-project) tron_project=$2 ;;
        --ethereum-project) ethereum_project=$2 ;;
        --main-project) main_project=$2 ;;
      esac
      shift 2 ;;
    *) usage >&2; exit 2 ;;
  esac
done
case "$action" in up|down|ps|logs|check) ;; *) usage >&2; exit 2 ;; esac
if [[ $action != up ]] && { $build || $with_ingestion || $pull_never; }; then
  echo "Build, ingestion and pull options require up" >&2; exit 2
fi
roles=(tron ethereum main)
if $with_bsc; then roles=(tron ethereum bsc main); fi
if $gateway_only; then roles=(main); elif [[ $action == down ]]; then
  if $with_bsc; then roles=(main bsc ethereum tron); else roles=(main ethereum tron); fi
fi
# Validate every selected role before starting any containers.
for role in "${roles[@]}"; do
  case "$role" in
    main) directory=$root ;;
    tron) directory=$root/dockerizd_tron/app ;;
    ethereum) directory=$root/dockerizd_ethereum ;;
    bsc) directory=$root/dockerizd_bsc ;;
  esac
  [[ -f $directory/.env ]] || { echo "Configure $directory/.env first" >&2; exit 2; }
done
for role in "${roles[@]}"; do
  case "$role" in main) project=$main_project ;; tron) project=$tron_project ;; ethereum) project=$ethereum_project ;; bsc) project=dockerizd_bsc ;; esac
  arguments=("$role" "$action" --project "$project")
  if [[ $action == up ]]; then
    if $build; then arguments+=(--build); fi
    if $pull_never; then arguments+=(--pull-never); fi
    if [[ $role != main ]] && ! $with_ingestion; then arguments+=(--api-only); fi
  fi
  bash "$root/scripts/vm.sh" "${arguments[@]}"
done
