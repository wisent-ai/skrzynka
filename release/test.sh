#!/usr/bin/env bash
# The post-build test a fleet builder runs: the real mailbox journey against
# the built tree. The job's PATH is minimal, so cargo is found where rustup
# installs it, exactly as release/build.sh finds it.
set -euo pipefail

if ! command -v cargo >/dev/null; then
  PATH="$HOME/.cargo/bin:$PATH"
  export PATH
fi
command -v cargo >/dev/null || {
  printf 'cargo is not installed for this builder\n' >&2
  exit 69
}
exec cargo test --locked --release --test mailboxes -- --include-ignored
