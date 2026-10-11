#!/bin/sh
set -eu
if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
  echo 'The host binary requires Apple Silicon macOS.' >&2
  exit 1
fi
artifact_dir=${1:?Pass the directory containing screenbridge and screenbridge.sha256}
(cd "$artifact_dir" && shasum -a 256 -c screenbridge.sha256)
prefix=${SCREENBRIDGE_PREFIX:-"$HOME/.local/share/screenbridge"}
mkdir -p "$prefix/bin" "$prefix/private"
chmod 700 "$prefix/private"
install -m 755 "$artifact_dir/screenbridge" "$prefix/bin/screenbridge"
"$prefix/bin/screenbridge" --version
