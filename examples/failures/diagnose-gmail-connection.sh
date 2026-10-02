#!/bin/sh
# Goal: learn which Gmail connection path an account can actually use, and why the others cannot.
# Status: Skrzynka development channel 0.2.x.
# Risk: read-only. One real IMAP login with the credential already stored, one authorization URL handed to Google, one delegated token mint for a Workspace address.
# Environment: macOS or Linux, Stado resolving the fleet database skrzynka, local Skarbiec owner.
# Usage: sh diagnose-gmail-connection.sh <mailbox-address> <organization-id> <oauth-callback-socket>
# Creates: nothing; no provider mutation and no credential write.
set -eu

[ "$#" -eq 3 ] || { echo "usage: $0 <mailbox-address> <organization-id> <oauth-callback-socket>" >&2; exit 64; }
ADDRESS=$1
ORGANIZATION=$2
CALLBACK=$3
SKRZYNKA_BIN=${SKRZYNKA_BIN:-target/debug/skrzynka}

[ -x "$SKRZYNKA_BIN" ] || { echo "ERROR: executable not found: $SKRZYNKA_BIN" >&2; exit 1; }

printf '%s\n' '== paths: the account-independent state of all three connection paths'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" account connection --provider gmail --bind "$CALLBACK"

printf '%s\n' '== account: every path measured for one address'
"$SKRZYNKA_BIN" --organization "$ORGANIZATION" account connection --provider gmail --bind "$CALLBACK" --email "$ADDRESS"

printf '%s\n' "Expected result: three paths, each 'usable', 'refused' or 'unproven', and 'usable_paths' counting the ones that authenticated."
printf '%s\n' "An organization with no mailbox for the address reports GMAIL_APP_PASSWORD_NOT_STORED for 'app_password'; name the organization that holds the mailbox to have its stored credential authenticated."
printf '%s\n' "Failure: 'usable_paths' of 0 means reception cannot be connected today. Each path's 'action' is the exact next step, and 'observed' carries the client IDs and console URLs somebody else may need."
printf '%s\n' 'Next: connect the usable path with examples/getting-started/add-and-sync-mailbox.sh.'
