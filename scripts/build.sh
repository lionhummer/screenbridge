#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
  echo 'Build the Mac host binary on Apple Silicon macOS.' >&2
  exit 1
fi
cargo build --locked --release
mkdir -p artifacts
cp target/release/screenbridge artifacts/screenbridge
(cd artifacts && shasum -a 256 screenbridge > screenbridge.sha256)
