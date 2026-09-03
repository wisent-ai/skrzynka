#!/bin/sh
# Goal: originate one plain-text message from a connected mailbox and read the stored outbound row back.
# Status: Skrzynka development channel 0.2.x.
# Risk: provider-facing mutation; the SMTP server may deliver a real email.
# Environment: existing isolated Skrzynka state with one connected mailbox.
# Usage: CONFIRM_SEND=yes sh send-outbound-message.sh <database> <mailbox-id-or-address> <recipient-address> <subject> <body-file>
# Creates: one SMTP submission and one durable outbound message.
set -eu

[ "$#" -eq 5 ] || { echo "usage: CONFIRM_SEND=yes $0 <database> <mailbox-id-or-address> <recipient-address> <subject> <body-file>" >&2; exit 64; }
[ "${CONFIRM_SEND:-}" = yes ] || { echo "ERROR: set CONFIRM_SEND=yes after checking the sending mailbox, recipient, subject and body" >&2; exit 1; }
DATABASE=$1
MAILBOX=$2
RECIPIENT=$3
SUBJECT=$4
BODY_FILE=$5
SKRZYNKA_BIN=${SKRZYNKA_BIN:-target/debug/skrzynka}

[ -x "$SKRZYNKA_BIN" ] || { echo "ERROR: executable not found: $SKRZYNKA_BIN" >&2; exit 1; }
[ -f "$DATABASE" ] || { echo "ERROR: database not found: $DATABASE" >&2; exit 1; }
[ -f "$BODY_FILE" ] || { echo "ERROR: message body file not found: $BODY_FILE" >&2; exit 1; }

printf '%s\n' '== inspect: confirm exactly one mailbox answers this selector and it is the identity you want to send as'
"$SKRZYNKA_BIN" --database "$DATABASE" mailbox list

IDEMPOTENCY_KEY=$(uuidgen | tr '[:upper:]' '[:lower:]')
printf '%s\n' '== send: originate exactly one idempotency-keyed message'
"$SKRZYNKA_BIN" --database "$DATABASE" message send \
  --mailbox "$MAILBOX" \
  --to "$RECIPIENT" \
  --subject "$SUBJECT" \
  --body-file "$BODY_FILE" \
  --idempotency-key "$IDEMPOTENCY_KEY"

printf '%s\n' '== observe: read the stored outbound row back from the local database'
"$SKRZYNKA_BIN" --database "$DATABASE" message outbound --mailbox "$MAILBOX" --limit 5

printf '%s\n' "Expected result: status is 'sent' with a provider_message_id."
printf '%s\n' "Failure: a failed row retains the SMTP refusal the server itself returned; an uncertain row must be checked in provider Sent mail before any new key is used."
printf '%s\n' "Failure: 'MAILBOX_SELECTOR_AMBIGUOUS' means the address names more than one mailbox; repeat with the exact mailbox id it prints."
printf '%s\n' "Cleanup: the SMTP message cannot be recalled by Skrzynka; remove local state only with the operations example."
printf '%s\n' 'Next: examples/operations/remove-local-mailbox.sh.'
