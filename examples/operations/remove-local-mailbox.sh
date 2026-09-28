#!/bin/sh
# Goal: undeclare a mailbox, inspect it, then remove only its Skrzynka state.
# Status: Skrzynka development channel 0.2.x.
# Risk: removes the skrzynka:mailbox tag in Skarbiec; destructive mutation of the fleet database: the mailbox's messages and reply evidence are deleted.
# Environment: Stado resolving the fleet database skrzynka.
# Usage: CONFIRM_REMOVE=yes sh remove-local-mailbox.sh <organization-id> <mailbox-uuid>
# Removes: the item's skrzynka:mailbox tag, one mailbox and its cascading records; the item and provider remain.
set -eu

[ "$#" -eq 2 ] || { echo "usage: CONFIRM_REMOVE=yes $0 <organization-id> <mailbox-uuid>" >&2; exit 64; }
[ "${CONFIRM_REMOVE:-}" = yes ] || { echo "ERROR: set CONFIRM_REMOVE=yes after backing up required evidence" >&2; exit 1; }
ORGANIZATION=$1
MAILBOX_ID=$2
SKRZYNKA_BIN=${SKRZYNKA_BIN:-target/debug/skrzynka}

[ -x "$SKRZYNKA_BIN" ] || { echo "ERROR: executable not found: $SKRZYNKA_BIN" >&2; exit 1; }

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
