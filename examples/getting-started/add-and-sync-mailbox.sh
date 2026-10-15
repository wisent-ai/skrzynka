#!/bin/sh
# Goal: declare one Skarbiec item a mailbox and observe its provider mail.
# Status: Skrzynka development channel 0.2.x.
# Risk: tags the item skrzynka:mailbox in Skarbiec; writes mailbox and message rows to the fleet database plus credentialed, read-only IMAP access.
# Environment: macOS or Linux, Stado resolving the fleet database skrzynka, local Skarbiec owner; SKRZYNKA_BIN naming the skrzynka executable.
# Usage: SKRZYNKA_BIN=<skrzynka> sh add-and-sync-mailbox.sh <skarbiec-item-id> <organization-id>
# Creates: the mailbox and its messages under the organization in the fleet database, and the item's skrzynka:mailbox tag; no provider mutation.
set -eu

usage() { echo "usage: SKRZYNKA_BIN=<skrzynka> $0 <skarbiec-item-id> <organization-id>" >&2; exit 64; } # https://man.freebsd.org/cgi/man.cgi?query=sysexits EX_USAGE "The command was used incorrectly"
fail() { echo "ERROR: $*" >&2; exit 1; } # https://pubs.opengroup.org/onlinepubs/9799919799/functions/exit.html EXIT_FAILURE
[ -n "${2+set}" ] && [ -z "${3+set}" ] || usage
ITEM_ID=$1
ORGANIZATION=$2
: "${SKRZYNKA_BIN:?set SKRZYNKA_BIN to the skrzynka executable: an installed skrzynka, or target/debug/skrzynka in a checkout}"

[ -x "$SKRZYNKA_BIN" ] || fail "executable not found: $SKRZYNKA_BIN"

printf '%s\n' '== declare: tag the Skarbiec item skrzynka:mailbox and import every INBOX message'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" mailbox declare --skarbiec-item "$ITEM_ID"

printf '%s\n' '== receive: read Skarbiec declarations again and import whatever arrived past each cursor'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" sync

printf '%s\n' "== observe: list the organization's normalized messages, newest first"
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" message list

printf '%s\n' "Expected result: at least one message when the provider inbox contains mail."
printf '%s\n' "Failure: inspect 'last_error_code' with: $SKRZYNKA_BIN --organization '$ORGANIZATION' mailbox list"
printf '%s\n' "Cleanup: CONFIRM_REMOVE=yes SKRZYNKA_BIN='$SKRZYNKA_BIN' sh examples/operations/remove-local-mailbox.sh '$ORGANIZATION' <mailbox-uuid>"
printf '%s\n' 'Next: use one printed message UUID with examples/core/reply-to-message.sh.'
