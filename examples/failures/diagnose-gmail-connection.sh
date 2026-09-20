#!/bin/sh
# Goal: learn which Gmail connection path an account can actually use, and why the others cannot.
# Status: Skrzynka development channel 0.2.x.
# Risk: read-only. One real IMAP login with the credential already stored, one authorization URL handed to Google, one delegated token mint for a Workspace address.
# Environment: macOS or Linux, local Skarbiec owner, isolated state directory.
# Usage: sh diagnose-gmail-connection.sh <mailbox-address> <new-state-directory>
# Creates: the named directory and a Skrzynka SQLite database; no provider mutation and no credential write.
set -eu

[ "$#" -eq 2 ] || { echo "usage: $0 <mailbox-address> <new-state-directory>" >&2; exit 64; }
ADDRESS=$1
STATE_DIR=$2
SKRZYNKA_BIN=${SKRZYNKA_BIN:-target/debug/skrzynka}

[ -x "$SKRZYNKA_BIN" ] || { echo "ERROR: executable not found: $SKRZYNKA_BIN" >&2; exit 1; }
[ ! -e "$STATE_DIR" ] || { echo "ERROR: refusing to reuse state path: $STATE_DIR" >&2; exit 1; }
mkdir -m 700 "$STATE_DIR"
DATABASE="$STATE_DIR/skrzynka.db"

printf '%s\n' '== paths: the account-independent state of all three connection paths'
"$SKRZYNKA_BIN" --database "$DATABASE" gmail connection

printf '%s\n' '== account: every path measured for one address'
"$SKRZYNKA_BIN" --database "$DATABASE" gmail connection --email "$ADDRESS"

printf '%s\n' "Expected result: three paths, each 'usable', 'refused' or 'unproven', and 'usable_paths' counting the ones that authenticated."
printf '%s\n' "A fresh database names no mailbox, so 'app_password' reports GMAIL_APP_PASSWORD_NOT_STORED; run this against your real database to have the stored credential authenticated."
printf '%s\n' "Failure: 'usable_paths' of 0 means reception cannot be connected today. Each path's 'action' is the exact next step, and 'observed' carries the client IDs and console URLs somebody else may need."
printf '%s\n' "Cleanup: rm -rf -- '$STATE_DIR'"
printf '%s\n' 'Next: connect the usable path with examples/getting-started/add-and-sync-mailbox.sh.'
