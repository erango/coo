#!/usr/bin/env bash
# Build dist/Coo.app (macOS), no Xcode needed.
#   ./scripts/bundle.sh            release build
#   ./scripts/bundle.sh --debug    debug build (faster)
#   ./scripts/bundle.sh --universal  release universal binary (arm64 + x86_64)
#   ./scripts/bundle.sh --open     launch it when done
#
# Signing: with PIGEON_SIGN_IDENTITY set (e.g. "Developer ID Application:
# Name (TEAMID)") the bundle is signed for distribution: hardened runtime,
# secure timestamp, entitlements, helper CLI first. Otherwise ad-hoc.
# PIGEON_PROVISION_PROFILE=<Developer ID .provisionprofile> embeds it and
# adds the Time Sensitive Notifications entitlement (restricted: claiming it
# without a matching profile stops the app from launching).
# Updates: Sparkle 2 (scripts/fetch-sparkle.sh) is embedded in
# Contents/Frameworks. CFBundleVersion is a build number Sparkle compares
# (COO_BUILD_NUMBER, default: the commit count); COO_FEED_URL overrides
# the appcast URL for local update tests.
# The `coo` CLI ships inside the bundle at Contents/Helpers/coo (not
# MacOS/: the default case-insensitive disk would make it the same file as
# MacOS/Coo) so agent setup can point at a stable path.
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
BUILD="${COO_BUILD_NUMBER:-$(git -C "$ROOT" rev-list --count HEAD)}"
FEED_URL="${COO_FEED_URL:-https://github.com/erango/coo/releases/latest/download/appcast.xml}"
SPARKLE_PUBLIC_KEY="SI4PR6GjSgQtfjIuBmJxRHmBrUpi2O5vx8Ru1lXUT+g="
SPARKLE="$("$ROOT/scripts/fetch-sparkle.sh")"
ICNS="$ROOT/crates/pigeon-bin/assets/icon/Coo.icns"
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
  for b in pigeon-app coo; do
    lipo -create "$TARGET_ROOT/aarch64-apple-darwin/release/$b" "$TARGET_ROOT/x86_64-apple-darwin/release/$b" -output "$TARGET/$b"
    echo "==> lipo $b: $(lipo -archs "$TARGET/$b")"
  done
else
  echo "==> building ($PROFILE)…"
  cargo build --manifest-path "$ROOT/Cargo.toml" -p pigeon-bin -p pigeon-cli --profile "$PROFILE"
  TARGET="$TARGET_ROOT/$PROFILE_DIR"
fi

APP="$ROOT/dist/Coo.app"
echo "==> assembling $APP (v$VERSION)…"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$TARGET/pigeon-app" "$APP/Contents/MacOS/Coo"
ditto "$SPARKLE/Sparkle.framework" "$APP/Contents/Frameworks/Sparkle.framework"
cp "$TARGET/coo" "$APP/Contents/Helpers/coo"
cp "$ICNS" "$APP/Contents/Resources/Coo.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Coo</string>
  <key>CFBundleDisplayName</key><string>Coo</string>
  <key>CFBundleIdentifier</key><string>il.co.frost-media.pigeon</string>
  <key>CFBundleExecutable</key><string>Coo</string>
  <key>CFBundleIconFile</key><string>Coo</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$BUILD</string>
  <key>SUFeedURL</key><string>$FEED_URL</string>
  <key>SUPublicEDKey</key><string>$SPARKLE_PUBLIC_KEY</string>
  <key>SUEnableAutomaticChecks</key><true/>
  <key>SUAutomaticallyUpdate</key><true/>
  <key>SUScheduledCheckInterval</key><integer>21600</integer>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

if [ -n "${PIGEON_SIGN_IDENTITY:-}" ]; then
  echo "==> signing with $PIGEON_SIGN_IDENTITY"
  ENTITLEMENTS="$ROOT/scripts/Coo.entitlements"
  if [ -n "${PIGEON_PROVISION_PROFILE:-}" ]; then
    cp "$PIGEON_PROVISION_PROFILE" "$APP/Contents/embedded.provisionprofile"
    ENTITLEMENTS="$(mktemp -d)/Coo.entitlements"
    cp "$ROOT/scripts/Coo.entitlements" "$ENTITLEMENTS"
    TEAM="$(security cms -D -i "$PIGEON_PROVISION_PROFILE" | plutil -extract TeamIdentifier.0 raw -o - -)"
    # PlistBuddy, not plutil: plutil reads the dots in these keys as a path.
    PB=/usr/libexec/PlistBuddy
    "$PB" -c "Add :com.apple.developer.usernotifications.time-sensitive bool true" "$ENTITLEMENTS"
    "$PB" -c "Add :com.apple.application-identifier string $TEAM.il.co.frost-media.pigeon" "$ENTITLEMENTS"
    "$PB" -c "Add :com.apple.developer.team-identifier string $TEAM" "$ENTITLEMENTS"
    echo "==> embedded provisioning profile (team $TEAM), Time Sensitive notifications on"
  fi
  SIGN=(codesign --force --timestamp --options runtime --entitlements "$ENTITLEMENTS" --sign "$PIGEON_SIGN_IDENTITY")
  # Explicit identifier: the app's sign-in broker only answers Coo's own signed programs.
  HELPER_SIGN=(codesign --force --timestamp --options runtime --identifier il.co.frost-media.pigeon.coo --entitlements "$ROOT/scripts/Coo.entitlements" --sign "$PIGEON_SIGN_IDENTITY")
  # Inside-out: nested code first, then the bundle (never --deep). Sparkle's
  # parts per its docs for non-sandboxed apps; Downloader.xpc keeps its own
  # entitlements.
  FW="$APP/Contents/Frameworks/Sparkle.framework/Versions/B"
  SPARKLE_SIGN=(codesign --force --timestamp --options runtime --sign "$PIGEON_SIGN_IDENTITY")
  "${SPARKLE_SIGN[@]}" "$FW/XPCServices/Installer.xpc"
  "${SPARKLE_SIGN[@]}" --preserve-metadata=entitlements "$FW/XPCServices/Downloader.xpc"
  "${SPARKLE_SIGN[@]}" "$FW/Autoupdate"
  "${SPARKLE_SIGN[@]}" "$FW/Updater.app"
  "${SPARKLE_SIGN[@]}" "$APP/Contents/Frameworks/Sparkle.framework"
  "${HELPER_SIGN[@]}" "$APP/Contents/Helpers/coo"
  "${SIGN[@]}" "$APP"
  codesign --verify --strict --verbose=2 "$APP"
else
  codesign --force --sign - "$APP/Contents/Frameworks/Sparkle.framework" >/dev/null
  codesign --force --sign - "$APP/Contents/Helpers/coo" >/dev/null
  codesign --force --sign - "$APP" >/dev/null
fi
echo "==> $APP"
[ "$DO_OPEN" = "1" ] && open "$APP"
exit 0
