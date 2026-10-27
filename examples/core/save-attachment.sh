#!/bin/sh
# Goal: list one stored message's attachments and save one of them to a new file.
# Status: Skrzynka development channel 0.2.x.
# Risk: read-only at the provider; the message is read with BODY.PEEK[] and is not marked seen.
# Environment: Stado resolving the fleet database skrzynka, with one connected mailbox of the organization; SKRZYNKA_BIN naming the skrzynka executable.
# Usage: SKRZYNKA_BIN=<skrzynka> sh save-attachment.sh <organization-id> <message-id> <index> <output-path>
# Creates: one new file at <output-path>; an existing file is never overwritten.
set -eu

usage() { echo "usage: SKRZYNKA_BIN=<skrzynka> $0 <organization-id> <message-id> <index> <output-path>" >&2; exit 64; } # https://man.freebsd.org/cgi/man.cgi?query=sysexits EX_USAGE "The command was used incorrectly"
fail() { echo "ERROR: $*" >&2; exit 1; } # https://pubs.opengroup.org/onlinepubs/9799919799/functions/exit.html EXIT_FAILURE
[ -n "${4+set}" ] && [ -z "${5+set}" ] || usage
ORGANIZATION=$1
MESSAGE=$2
INDEX=$3
OUTPUT=$4
: "${SKRZYNKA_BIN:?set SKRZYNKA_BIN to the skrzynka executable: an installed skrzynka, or target/debug/skrzynka in a checkout}"

[ -x "$SKRZYNKA_BIN" ] || fail "executable not found: $SKRZYNKA_BIN"

printf '%s\n' '== inspect: the files this message carries, each with its index'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" message attachment list "$MESSAGE"

printf '%s\n' '== save: read the attachment from the provider and write it to a new file'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" message attachment save "$MESSAGE" \
  --index "$INDEX" \
  --output "$OUTPUT"

printf '%s\n' "Expected result: the printed size_bytes and sha256 describe the file at $OUTPUT."
printf '%s\n' "Failure: 'ATTACHMENT_NOT_FOUND' names the indexes the message has; 'ATTACHMENT_OUTPUT_EXISTS' means the path is taken."
printf '%s\n' "Failure: 'MESSAGE_SOURCE_GONE' means the provider no longer holds the message in INBOX; its stored text remains."
printf '%s\n' "Cleanup: remove $OUTPUT when it is no longer needed; nothing changed at the provider."
