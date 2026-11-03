#!/usr/bin/env bash
set -euo pipefail

source_dir=${WISENT_SOURCE_DIR:?WISENT_SOURCE_DIR is required}
output_dir=${WISENT_OUTPUT_DIR:?WISENT_OUTPUT_DIR is required}
platform=${WISENT_PLATFORM:?WISENT_PLATFORM is required}
version=${WISENT_VERSION:?WISENT_VERSION is required}
export SKRZYNKA_SOURCE_REVISION=${WISENT_SOURCE_COMMIT:?WISENT_SOURCE_COMMIT is required}
: "${CARGO_TARGET_DIR:?The builder-owned CARGO_TARGET_DIR is required}"
# A fleet builder's job environment carries a minimal PATH; rustup installs
# cargo under ~/.cargo/bin, so a build that only asked PATH exited 127.
if ! command -v cargo >/dev/null; then
  PATH="$HOME/.cargo/bin:$PATH"
  export PATH
fi
command -v cargo >/dev/null || {
  printf 'cargo is not installed for this builder\n' >&2
  exit 69
}

case "$platform" in
  darwin-arm64) expected_os=Darwin; expected_arch=arm64 ;;
  linux-amd64) expected_os=Linux; expected_arch=x86_64 ;;
  *) printf 'unsupported release platform: %s\n' "$platform" >&2; exit 1 ;;
esac
if [[ "$(uname -s)" != "$expected_os" || "$(uname -m)" != "$expected_arch" ]]; then
  printf 'this builder cannot produce %s\n' "$platform" >&2
  exit 1
fi

mkdir -p "$output_dir/bin" "$output_dir/evidence"
# Through Stado's Cargo executor, which replaces the locked private Git
# package (the onboarding runtime the GitHub star request runs on) with the
# manifest's private-cargo-sources input: a worker holds no Git credential.
# The executor reads Cargo's diagnostics itself and keeps them in its
# evidence, so the binary is read where Cargo places it under the
# builder-owned target directory.
stado product cargo --manifest-path "$source_dir/Cargo.toml" build --locked --release --bin skrzynka \
  | tee "$output_dir/evidence/build.log"
binary="$CARGO_TARGET_DIR/release/skrzynka"
[[ -x "$binary" ]] || {
  printf 'the build placed no executable at %s\n' "$binary" >&2
  exit 1
}
observed_version=$("$binary" --version)
if [[ "$observed_version" != "skrzynka $version" ]]; then
  printf 'built version %s does not match declared version %s\n' "$observed_version" "$version" >&2
  exit 1
fi
install -m 0755 "$binary" "$output_dir/bin/skrzynka"
(
  cd "$output_dir"
  shasum -a 256 bin/skrzynka > evidence/DIGESTS
)
