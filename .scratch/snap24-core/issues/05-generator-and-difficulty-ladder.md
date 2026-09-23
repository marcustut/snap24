# 05: Generator + difficulty ladder

**What to build:** `generate(mode, difficulty, rng) -> Puzzle` for both modes, plus the difficulty ladder. Classic is a fixed 5 cards targeting 24; Custom scales the card count with difficulty and draws a target from the hand's reachable set.

**Blocked by:** 03

**Status:** done

- [x] Classic: always 5 cards from a standard 52-card deck (A=1, J=11, Q=12, K=13), target 24, guaranteed solvable.
- [x] Custom: target sampled from the dealt hand's reachable set (excluding trivial values), guaranteed solvable; card count follows the tier ladder (5 → 3; fewer cards = harder).
- [x] All six tiers defined (Easy / Medium / Hard / Expert / Insane / Blind) with their card count and view time.
- [x] Deterministic under a seeded RNG; tests assert every generated puzzle is solvable and matches its tier parameters.
- [x] `cargo test` green.

**Notes (decided here — these are the tunable game-design numbers):**
`generate(mode, difficulty, rng) -> Puzzle` in `crates/snap24-core/src/generator.rs` with `Mode`, `Difficulty`, `Puzzle`, and a dependency-free seeded `Rng` (xorshift64*). Classic retries the deal until 24 is reachable; Custom samples a target from the hand's reachable set minus "trivial" values (defined as `0`, `1`, a dealt card, or the plain sum/product of all cards) and retries if none remain. **Custom random targets are whole positive integers** (`is_integer() && is_positive()`), so they are readable/typable; fractions remain for the solver and reveal. `generate_targeted(mode, difficulty, target, rng)` deals a hand that reaches a player-chosen target (retrying up to 20 000 hands; falls back to `generate` if the target is impossible for the card count, so it never hangs).

Ladder (view = seconds visible before flipping face-down; `None` = indefinitely, `Some(0)` = never shown):

| Tier | Cards | View |
|---|---|---|
| Easy | 5 | ∞ |
| Medium | 5 | 10s |
| Hard | 4 | 8s |
| Expert | 4 | 6s |
| Insane | 3 | 4s |
| Blind | 3 | 0s |

Card counts and view windows are exposed as `Difficulty::card_count()` / `Difficulty::view_seconds()` so tickets 07/08 can consume them without hard-coding. If the intended numbers differ, changing these two methods and the `is_trivial` predicate is the whole change.
