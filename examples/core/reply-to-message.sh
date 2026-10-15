#!/bin/sh
# Goal: send one plain-text reply through the mailbox that received a stored message.
# Status: Skrzynka development channel 0.2.x.
# Risk: provider-facing mutation; the SMTP server may deliver a real email.
# Environment: Stado resolving the fleet database skrzynka, with one imported message of the organization; SKRZYNKA_BIN naming the skrzynka executable.
# Usage: CONFIRM_SEND=yes SKRZYNKA_BIN=<skrzynka> sh reply-to-message.sh <organization-id> <message-uuid> <body-file>
# Creates: one SMTP submission and one durable reply attempt.
set -eu

usage() { echo "usage: CONFIRM_SEND=yes SKRZYNKA_BIN=<skrzynka> $0 <organization-id> <message-uuid> <body-file>" >&2; exit 64; } # https://man.freebsd.org/cgi/man.cgi?query=sysexits EX_USAGE "The command was used incorrectly"
fail() { echo "ERROR: $*" >&2; exit 1; } # https://pubs.opengroup.org/onlinepubs/9799919799/functions/exit.html EXIT_FAILURE
[ -n "${3+set}" ] && [ -z "${4+set}" ] || usage
[ -n "${CONFIRM_SEND+set}" ] && [ "$CONFIRM_SEND" = yes ] || fail "set CONFIRM_SEND=yes after checking the recipient and body"
ORGANIZATION=$1
MESSAGE_ID=$2
BODY_FILE=$3
: "${SKRZYNKA_BIN:?set SKRZYNKA_BIN to the skrzynka executable: an installed skrzynka, or target/debug/skrzynka in a checkout}"

[ -x "$SKRZYNKA_BIN" ] || fail "executable not found: $SKRZYNKA_BIN"
[ -f "$BODY_FILE" ] || fail "reply body file not found: $BODY_FILE"

printf '%s\n' '== inspect: confirm source mailbox, reply target, subject, and original body'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" message show "$MESSAGE_ID"

IDEMPOTENCY_KEY=$(uuidgen | tr '[:upper:]' '[:lower:]')
printf '%s\n' '== send: submit exactly one idempotency-keyed reply'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" message reply "$MESSAGE_ID" \
  --body-file "$BODY_FILE" --idempotency-key "$IDEMPOTENCY_KEY"

printf '%s\n' "Expected result: status is 'sent' with a provider_message_id."
printf '%s\n' "Failure: a failed attempt is retained; an uncertain attempt must be checked in provider Sent mail before any new key is used."
printf '%s\n' "Cleanup: the SMTP message cannot be recalled by Skrzynka; remove the mailbox's state only with the operations example."
printf '%s\n' 'Next: examples/operations/remove-local-mailbox.sh.'
