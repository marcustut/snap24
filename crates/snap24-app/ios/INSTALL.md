# Install Snap 24 on your iPhone

There are two routes. Both need an Apple ID; only the second needs a **paid**
Apple Developer account.

The repo contains `Snap24-unsigned.ipa` — a Release build for **arm64 devices**
(bundle `me.marcustut.snap24`, display name "Snap 24"). An unsigned IPA cannot
be installed as-is: iOS only runs apps signed by a certificate tied to your
Apple ID. So pick a route below to sign + install it.

---

## Route A — free Apple ID (personal team, re-sign every 7 days)

Good for trying it on your own phone. No paid account. Limit: the app expires
after 7 days and needs re-installing, and only *your* devices can run it.

**Easiest: install straight from Xcode (no IPA needed)**

1. Plug in the iPhone, unlock it, tap **Trust**. Enable **Developer Mode** on
   the phone (Settings → Privacy & Security → Developer Mode) if prompted.
2. Get your **Team ID**: Xcode → Settings → Accounts → select your Apple ID →
   the 10-character **Team ID** is shown.
3. Run:

   ```sh
   DEVELOPMENT_TEAM=<your-team-id> crates/snap24-app/ios/run-device.sh
   ```

   That builds Release, signs with your personal team and installs onto the
   connected device. (Under the hood it's Xcode's automatic signing; the first
   run may prompt to trust the developer certificate on the phone.)

**Or sideload the IPA with a free Apple ID**

Use [Sideloadly](https://sideloadly.io) (or AltStore). Drag
`Snap24-unsigned.ipa` in, enter your Apple ID, and it re-signs and installs over
USB. Same 7-day expiry.

---

## Route B — paid Apple Developer Program ($99/yr)

Gives Ad Hoc + TestFlight distribution (up to 100 devices), 1-year profiles, and
the App Store.

1. Create the app record in **App Store Connect** for `me.marcustut.snap24`
   (Agreements, Tax and Banking must be complete to ship).
2. For **TestFlight / Ad Hoc**, archive and export a signed IPA:

   ```sh
   DEVELOPMENT_TEAM=<your-team-id> crates/snap24-app/ios/archive.sh
   # writes <derived>/export/*.ipa
   ```

   which runs `xcodebuild archive` + `-exportArchive`
   (`ExportOptions.plist`, `method = app-store-connect`; switch to
   `ad-hoc`/`development` for direct installs).
3. Upload with Xcode **Organizer** (Window → Organizer → Distribute App),
   **Transporter.app**, or:

   ```sh
   xcrun altool --upload-app -f <ipa> -t ios -u <apple-id> -p <app-specific-password>
   ```

   Then install via **TestFlight**.

---

## Notes

- The bundle id must be unique to your account; `me.marcustut.snap24` is fine
  since you own `marcustut.me`. Changing it after shipping creates a *new* app.
- Apple Developer Mode on the phone is required for local installs (iOS 16+).
- The IPA here is **unsigned**; never share it as a shippable build — sign it
  with your own certificate via one of the routes above.
- Regenerate the IPA any time with:

  ```sh
  cargo build -p snap24-app --release --target aarch64-apple-ios
  xcodebuild archive -project crates/snap24-app/ios/bevy_mobile_example.xcodeproj \
    -scheme bevy_mobile_example -configuration Release \
    -destination 'generic/platform=iOS' -archivePath /tmp/Snap24.xcarchive \
    ENABLE_USER_SCRIPT_SANDBOXING=NO CODE_SIGNING_ALLOWED=NO
  cd /tmp && mkdir -p Payload && cp -R Snap24.xcarchive/Products/Applications/Snap24.app Payload/ \
    && zip -qry Snap24-unsigned.ipa Payload
  ```
