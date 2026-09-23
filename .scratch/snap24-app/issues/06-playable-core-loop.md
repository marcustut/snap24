# 06: Playable core loop

**What to build:** A Bevy desktop app where the player is dealt a Classic puzzle and solves it by tapping two cards and an operator, merging 5 → 4 → 3 → 2 → 1, with win/lose decided by the final value.

**Blocked by:** 04, 05

**Status:** ready-for-agent

- [ ] App launches on macOS and deals a solvable Classic puzzle from `snap24-core`.
- [ ] Tapping two cards plus an operator merges them; illegal merges (e.g. division by zero) are blocked.
- [ ] Win when the final value equals the target; lose otherwise; both show the target and offer a New Puzzle action.
- [ ] Cards render values with the A/J/Q/K rank mapping.
- [ ] Full manual playthrough verified.
