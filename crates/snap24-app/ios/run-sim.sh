#!/usr/bin/env bash
# Build snap24-app for the iOS simulator, install and launch it.
#
#   crates/snap24-app/ios/run-sim.sh [SIMULATOR_UDID] [SCREENSHOT_PATH]
#
# With no UDID it uses the first available simulator.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE="$(cd "$SCRIPT_DIR/../../.." && pwd)"
PROJECT="$SCRIPT_DIR/bevy_mobile_example.xcodeproj"
DERIVED="${DERIVED_DATA:-/tmp/s24-ios-build}"
BUNDLE_ID="com.snap24.game"

DEVICE="${1:-$(xcrun simctl list devices available | grep -m1 -Eo '[0-9a-f]{8}-([0-9a-f]{4}-){3}[0-9a-f]{12}')}"
SHOT="${2:-}"

if [ -z "$DEVICE" ]; then
  echo "No available simulator found; create one in Xcode." >&2
  exit 1
fi

echo "==> simulator $DEVICE"
xcrun simctl boot "$DEVICE" 2>/dev/null || true

echo "==> xcodebuild"
xcodebuild -project "$PROJECT" -scheme bevy_mobile_example -configuration Debug \
  -destination "id=$DEVICE" -derivedDataPath "$DERIVED" \
  ENABLE_USER_SCRIPT_SANDBOXING=NO \
  | tail -5

APP="$DERIVED/Build/Products/Debug-iphonesimulator/Snap24.app"
echo "==> install $APP"
xcrun simctl terminate "$DEVICE" "$BUNDLE_ID" 2>/dev/null || true
xcrun simctl install "$DEVICE" "$APP"

echo "==> launch $BUNDLE_ID"
xcrun simctl launch "$DEVICE" "$BUNDLE_ID"

if [ -n "$SHOT" ]; then
  sleep 6
  xcrun simctl io "$DEVICE" screenshot "$SHOT"
  echo "==> screenshot $SHOT"
fi
