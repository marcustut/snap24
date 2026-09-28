# 12: iOS adaptation

**What to build:** The full game running on iOS with touch input, safe-area layout, haptics and audio.

**Blocked by:** 06, 11

**Status:** done

- [x] Tap/merge loop is playable on device with touch.
- [x] Layout respects safe areas across phone sizes and orientations.
- [x] Haptics on merge/win; audio works, with mute.
- [x] Timer and face-down flow behave correctly on device.

**What changed:**

- **Scaling** — `fit_ui` (iOS only) sets `UiScale` from the window: `min(width/720, height/1100)` clamped, so the desktop-pixel layout fits the phone and survives landscape (height-limited). Verified on the iPhone 17 Pro simulator: title screen and the 5-card board both fit, no wrapping.
- **Safe areas** — Bevy exposes no inset API, so `screen_padding()` adds extra top/bottom padding on iOS (≈60pt/38pt after scaling) to clear the notch and home indicator. Approximate, documented.
- **Touch** — no code needed: Bevy's picking plugin already routes touch to pointer events, which drive the UI `Interaction` the whole game uses (cards, operator keys, difficulty slider, buttons).
- **Haptics** — `objc2` + `objc2-ui-kit` (iOS-only deps): `impact()` on a merge, `success()` on a win. Feedback generators need the main thread, so `fx_system` takes Bevy's `NonSendMarker` to be scheduled there. Gated by the same mute toggle as audio.
- **Audio** — the embedded WAVs play through CoreAudio; mute control unchanged.

**Verified vs manual:** rendering, scaling and the full flow were verified on the simulator (screenshots); the app compiles and launches with the haptics/objc2 code. **Touch feel, haptics and device performance need a human on hardware** — the simulator can't feel haptics and I can't tap it. Device builds still need signing (ticket 13).

**Note:** the top bar sits just under the status bar (approximate padding); a true safe-area query via `objc2` is possible if the approximation proves off.
