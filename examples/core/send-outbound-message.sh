#!/bin/sh
# Goal: originate one plain-text message from a connected mailbox and read the stored outbound row back.
# Status: Skrzynka development channel 0.2.x.
# Risk: provider-facing mutation; the SMTP server may deliver a real email.
# Environment: Stado resolving the fleet database skrzynka, with one connected mailbox of the organization; SKRZYNKA_BIN naming the skrzynka executable.
# Usage: CONFIRM_SEND=yes SKRZYNKA_BIN=<skrzynka> sh send-outbound-message.sh <organization-id> <mailbox-id-or-address> <recipient-address> <subject> <body-file>
# Creates: one SMTP submission and one durable outbound message.
set -eu

usage() { echo "usage: CONFIRM_SEND=yes SKRZYNKA_BIN=<skrzynka> $0 <organization-id> <mailbox-id-or-address> <recipient-address> <subject> <body-file>" >&2; exit 64; } # https://man.freebsd.org/cgi/man.cgi?query=sysexits EX_USAGE "The command was used incorrectly"
fail() { echo "ERROR: $*" >&2; exit 1; } # https://pubs.opengroup.org/onlinepubs/9799919799/functions/exit.html EXIT_FAILURE
[ -n "${5+set}" ] && [ -z "${6+set}" ] || usage
[ -n "${CONFIRM_SEND+set}" ] && [ "$CONFIRM_SEND" = yes ] || fail "set CONFIRM_SEND=yes after checking the sending mailbox, recipient, subject and body"
ORGANIZATION=$1
MAILBOX=$2
RECIPIENT=$3
SUBJECT=$4
BODY_FILE=$5
: "${SKRZYNKA_BIN:?set SKRZYNKA_BIN to the skrzynka executable: an installed skrzynka, or target/debug/skrzynka in a checkout}"

[ -x "$SKRZYNKA_BIN" ] || fail "executable not found: $SKRZYNKA_BIN"
[ -f "$BODY_FILE" ] || fail "message body file not found: $BODY_FILE"

printf '%s\n' '== inspect: confirm exactly one mailbox answers this selector and it is the identity you want to send as'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" mailbox list

IDEMPOTENCY_KEY=$(uuidgen | tr '[:upper:]' '[:lower:]')
printf '%s\n' '== send: originate exactly one idempotency-keyed message'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" message send \
  --mailbox "$MAILBOX" \
  --to "$RECIPIENT" \
  --subject "$SUBJECT" \
  --body-file "$BODY_FILE" \
  --idempotency-key "$IDEMPOTENCY_KEY"

printf '%s\n' '== observe: read the stored outbound rows back from the fleet database'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" message outbound --mailbox "$MAILBOX"

printf '%s\n' "Expected result: the message just sent has status 'sent' with a provider_message_id."
printf '%s\n' "Failure: a failed row retains the SMTP refusal the server itself returned; an uncertain row must be checked in provider Sent mail before any new key is used."
printf '%s\n' "Failure: 'MAILBOX_SELECTOR_AMBIGUOUS' means the address names more than one mailbox; repeat with the exact mailbox id it prints."
printf '%s\n' "Cleanup: the SMTP message cannot be recalled by Skrzynka; remove the mailbox's state only with the operations example."
printf '%s\n' 'Next: examples/operations/remove-local-mailbox.sh.'
