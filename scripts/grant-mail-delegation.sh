#!/bin/sh
# One-time Google Workspace domain-wide delegation grant, performed on this
# machine by Weles.
#
# The grant exists only in the Workspace admin console, so it is browser work
# and Weles owns it. Weles launches only the immutable browser release its
# deployment selected, and it reads that coordinate from the environment; the
# authoritative copy of the coordinate is the release receipt the download
# script wrote after verifying the archive checksum. This script reads the
# receipt, states the coordinate, and hands over to Skrzynka, which mints a
# delegated token, calls Weles when Google says the client is not authorized
# yet, and retries the mint afterwards.
#
# Usage: scripts/grant-mail-delegation.sh user@workspace-domain
set -eu

ADDRESS=${1:-}
if [ -z "$ADDRESS" ]; then
    printf 'usage: %s <workspace-address>\n' "$0" >&2
    exit 2
fi

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
CHROMIUM_ROOT=${WELES_CHROMIUM_DIR:-$HOME/.local/share/weles-chromium}

RECEIPT=
for candidate in "$CHROMIUM_ROOT"/*/.weles-release; do
    [ -f "$candidate" ] || continue
    RECEIPT=$candidate
    break
done
if [ -z "$RECEIPT" ]; then
    printf 'No verified Weles Chromium release receipt under %s.\n' "$CHROMIUM_ROOT" >&2
    printf 'Install the deployment-selected release first; Weles refuses any other binary.\n' >&2
    exit 1
fi

RELEASE_DIR=$(dirname -- "$RECEIPT")
WELES_CHROMIUM_RELEASE_VERSION=$(basename -- "$RELEASE_DIR")
WELES_CHROMIUM_RELEASE_SHA256=$(sed -n 's/^archive_sha256=//p' "$RECEIPT")
if [ -z "$WELES_CHROMIUM_RELEASE_SHA256" ]; then
    printf 'Release receipt %s carries no archive_sha256.\n' "$RECEIPT" >&2
    exit 1
fi
export WELES_CHROMIUM_RELEASE_VERSION WELES_CHROMIUM_RELEASE_SHA256

SKRZYNKA_BIN=${SKRZYNKA_BIN:-}
if [ -z "$SKRZYNKA_BIN" ]; then
    if [ -x "$ROOT/target/release/skrzynka" ]; then
        SKRZYNKA_BIN=$ROOT/target/release/skrzynka
    else
        SKRZYNKA_BIN=skrzynka
    fi
fi

printf 'Weles release %s\n' "$WELES_CHROMIUM_RELEASE_VERSION" >&2
printf 'Granting domain-wide delegation for %s\n' "$ADDRESS" >&2
exec "$SKRZYNKA_BIN" gmail delegate --email "$ADDRESS" --authorize
