#!/bin/sh
# Goal: list the mailing lists one mailbox receives and ask the list one message came from to stop sending.
# Status: Skrzynka development channel 0.2.x.
# Risk: list-facing request; a one-click POST or an unsubscribe mail reaches the list's operator.
# Environment: Stado resolving the fleet database skrzynka, with one connected mailbox of the organization; SKRZYNKA_BIN naming the skrzynka executable.
# Usage: CONFIRM_UNSUBSCRIBE=yes SKRZYNKA_BIN=<skrzynka> sh leave-mailing-list.sh <organization-id> <mailbox-id> <message-id>
# Creates: one recorded unsubscribe attempt and, for a mailto list, one sent mail.
set -eu

usage() { echo "usage: CONFIRM_UNSUBSCRIBE=yes SKRZYNKA_BIN=<skrzynka> $0 <organization-id> <mailbox-id> <message-id>" >&2; exit 64; } # https://man.freebsd.org/cgi/man.cgi?query=sysexits EX_USAGE "The command was used incorrectly"
fail() { echo "ERROR: $*" >&2; exit 1; } # https://pubs.opengroup.org/onlinepubs/9799919799/functions/exit.html EXIT_FAILURE
[ -n "${3+set}" ] && [ -z "${4+set}" ] || usage
ORGANIZATION=$1
MAILBOX=$2
MESSAGE=$3
: "${SKRZYNKA_BIN:?set SKRZYNKA_BIN to the skrzynka executable: an installed skrzynka, or target/debug/skrzynka in a checkout}"

[ -x "$SKRZYNKA_BIN" ] || fail "executable not found: $SKRZYNKA_BIN"

printf '%s\n' '== inspect: the lists this mailbox receives, with their ways out and last request'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" message subscriptions --mailbox "$MAILBOX"

[ -n "${CONFIRM_UNSUBSCRIBE+set}" ] && [ "$CONFIRM_UNSUBSCRIBE" = yes ] || fail "set CONFIRM_UNSUBSCRIBE=yes after checking the list and the message above"

IDEMPOTENCY_KEY=$(uuidgen | tr '[:upper:]' '[:lower:]')
printf '%s\n' '== unsubscribe: ask the list exactly once, claimed by one idempotency key'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" message unsubscribe "$MESSAGE" \
  --idempotency-key "$IDEMPOTENCY_KEY"

printf '%s\n' "Expected result: the attempt has status 'sent', method 'one_click' or 'mailto', and the list's address as target."
printf '%s\n' "Failure: 'UNSUBSCRIBE_NEEDS_BROWSER' names the pages to open yourself; 'UNSUBSCRIBE_UNAVAILABLE' means the message names no way out."
printf '%s\n' "Failure: an 'uncertain' attempt must be checked in the mailbox's Sent mail before asking again with a new key."
