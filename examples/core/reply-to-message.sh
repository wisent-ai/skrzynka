#!/bin/sh
# Goal: send one plain-text reply through the mailbox that received a stored message.
# Status: Skrzynka development channel 0.2.x.
# Risk: provider-facing mutation; the SMTP server may deliver a real email.
# Environment: existing isolated Skrzynka state with one imported message.
# Usage: CONFIRM_SEND=yes sh reply-to-message.sh <database> <message-uuid> <body-file>
# Creates: one SMTP submission and one durable reply attempt.
set -eu

[ "$#" -eq 3 ] || { echo "usage: CONFIRM_SEND=yes $0 <database> <message-uuid> <body-file>" >&2; exit 64; }
[ "${CONFIRM_SEND:-}" = yes ] || { echo "ERROR: set CONFIRM_SEND=yes after checking the recipient and body" >&2; exit 1; }
DATABASE=$1
MESSAGE_ID=$2
BODY_FILE=$3
SKRZYNKA_BIN=${SKRZYNKA_BIN:-target/debug/skrzynka}

[ -x "$SKRZYNKA_BIN" ] || { echo "ERROR: executable not found: $SKRZYNKA_BIN" >&2; exit 1; }
[ -f "$DATABASE" ] || { echo "ERROR: database not found: $DATABASE" >&2; exit 1; }
[ -f "$BODY_FILE" ] || { echo "ERROR: reply body file not found: $BODY_FILE" >&2; exit 1; }

printf '%s\n' '== inspect: confirm source mailbox, reply target, subject, and original body'
"$SKRZYNKA_BIN" --database "$DATABASE" message show "$MESSAGE_ID"

IDEMPOTENCY_KEY=$(uuidgen | tr '[:upper:]' '[:lower:]')
printf '%s\n' '== send: submit exactly one idempotency-keyed reply'
"$SKRZYNKA_BIN" --database "$DATABASE" message reply "$MESSAGE_ID" \
  --body-file "$BODY_FILE" --idempotency-key "$IDEMPOTENCY_KEY"

printf '%s\n' "Expected result: status is 'sent' with a provider_message_id."
printf '%s\n' "Failure: a failed attempt is retained; an uncertain attempt must be checked in provider Sent mail before any new key is used."
printf '%s\n' "Cleanup: the SMTP message cannot be recalled by Skrzynka; remove local state only with the operations example."
printf '%s\n' 'Next: examples/operations/remove-local-mailbox.sh.'
