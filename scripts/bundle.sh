#!/usr/bin/env bash
# Build dist/Pigeon.app (macOS), no Xcode needed.
#   ./scripts/bundle.sh            release build
#   ./scripts/bundle.sh --debug    debug build (faster)
#   ./scripts/bundle.sh --universal  release universal binary (arm64 + x86_64)
#   ./scripts/bundle.sh --open     launch it when done
#
# Signing: with PIGEON_SIGN_IDENTITY set (e.g. "Developer ID Application:
# Name (TEAMID)") the bundle is signed for distribution: hardened runtime,
# secure timestamp, entitlements, helper CLI first. Otherwise ad-hoc.
# The `pigeon` CLI ships inside the bundle at Contents/Helpers/pigeon (not
# MacOS/: the default case-insensitive disk would make it the same file as
# MacOS/Pigeon) so agent setup can point at a stable path. Signing is ad-hoc for now; Developer ID
# signing + notarization come with the release workflow.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="release"; PROFILE_DIR="release"; DO_OPEN=0; UNIVERSAL=0
for arg in "$@"; do
  case "$arg" in
    --debug) PROFILE="dev"; PROFILE_DIR="debug" ;;
    --open) DO_OPEN=1 ;;
    --universal) UNIVERSAL=1 ;;
    *) echo "unknown arg: $arg" >&2; exit 1 ;;
  esac
done

VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
ICNS="$ROOT/crates/pigeon-bin/assets/icon/Pigeon.icns"
if [ ! -f "$ICNS" ] || [ "$ROOT/design/logo/app-icon.svg" -nt "$ICNS" ]; then
  "$ROOT/scripts/gen-icon.sh"
fi

TARGET_ROOT="${CARGO_TARGET_DIR:-$ROOT/target}"
if [ "$UNIVERSAL" = "1" ]; then
  echo "==> building universal (arm64 + x86_64)…"
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --manifest-path "$ROOT/Cargo.toml" -p pigeon-bin -p pigeon-cli --release --target "$t"
  done
  TARGET="$TARGET_ROOT/universal"
  mkdir -p "$TARGET"
  for b in pigeon-app pigeon; do
    lipo -create "$TARGET_ROOT/aarch64-apple-darwin/release/$b" "$TARGET_ROOT/x86_64-apple-darwin/release/$b" -output "$TARGET/$b"
    echo "==> lipo $b: $(lipo -archs "$TARGET/$b")"
  done
else
  echo "==> building ($PROFILE)…"
  cargo build --manifest-path "$ROOT/Cargo.toml" -p pigeon-bin -p pigeon-cli --profile "$PROFILE"
  TARGET="$TARGET_ROOT/$PROFILE_DIR"
fi

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

if [ -n "${PIGEON_SIGN_IDENTITY:-}" ]; then
  echo "==> signing with $PIGEON_SIGN_IDENTITY"
  SIGN=(codesign --force --timestamp --options runtime --entitlements "$ROOT/scripts/Pigeon.entitlements" --sign "$PIGEON_SIGN_IDENTITY")
  # Inside-out: nested code first, then the bundle (never --deep).
  "${SIGN[@]}" "$APP/Contents/Helpers/pigeon"
  "${SIGN[@]}" "$APP"
  codesign --verify --strict --verbose=2 "$APP"
else
  codesign --force --sign - "$APP/Contents/Helpers/pigeon" >/dev/null
  codesign --force --sign - "$APP" >/dev/null
fi
echo "==> $APP"
[ "$DO_OPEN" = "1" ] && open "$APP"
exit 0
