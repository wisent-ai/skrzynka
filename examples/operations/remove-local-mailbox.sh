#!/bin/sh
# Goal: undeclare a mailbox, inspect it, then remove only its Skrzynka state.
# Status: Skrzynka development channel 0.2.x.
# Risk: removes the skrzynka:mailbox tag in Skarbiec; destructive mutation of the fleet database: the mailbox's messages and reply evidence are deleted.
# Environment: Stado resolving the fleet database skrzynka; SKRZYNKA_BIN naming the skrzynka executable.
# Usage: CONFIRM_REMOVE=yes SKRZYNKA_BIN=<skrzynka> sh remove-local-mailbox.sh <organization-id> <mailbox-uuid>
# Removes: the item's skrzynka:mailbox tag, one mailbox and its cascading records; the item and provider remain.
set -eu

usage() { echo "usage: CONFIRM_REMOVE=yes SKRZYNKA_BIN=<skrzynka> $0 <organization-id> <mailbox-uuid>" >&2; exit 64; } # https://man.freebsd.org/cgi/man.cgi?query=sysexits EX_USAGE "The command was used incorrectly"
fail() { echo "ERROR: $*" >&2; exit 1; } # https://pubs.opengroup.org/onlinepubs/9799919799/functions/exit.html EXIT_FAILURE
[ -n "${2+set}" ] && [ -z "${3+set}" ] || usage
[ -n "${CONFIRM_REMOVE+set}" ] && [ "$CONFIRM_REMOVE" = yes ] || fail "set CONFIRM_REMOVE=yes after backing up required evidence"
ORGANIZATION=$1
MAILBOX_ID=$2
: "${SKRZYNKA_BIN:?set SKRZYNKA_BIN to the skrzynka executable: an installed skrzynka, or target/debug/skrzynka in a checkout}"

[ -x "$SKRZYNKA_BIN" ] || fail "executable not found: $SKRZYNKA_BIN"

printf '%s\n' '== undeclare: remove the skrzynka:mailbox tag so polling stops and removal is allowed'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" mailbox undeclare "$MAILBOX_ID"
printf '%s\n' '== inspect: record the exact resource before deletion'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" mailbox show "$MAILBOX_ID"
printf '%s\n' '== remove: delete the mailbox and its child records'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" mailbox remove "$MAILBOX_ID" --confirm
printf '%s\n' 'Expected result: removed contains the mailbox UUID; listing no longer contains it.'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" mailbox list
printf '%s\n' 'Recovery: declare the same exact Skarbiec item again and synchronize provider mail; deleted reply evidence is recoverable only from a database backup.'
printf '%s\n' 'Off-switch: the provider needs no cleanup; the Skarbiec item keeps every value and every tag except skrzynka:mailbox.'
