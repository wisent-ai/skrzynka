#!/bin/sh
# Author (or re-author) this product's landing content plan through Brama.
#
# Model access follows the fleet contract: the Brama stable-port adapter on
# 127.0.0.1:17601 (stado resolver) and the operations grant read from the
# fleet Skarbiec vault at run time — the token never appears in argv or in a
# checked-in file. Usage:
#
#   scripts/author-landing-plan.sh [attempts]
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
attempts=${1:-12}
: "${SKARBIEC_VAULT_FILE:=$HOME/.stado/skarbiec.vault.json}"
export SKARBIEC_VAULT_FILE
MODEL_ROUTER_TOKEN=$(skarbiec get brama-operations-model-router | /usr/bin/python3 -c 'import json,sys; print(json.load(sys.stdin)["fields"]["token"])')
export MODEL_ROUTER_TOKEN
export MODEL_ROUTER_URL=${MODEL_ROUTER_URL:-http://127.0.0.1:17601}
export MODEL_ROUTER_MODEL=${MODEL_ROUTER_MODEL:-weles/agent/primary}
# The routed deployment is shared with fleet agents and refuses under load;
# a refused run is retried whole, bounded, with a pause between runs.
runs=${PLAN_RUNS:-6}
i=1
while :; do
  if node "${LANDING_CLI:-../landing-cli/src/cli.js}" plan \
    --brief landing.brief.json \
    --components landing.components.json \
    --out landing.plan.json \
    --attempts "$attempts"; then
    exit 0
  fi
  [ "$i" -ge "$runs" ] && exit 1
  i=$((i+1))
  sleep 30
done
