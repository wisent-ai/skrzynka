#!/bin/sh
# Goal: add one Skarbiec mailbox through the loopback API and read synchronized messages.
# Status: Skrzynka development channel 0.1.x.
# Risk: local mutation plus credentialed, read-only IMAP access.
# Environment: a running loopback Skrzynka service, curl, and jq.
# Usage: sh use-loopback-api.sh <skarbiec-item-id> [http://127.0.0.1:8787]
# Creates: one local mailbox and normalized message rows; no provider mutation.
set -eu

[ "$#" -ge 1 ] && [ "$#" -le 2 ] || { echo "usage: $0 <skarbiec-item-id> [api-url]" >&2; exit 64; }
command -v curl >/dev/null 2>&1 || { echo "ERROR: curl is required" >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "ERROR: jq is required" >&2; exit 1; }
ITEM_ID=$1
API_URL=${2:-http://127.0.0.1:8787}
case "$API_URL" in
  http://127.0.0.1:*|http://localhost:*) ;;
  *) echo "ERROR: API URL must be loopback HTTP" >&2; exit 1 ;;
esac

printf '%s\n' '== service: read health without mutating state'
curl --fail --silent --show-error "$API_URL/healthz"
printf '\n'

printf '%s\n' '== add: send only the exact item reference; the API has no password field'
REQUEST=$(jq -cn --arg item "$ITEM_ID" '{skarbiec_item_id:$item}')
MAILBOX=$(
  curl --fail --silent --show-error \
    --header 'content-type: application/json' \
    --data "$REQUEST" \
    "$API_URL/v1/mailboxes"
)
printf '%s\n' "$MAILBOX"
MAILBOX_ID=$(printf '%s' "$MAILBOX" | jq -er '.id')
[ -n "$MAILBOX_ID" ] || { echo "ERROR: mailbox response contained no id" >&2; exit 1; }

printf '%s\n' '== receive: synchronize the selected mailbox'
curl --fail --silent --show-error --request POST "$API_URL/v1/mailboxes/$MAILBOX_ID/sync"
printf '\n'
printf '%s\n' '== observe: read its bounded normalized inbox'
curl --fail --silent --show-error "$API_URL/v1/messages?mailbox_id=$MAILBOX_ID&limit=20"
printf '\n'

printf '%s\n' 'Expected result: a ready health response, one mailbox UUID, and messages when provider INBOX contains mail.'
printf '%s\n' "Failure: GET $API_URL/v1/mailboxes/$MAILBOX_ID exposes the normalized mailbox error without credentials."
printf '%s\n' "Cleanup: curl --fail --request DELETE '$API_URL/v1/mailboxes/$MAILBOX_ID?confirm=true'"
printf '%s\n' 'Next: POST one idempotency-keyed reply using docs/API.md after inspecting the real recipient.'
