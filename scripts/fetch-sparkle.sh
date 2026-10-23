#!/usr/bin/env bash
# Fetch the pinned Sparkle release into vendor/Sparkle-<version> (gitignored)
# and print its path. Verified by sha256; downloads only when missing.
set -euo pipefail
VERSION="2.10.0"
SHA256="c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/vendor/Sparkle-$VERSION"
if [ ! -d "$DEST/Sparkle.framework" ]; then
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  curl -fsSL -o "$TMP/sparkle.tar.xz" \
    "https://github.com/sparkle-project/Sparkle/releases/download/$VERSION/Sparkle-$VERSION.tar.xz"
  GOT="$(shasum -a 256 "$TMP/sparkle.tar.xz" | awk '{print $1}')"
  if [ "$GOT" != "$SHA256" ]; then
    echo "Sparkle $VERSION checksum mismatch: $GOT" >&2
    exit 1
  fi
  mkdir -p "$DEST"
  tar -xJf "$TMP/sparkle.tar.xz" -C "$DEST"
fi
echo "$DEST"
