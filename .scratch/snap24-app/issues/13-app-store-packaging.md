# 13: App Store packaging/signing

**What to build:** A signed, submittable iOS build of Snap 24.

**Blocked by:** 12

**Status:** done (config + drafts); signing/upload needs the user's Apple account

- [x] Bundle id, icons, launch screen and the name "Snap 24" configured.
- [ ] Release build signs and archives, and uploads to App Store Connect. *(path provided; needs your Apple Developer team + an App Store Connect app record)*
- [x] Store listing text and screenshots drafted.

**Configured:** display name `Snap 24` (`CFBundleDisplayName`), bundle id `me.marcustut.snap24`, iPhone-only (`TARGETED_DEVICE_FAMILY = 1`), an empty `UILaunchScreen`, and a generated **1024² app icon** (`make_icon.py` → `Assets.xcassets/AppIcon.appiconset/`, wired via `ASSETCATALOG_COMPILER_APPICON_NAME`). Verified in the built bundle: `CFBundleDisplayName = Snap 24`, `Assets.car` present. A **Release build succeeds** (`xcodebuild -configuration Release`).

**Archive/upload path:** `ios/archive.sh` (`DEVELOPMENT_TEAM=… archive.sh`) runs `xcodebuild archive` + `-exportArchive` with `ExportOptions.plist` (`method = app-store-connect`), substituting the team id; then upload via Organizer / Transporter / `altool`. **Not executed** — signing needs an Apple Developer team, a distribution cert, a provisioning profile and an App Store Connect app record, and no credentials belong in the repo. Documented in `ios/README.md`.

**Drafts:** `ios/app-store/listing.md` (name, subtitle, description, keywords, privacy = "no data collected") and `ios/app-store/screenshots/` — 7 screenshots at 6.9" (1320×2868) captured from the running app (title, mode, difficulty, custom target, board, face-down, solved).

**Bug found while capturing:** `WinitSettings::mobile()` is event-driven, so `Time` doesn't advance without input — the view countdown (and animations) can freeze. Switched to `WinitSettings::game()` (continuous while focused, low-power when backgrounded). **Caveat:** the countdown could not be verified on the simulator because it suspends the app when not foregrounded; needs a human check on a device/foregrounded app.
