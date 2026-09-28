# 11: iOS toolchain spike

**What to build:** A bare Bevy app running on an iOS simulator/device, to choose and document the build toolchain before the real app is ported. De-risks the highest-uncertainty part of the project early, in parallel with core work.

**Blocked by:** 01

**Status:** done

- [x] A Bevy app launches on the iOS simulator (or a device) from this workspace.
- [x] The chosen build path (cargo-mobile2 vs direct Xcode) is documented with exact commands.
- [x] Known limitations and risks recorded (rendering backend, signing, hot reload, compile times).

**Result:** the **real game binary** launches on the iPhone 17 Pro simulator and renders the title screen (wordmark, hero, tagline, brass Play). Xcode treats the Rust executable as the app's executable — no Swift bridge.

**Chosen path:** a checked-in **direct Xcode project** (`crates/snap24-app/ios/`, adapted from Bevy's `examples/mobile`, MIT OR Apache-2.0) whose run-script phase cargo-builds `snap24-app` and `lipo`s it into the bundle. `cargo-mobile2` wasn't used: one less moving part, no global `cargo install`, reuses the workspace `target/` cache. All commands are in `ios/README.md`; `ios/run-sim.sh [UDID] [screenshot]` wraps build+install+launch. Xcode 27 needs `IPHONEOS_DEPLOYMENT_TARGET ≥ 15` and `ENABLE_USER_SCRIPT_SANDBOXING=NO` for the cargo phase.

**Findings folded back into the app:** (1) on iOS set a **fullscreen window** — the desktop 1280×720 default is wider than the phone, so the UI lands off-screen; (2) insert **`WinitSettings::mobile()`** or the event loop stalls after one frame; (3) fonts now use `include_bytes!` like the SFX, so no build-machine asset path is needed (works on device).

**Risks / limitations:** layout is desktop-shaped (hero overflows, wordmark collides with the status bar) — safe areas/touch/phone scaling are ticket 12; device builds need signing/provisioning; Metal-on-simulator works but perf isn't device-representative; no hot reload (cold Bevy iOS debug build ≈ 2 min, then incremental); `AssetServer` isn't used, so nothing yet needs the assets bundle.

**Environment verified:** Xcode 27.0, iOS 27 simulator SDK, Rust targets `aarch64-apple-ios` / `aarch64-apple-ios-sim` / `x86_64-apple-ios` already installed.
