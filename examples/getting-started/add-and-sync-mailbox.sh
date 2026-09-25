#!/bin/sh
# Goal: declare one Skarbiec item a mailbox and observe its provider mail.
# Status: Skrzynka development channel 0.2.x.
# Risk: tags the item skrzynka:mailbox in Skarbiec; local mutation plus credentialed, read-only IMAP access.
# Environment: macOS or Linux, local Skarbiec owner, isolated state directory.
# Usage: sh add-and-sync-mailbox.sh <skarbiec-item-id> <new-state-directory>
# Creates: the named directory, a Skrzynka SQLite database, and the item's skrzynka:mailbox tag; no provider mutation.
set -eu

[ "$#" -eq 2 ] || { echo "usage: $0 <skarbiec-item-id> <new-state-directory>" >&2; exit 64; }
ITEM_ID=$1
STATE_DIR=$2
SKRZYNKA_BIN=${SKRZYNKA_BIN:-target/debug/skrzynka}

[ -x "$SKRZYNKA_BIN" ] || { echo "ERROR: executable not found: $SKRZYNKA_BIN" >&2; exit 1; }
[ ! -e "$STATE_DIR" ] || { echo "ERROR: refusing to reuse state path: $STATE_DIR" >&2; exit 1; }
mkdir -m 700 "$STATE_DIR"
DATABASE="$STATE_DIR/skrzynka.db"

printf '%s\n' '== declare: tag the Skarbiec item skrzynka:mailbox and import its first INBOX page'
"$SKRZYNKA_BIN" --database "$DATABASE" mailbox declare --skarbiec-item "$ITEM_ID"

printf '%s\n' '== receive: read Skarbiec declarations again and perform one bounded IMAP synchronization'
"$SKRZYNKA_BIN" --database "$DATABASE" sync

printf '%s\n' '== observe: list normalized messages from the isolated database'
"$SKRZYNKA_BIN" --database "$DATABASE" message list --limit 20

printf '%s\n' "Expected result: at least one message when the provider inbox contains mail."
printf '%s\n' "Failure: inspect 'last_error_code' with: $SKRZYNKA_BIN --database '$DATABASE' mailbox list"
printf '%s\n' "Cleanup: rm -rf -- '$STATE_DIR'; to stop treating the item as a mailbox: skarbiec retag '$ITEM_ID' --tags <its other tags>"
printf '%s\n' 'Next: use one printed message UUID with examples/core/reply-to-message.sh.'
