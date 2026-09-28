#!/usr/bin/env bash
# Archive Snap 24 for the App Store and export an .ipa.
#
# Signing needs YOUR Apple Developer team — pass it in (nothing is stored here):
#
#   DEVELOPMENT_TEAM=ABCDE12345 crates/snap24-app/ios/archive.sh
#
# Then upload with Xcode Organizer, Transporter.app, or:
#   xcrun altool --upload-app -f <ipa> -t ios -u <apple-id> -p <app-specific-password>
#
# Requires: an Apple Developer account, a distribution certificate and an
# App Store provisioning profile for me.marcustut.snap24 registered in App Store
# Connect (create the app record first).
set -euo pipefail

: "${DEVELOPMENT_TEAM:?set DEVELOPMENT_TEAM=<your 10-char team id>}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT="$SCRIPT_DIR/bevy_mobile_example.xcodeproj"
DERIVED="${DERIVED_DATA:-/tmp/s24-archive}"
ARCHIVE="$DERIVED/Snap24.xcarchive"
EXPORT_DIR="$DERIVED/export"
OPTS="$DERIVED/ExportOptions.plist"

mkdir -p "$DERIVED"
sed "s/TEAM_ID/$DEVELOPMENT_TEAM/" "$SCRIPT_DIR/ExportOptions.plist" > "$OPTS"

echo "==> archiving (team $DEVELOPMENT_TEAM)"
xcodebuild archive \
  -project "$PROJECT" \
  -scheme bevy_mobile_example \
  -configuration Release \
  -destination 'generic/platform=iOS' \
  -archivePath "$ARCHIVE" \
  -derivedDataPath "$DERIVED" \
  -allowProvisioningUpdates \
  ENABLE_USER_SCRIPT_SANDBOXING=NO \
  DEVELOPMENT_TEAM="$DEVELOPMENT_TEAM" \
  CODE_SIGN_STYLE=Automatic \
  | tail -8

echo "==> exporting .ipa"
xcodebuild -exportArchive \
  -archivePath "$ARCHIVE" \
  -exportOptionsPlist "$OPTS" \
  -exportPath "$EXPORT_DIR" \
  -allowProvisioningUpdates \
  | tail -8

echo "==> done: $EXPORT_DIR"
ls "$EXPORT_DIR"
