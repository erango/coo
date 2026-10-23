#!/usr/bin/env bash
# Cargo runner (macOS). The desktop app runs from inside a minimal Coo.app
# next to the binary, so dev builds show "Coo" in the menu bar and Dock, use
# the app icon and can post real notifications. Everything else (tests, the
# coo CLI) runs as-is.
set -euo pipefail
BIN="$1"; shift
if [ "$(basename "$BIN")" != "pigeon-app" ]; then
  exec "$BIN" "$@"
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$(dirname "$BIN")/Coo.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp -f "$BIN" "$APP/Contents/MacOS/Coo"
cp -f "$ROOT/crates/pigeon-bin/assets/icon/Coo.icns" "$APP/Contents/Resources/Coo.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Coo</string>
  <key>CFBundleDisplayName</key><string>Coo</string>
  <key>CFBundleIdentifier</key><string>il.co.frost-media.pigeon</string>
  <key>CFBundleExecutable</key><string>Coo</string>
  <key>CFBundleIconFile</key><string>Coo</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$APP" >/dev/null 2>&1 || true
exec "$APP/Contents/MacOS/Coo" "$@"
