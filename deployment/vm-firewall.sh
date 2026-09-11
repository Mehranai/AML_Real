#!/usr/bin/env bash
set -euo pipefail
# Only this project's dedicated chain is changed; existing Docker rules stay intact.
source /etc/aml-vm-firewall.env
interface=$(ip -j route get "$MAIN_IP" | jq -er '.[0].dev')
iptables -w -N AML-VM-API 2>/dev/null || iptables -w -S AML-VM-API >/dev/null
iptables -w -F AML-VM-API
iptables -w -A AML-VM-API -i "$interface" -p tcp -m conntrack --ctorigdstport "$API_PORT" ! -s "$MAIN_IP" -j DROP
iptables -w -A AML-VM-API -j RETURN
iptables -w -C DOCKER-USER -j AML-VM-API 2>/dev/null || iptables -w -I DOCKER-USER 1 -j AML-VM-API
