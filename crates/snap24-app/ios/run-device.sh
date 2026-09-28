#!/usr/bin/env bash
# Build Snap 24 for a connected iPhone, sign it with YOUR team and install it.
#
#   DEVELOPMENT_TEAM=ABCDE12345 crates/snap24-app/ios/run-device.sh
#
# Automatic signing works with a free Apple ID (personal team): the app then
# expires after 7 days and must be re-installed. You can find your Team ID in
# Xcode -> Settings -> Accounts.
set -euo pipefail

: "${DEVELOPMENT_TEAM:?set DEVELOPMENT_TEAM=<your 10-char team id>}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT="$SCRIPT_DIR/bevy_mobile_example.xcodeproj"
DERIVED="${DERIVED_DATA:-/tmp/s24-ios-device}"
BUNDLE_ID="me.marcustut.snap24"

echo "==> xcodebuild (Release, team $DEVELOPMENT_TEAM)"
xcodebuild -project "$PROJECT" -scheme bevy_mobile_example -configuration Release \
  -destination 'generic/platform=iOS' -derivedDataPath "$DERIVED" \
  -allowProvisioningUpdates \
  ENABLE_USER_SCRIPT_SANDBOXING=NO \
  DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM" \
  CODE_SIGN_STYLE=Automatic \
  | tail -5

APP="$DERIVED/Build/Products/Release-iphoneos/Snap24.app"

if ! command -v ios-deploy >/dev/null 2>&1; then
  echo
  echo "==> built $APP"
  echo "Install it with Xcode (Window -> Devices and Simulators -> drag the .app"
  echo "onto your iPhone), or 'brew install ios-deploy' and re-run."
  exit 0
fi

echo "==> installing $APP"
ios-deploy --bundle "$APP"
