# Snap 24 on iOS

Ticket 11 spike: get a Bevy app from this workspace onto the iOS simulator and
record the build path. This runs the **real game binary** (not a stub) — Xcode
treats the Rust executable itself as the app's executable, so there is no Swift
bridge.

## Chosen path: direct Xcode project (no cargo-mobile2)

We use a checked-in Xcode project (adapted from Bevy's `examples/mobile`, MIT OR
Apache-2.0) with a run-script build phase that invokes Cargo. `cargo-mobile2`
would also work, but the direct project is one less moving part, needs no
global `cargo install`, and reuses our existing workspace `target/` cache.

Project layout:

| Path | What |
|---|---|
| `bevy_mobile_example.xcodeproj/` | Xcode project (target/scheme kept from the template) |
| `ios-src/Info.plist` | app plist; bundle id comes from the project (`me.marcustut.snap24`) |
| `build_rust_deps.sh` | Xcode run-script phase: cargo-builds `snap24-app` for the right target and `lipo`s it into the app bundle |
| `run-sim.sh` | convenience: build + install + launch on a simulator |

## Prerequisites

- Xcode with an iOS SDK (verified with Xcode 27.0).
- Rust iOS targets: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios`.
- An available simulator (`xcrun simctl list devices available`).

## Build & run

```sh
# one command (boots/installs/launches on the first available simulator):
crates/snap24-app/ios/run-sim.sh

# or pick a device explicitly:
crates/snap24-app/ios/run-sim.sh <SIMULATOR_UDID>
```

The exact commands it wraps:

```sh
DEVICE=<UDID>
xcodebuild -project crates/snap24-app/ios/bevy_mobile_example.xcodeproj \
  -scheme bevy_mobile_example -configuration Debug \
  -destination "id=$DEVICE" -derivedDataPath /tmp/s24-ios-build \
  ENABLE_USER_SCRIPT_SANDBOXING=NO

xcrun simctl install "$DEVICE" /tmp/s24-ios-build/Build/Products/Debug-iphonesimulator/Snap24.app
xcrun simctl launch  "$DEVICE" me.marcustut.snap24
xcrun simctl io "$DEVICE" screenshot /tmp/sim.png   # optional
```

`ENABLE_USER_SCRIPT_SANDBOXING=NO` is required because the run-script phase
writes to the workspace `target/` directory.

## What the spike needed (iOS-specific fixes already in the app)

1. **Fullscreen window** — Bevy's default window is the desktop 1280×720
   *points*, wider than the phone, so the UI rendered off-screen. On iOS we set
   `WindowMode::BorderlessFullscreen(Primary)` (`main.rs`, `cfg(target_os="ios")`).
2. **`WinitSettings::mobile()`** — without it the loop isn't driven
   event-driven and the app shows one frame; Bevy logs "processing non
   `RedrawRequested` event" repeatedly.
3. **Debuggability** — `bevy_mobile_example` keeps its template name; the app
   product is `Snap24.app` (`PRODUCT_NAME = Snap24`).

## Adaptation (ticket 12)

- **Scaling** — `fit_ui` sets `UiScale` from the window so the desktop-pixel
  layout fits a phone: `min(width/720, height/1100)`, clamped. Fitting both axes
  keeps landscape from overflowing.
- **Safe areas** — Bevy exposes no inset API, so `screen_padding()` adds extra
  top/bottom padding in design units on iOS (≈60pt/38pt after scaling) to clear
  the notch and home indicator. It's an approximation, not true insets.
- **Touch** — nothing to wire: Bevy's picking plugin turns touches into pointer
  events, which already drive the UI `Interaction` the whole game uses (cards,
  operator keys, slider, buttons).
- **Haptics** — `haptics::impact()` on a merge and `haptics::success()` on a win,
  via `objc2`/`objc2-ui-kit` (iOS only; no-ops elsewhere). Generators must be
  used on the main thread, so `fx_system` takes Bevy's `NonSendMarker` to be
  scheduled there. Gated by the same mute toggle as audio.
- **Audio** — the embedded WAVs play through CoreAudio; the `Sound/Muted`
  control works as on desktop.

## App Store packaging (ticket 13)

Configured in the project:

| Item | Where |
|---|---|
| Name "Snap 24" | `ios-src/Info.plist` → `CFBundleDisplayName` |
| Bundle id | project → `PRODUCT_BUNDLE_IDENTIFIER = me.marcustut.snap24` |
| App icon | `Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png` (regenerate with `python3 make_icon.py`) + `ASSETCATALOG_COMPILER_APPICON_NAME = AppIcon` |
| Launch screen | `Info.plist` → empty `UILaunchScreen` dict (solid background) |
| Device family | iPhone (`TARGETED_DEVICE_FAMILY = 1`) |

**What still needs you (I can't do it):** signing and upload, which need your Apple
Developer account. Create the app record in App Store Connect for
`me.marcustut.snap24`, then:

```sh
DEVELOPMENT_TEAM=<your-team-id> crates/snap24-app/ios/archive.sh
# then upload the .ipa via Xcode Organizer, Transporter.app, or:
xcrun altool --upload-app -f <ipa> -t ios -u <apple-id> -p <app-specific-password>
```

`archive.sh` runs `xcodebuild archive` (Release, generic iOS device) and
`-exportArchive` with `ExportOptions.plist` (`method = app-store-connect`),
substituting your team id. No credentials are stored in the repo.

**Listing + screenshots** are drafted in `app-store/`: `listing.md` (name,
subtitle, description, keywords, privacy answers) and
`app-store/screenshots/*.png` at 6.9" (1320×2868).

## Known limitations / risks

- **Layout is desktop-shaped.** The title screen's hero overflows the phone
  width and the wordmark collides with the status bar. Safe-area insets, phone
  scaling and touch targets are **ticket 12**, not the spike.
- **Device builds need signing.** This project builds unsigned for the
  simulator (`CODE_SIGN_IDENTITY=""`). A real device needs a team, provisioning
  profile and bundle-id changes.
- **Rendering backend** is Metal via wgpu; it works on the Apple-Silicon
  simulator. Performance on the simulator is not representative of a device.
- **No hot reload.** Every change is a full `cargo build` + `xcodebuild`.
  Debug iOS builds of Bevy take on the order of a couple of minutes from cold;
  repeat builds reuse `target/` and are quick.
- **Assets are embedded.** Fonts and SFX use `include_bytes!`, so they work on
  device without an asset bundle. Anything that later needs `AssetServer`
  (e.g. images) would need the assets folder copied into the app bundle — the
  template's resource reference was removed here.
- **Input** is untested (no touch handling added yet).
