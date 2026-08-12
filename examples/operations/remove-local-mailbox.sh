#!/bin/sh
# Goal: disable a mailbox, inspect it, then remove only its local Skrzynka state.
# Status: Skrzynka development channel 0.2.x.
# Risk: destructive local mutation; local messages and reply evidence are deleted.
# Environment: stopped loopback service or a database not used by another process.
# Usage: CONFIRM_REMOVE=yes sh remove-local-mailbox.sh <database> <mailbox-uuid>
# Removes: one local mailbox and cascading local records; Skarbiec/provider remain unchanged.
set -eu

[ "$#" -eq 2 ] || { echo "usage: CONFIRM_REMOVE=yes $0 <database> <mailbox-uuid>" >&2; exit 64; }
[ "${CONFIRM_REMOVE:-}" = yes ] || { echo "ERROR: set CONFIRM_REMOVE=yes after backing up required local evidence" >&2; exit 1; }
DATABASE=$1
MAILBOX_ID=$2
SKRZYNKA_BIN=${SKRZYNKA_BIN:-target/debug/skrzynka}

[ -x "$SKRZYNKA_BIN" ] || { echo "ERROR: executable not found: $SKRZYNKA_BIN" >&2; exit 1; }
[ -f "$DATABASE" ] || { echo "ERROR: database not found: $DATABASE" >&2; exit 1; }

printf '%s\n' '== disable: stop future poll selection for this mailbox'
"$SKRZYNKA_BIN" --database "$DATABASE" mailbox disable "$MAILBOX_ID"
printf '%s\n' '== inspect: record the exact local resource before deletion'
"$SKRZYNKA_BIN" --database "$DATABASE" mailbox show "$MAILBOX_ID"
printf '%s\n' '== remove: delete the local mailbox and its local child records'
"$SKRZYNKA_BIN" --database "$DATABASE" mailbox remove "$MAILBOX_ID" --confirm
printf '%s\n' 'Expected result: removed contains the mailbox UUID; listing no longer contains it.'
"$SKRZYNKA_BIN" --database "$DATABASE" mailbox list
printf '%s\n' 'Recovery: re-add the same exact Skarbiec item and synchronize provider mail; deleted local reply evidence is recoverable only from a database backup.'
printf '%s\n' 'Off-switch: provider and Skarbiec need no cleanup because this operation never changed them.'
