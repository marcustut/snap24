# 05: Generator + difficulty ladder

**What to build:** `generate(mode, difficulty, rng) -> Puzzle` for both modes, plus the difficulty ladder. Classic is a fixed 5 cards targeting 24; Custom scales the card count with difficulty and draws a target from the hand's reachable set.

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] Classic: always 5 cards from a standard 52-card deck (A=1, J=11, Q=12, K=13), target 24, guaranteed solvable.
- [ ] Custom: target sampled from the dealt hand's reachable set (excluding trivial values), guaranteed solvable; card count follows the tier ladder (5 → 3; fewer cards = harder).
- [ ] All six tiers defined (Easy / Medium / Hard / Expert / Insane / Blind) with their card count and view time.
- [ ] Deterministic under a seeded RNG; tests assert every generated puzzle is solvable and matches its tier parameters.
- [ ] `cargo test` green.
