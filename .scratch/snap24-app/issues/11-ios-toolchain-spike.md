# 11: iOS toolchain spike

**What to build:** A bare Bevy app running on an iOS simulator/device, to choose and document the build toolchain before the real app is ported. De-risks the highest-uncertainty part of the project early, in parallel with core work.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] A Bevy app launches on the iOS simulator (or a device) from this workspace.
- [ ] The chosen build path (cargo-mobile2 vs direct Xcode) is documented with exact commands.
- [ ] Known limitations and risks recorded (rendering backend, signing, hot reload, compile times).
