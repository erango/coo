#!/usr/bin/env bash
# Build dist/Pigeon.app (macOS), no Xcode needed.
#   ./scripts/bundle.sh            release build
#   ./scripts/bundle.sh --debug    debug build (faster)
#   ./scripts/bundle.sh --open     launch it when done
# The `pigeon` CLI ships inside the bundle at Contents/Helpers/pigeon (not
# MacOS/: the default case-insensitive disk would make it the same file as
# MacOS/Pigeon) so agent setup can point at a stable path. Signing is ad-hoc for now; Developer ID
# signing + notarization come with the release workflow.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="release"; PROFILE_DIR="release"; DO_OPEN=0
for arg in "$@"; do
  case "$arg" in
    --debug) PROFILE="dev"; PROFILE_DIR="debug" ;;
    --open) DO_OPEN=1 ;;
    *) echo "unknown arg: $arg" >&2; exit 1 ;;
  esac
done

VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
ICNS="$ROOT/crates/pigeon-bin/assets/icon/Pigeon.icns"
if [ ! -f "$ICNS" ] || [ "$ROOT/design/logo/app-icon.svg" -nt "$ICNS" ]; then
  "$ROOT/scripts/gen-icon.sh"
fi

echo "==> building ($PROFILE)…"
cargo build --manifest-path "$ROOT/Cargo.toml" -p pigeon-bin -p pigeon-cli --profile "$PROFILE"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}/$PROFILE_DIR"

APP="$ROOT/dist/Pigeon.app"
echo "==> assembling $APP (v$VERSION)…"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources"
cp "$TARGET/pigeon-app" "$APP/Contents/MacOS/Pigeon"
cp "$TARGET/pigeon" "$APP/Contents/Helpers/pigeon"
cp "$ICNS" "$APP/Contents/Resources/Pigeon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Pigeon</string>
  <key>CFBundleDisplayName</key><string>Pigeon</string>
  <key>CFBundleIdentifier</key><string>il.co.frost-media.pigeon</string>
  <key>CFBundleExecutable</key><string>Pigeon</string>
  <key>CFBundleIconFile</key><string>Pigeon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>© Frost Media</string>
</dict>
</plist>
PLIST

codesign --force --sign - "$APP/Contents/Helpers/pigeon" >/dev/null
codesign --force --sign - "$APP" >/dev/null
echo "==> $APP"
[ "$DO_OPEN" = "1" ] && open "$APP"
exit 0
