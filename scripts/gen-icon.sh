#!/usr/bin/env bash
# Regenerate the app icon from the design master (design/logo/app-icon.svg):
#   crates/pigeon-bin/assets/icon/Coo.icns  (bundle icon)
#   crates/pigeon-bin/assets/icon/icon_512.png (runtime dock/window icon)
# macOS only: rasterizes with AppKit (transparent corners), then iconutil.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SVG="$ROOT/design/logo/app-icon.svg"
OUT="$ROOT/crates/pigeon-bin/assets/icon"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$OUT"

echo "==> rasterizing $SVG"
swift "$ROOT/scripts/svg2png.swift" "$SVG" "$TMP/icon_1024.png" 1024

ISET="$TMP/Coo.iconset"
mkdir -p "$ISET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$TMP/icon_1024.png" --out "$ISET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" "$TMP/icon_1024.png" --out "$ISET/icon_${s}x${s}@2x.png" >/dev/null
done

iconutil -c icns "$ISET" -o "$OUT/Coo.icns"
cp "$ISET/icon_512x512.png" "$OUT/icon_512.png"
echo "==> wrote $OUT/Coo.icns and icon_512.png"
