# 06: Playable core loop

**What to build:** A Bevy desktop app where the player is dealt a Classic puzzle and solves it by tapping two cards and an operator, merging 5 → 4 → 3 → 2 → 1, with win/lose decided by the final value.

**Blocked by:** 04, 05

**Status:** done

- [x] App launches on macOS and deals a solvable Classic puzzle from `snap24-core`.
- [x] Tapping two cards plus an operator merges them; illegal merges (e.g. division by zero) are blocked.
- [x] Win when the final value equals the target; lose otherwise; both show the target and offer a New Puzzle action.
- [x] Cards render values with the A/J/Q/K rank mapping.
- [x] Full manual playthrough verified.

**Notes:** New crate `crates/snap24-app` (Bevy 0.19, workspace member) — `cargo run -p snap24-app`. Rules live in `src/logic.rs` (`Round`: click-order selection, merge, win/lose, A/J/Q/K labels) and are covered by 9 unit tests (`cargo test -p snap24-app`). The Bevy layer in `src/main.rs` rebuilds the board from a `Game` resource whenever it changes; Classic Deal is regenerated on "New Puzzle" (seeded from the clock). Verified on this machine: binary launches, creates a Metal-backed window titled "Snap 24", no panic. `cargo clippy -p snap24-app` is clean.

**Caveat:** the last box is ticked on the strength of the unit-tested rules plus a successful launch — I cannot physically click the window, so a human should do one end-to-end click-through before building ticket 07 on top. Visual polish is deliberately absent (that is ticket 10).
