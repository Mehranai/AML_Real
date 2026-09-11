#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 [--main-url URL] [--user USER] [--tron-address ADDRESS] [--tron-path-target ADDRESS] [--ethereum-address ADDRESS] [--ethereum-path-target ADDRESS] [--bsc-address ADDRESS] [--bsc-path-target ADDRESS]"
}
main_url=http://127.0.0.1:8080
user=
tron_address=
tron_target=
ethereum_address=
ethereum_target=
bsc_address=
bsc_target=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h) usage; exit 0 ;;
    --main-url|--user|--tron-address|--tron-path-target|--ethereum-address|--ethereum-path-target|--bsc-address|--bsc-path-target)
      [[ $# -ge 2 && -n $2 && $2 != --* ]] || { usage >&2; exit 2; }
      case "$1" in
        --main-url) main_url=${2%/} ;;
        --user) user=$2 ;;
        --tron-address) tron_address=$2 ;;
        --tron-path-target) tron_target=$2 ;;
        --ethereum-address) ethereum_address=$2 ;;
        --ethereum-path-target) ethereum_target=$2 ;;
        --bsc-address) bsc_address=$2 ;;
        --bsc-path-target) bsc_target=$2 ;;
      esac
      shift 2 ;;
    *) usage >&2; exit 2 ;;
  esac
done
[[ $main_url =~ ^https?:// ]] || { echo "Expected an HTTP(S) main URL" >&2; exit 2; }
[[ -z $tron_target || -n $tron_address ]] || { echo "TRON target requires source address" >&2; exit 2; }
[[ -z $ethereum_target || -n $ethereum_address ]] || { echo "Ethereum target requires source address" >&2; exit 2; }
[[ -z $bsc_target || -n $bsc_address ]] || { echo "BSC target requires source address" >&2; exit 2; }
for tool in curl jq; do command -v "$tool" >/dev/null || { echo "Required: $tool" >&2; exit 1; }; done
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
curl_args=(--fail --silent --show-error --connect-timeout 10 --max-time 310 --cookie "$work/cookies" --cookie-jar "$work/cookies")
if [[ -n $user ]]; then
  [[ $user != *:* && $user != *$'\n'* && $user != *$'\r'* ]] || { echo "Invalid user name" >&2; exit 2; }
  read -r -s -p "Password for $user: " password
  echo >&2
  credential=$user:$password
  credential=${credential//\\/\\\\}
  credential=${credential//\"/\\\"}
  [[ $credential != *$'\r'* ]] || { echo "Invalid password" >&2; exit 2; }
  printf 'user = "%s"\n' "$credential" > "$work/curl.conf"
  chmod 600 "$work/curl.conf"
  unset credential password
  curl_args+=(--config "$work/curl.conf")
fi
get() {
  curl "${curl_args[@]}" --output "$work/response.json" "$main_url$1"
  jq -e 'type == "object"' "$work/response.json" >/dev/null
  echo "PASS GET $1"
}
encode() { jq -nr --arg value "$1" '$value | @uri'; }
get /health
jq -e '.status == "alive"' "$work/response.json" >/dev/null
get /ready
jq -e '.dependencies.neo4j == "ready"' "$work/response.json" >/dev/null
networks=(tron ethereum)
if [[ -n $bsc_address ]]; then networks+=(bsc); fi
for network in "${networks[@]}"; do
  get "/networks/$network/ready"
  jq -e '.status == "ready"' "$work/response.json" >/dev/null
done
for network in "${networks[@]}"; do
  case "$network" in
    tron) address=$tron_address; target=$tron_target; depth=max_depth ;;
    ethereum) address=${ethereum_address,,}; target=${ethereum_target,,}; depth=max_hops ;;
    bsc) address=${bsc_address,,}; target=${bsc_target,,}; depth=max_hops ;;
  esac
  [[ -n $address ]] || continue
  source_uri=$(encode "$address")
  get "/api/$network/wallet/$source_uri/investigation"
  jq -e --arg address "$address" '.address == $address' "$work/response.json" >/dev/null
  jq -e '.investigation.state == "temporary" and .risk_engine.probability_claimed == false' "$work/response.json" >/dev/null
  if [[ -n $target ]]; then
    target_uri=$(encode "$target")
    get "/api/$network/wallet/$source_uri/paths/$target_uri?$depth=10"
    jq -e '.paths | type == "array"' "$work/response.json" >/dev/null
  fi
done
echo "AML multi-VM smoke test completed successfully."
