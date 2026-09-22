#!/usr/bin/env bash
set -euo pipefail

source_dir=${WISENT_SOURCE_DIR:?WISENT_SOURCE_DIR is required}
output_dir=${WISENT_OUTPUT_DIR:?WISENT_OUTPUT_DIR is required}
platform=${WISENT_PLATFORM:?WISENT_PLATFORM is required}
version=${WISENT_VERSION:?WISENT_VERSION is required}
: "${CARGO_TARGET_DIR:?The builder-owned CARGO_TARGET_DIR is required}"

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
cargo build --locked --release --bin skrzynka --manifest-path "$source_dir/Cargo.toml" \
  --message-format=json-render-diagnostics | tee "$output_dir/evidence/build.jsonl"
binary=$(jq -er 'select(.reason == "compiler-artifact" and .target.name == "skrzynka" and .executable != null) | .executable' "$output_dir/evidence/build.jsonl")
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
