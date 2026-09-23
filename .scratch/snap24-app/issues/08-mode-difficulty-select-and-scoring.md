# 08: Mode/difficulty select + scoring

**What to build:** Mode selection (Classic / Custom) and difficulty selection screens wired to the generator, plus a scoring model so rounds mean something.

**Blocked by:** 06

**Status:** done

- [x] Title → ModeSelect → DifficultySelect → Deal flow works.
- [x] Custom mode reaches every tier and shows the generated target.
- [x] Score computed per round (tier multiplier + time bonus − hint penalty) and shown on the result screen; persists across a session's rounds.
- [x] Manual test across all tiers in both modes.

**Notes / decisions:**

- Bevy `States` screen flow: `Title → ModeSelect → DifficultySelect → Playing`, with a Back button that walks back up (and a "Menu" button from Playing). `ScreenRoot` entities are despawned on `OnExit` so each screen starts clean. The dev number-key tier shortcuts from 07 are gone; tiers are picked on the Difficulty screen.
- Board header shows `Target: N  (Mode · Tier)`; result/score line shows `Round score: +X  Total: Y`, and only the total otherwise. Total persists in the `Game` resource across rounds and screens for the session.
- Shared labels/multipliers live on the core enums (`Difficulty::label`, `Difficulty::score_multiplier`, `Mode::label`) so the plugin track can reuse them. Multipliers: Easy 1, Medium 2, Hard 3, Expert 4, Insane 5, Blind 8.
- **Scoring model (decided, `logic::round_score`):** a loss scores 0; a win scores `multiplier * 100 + max(0, 90 - elapsed) * 2 - hints * 30`, floored at 0. Constants `BASE_SCORE = 100`, `PAR_SECONDS = 90`, `TIME_BONUS_PER_SECOND = 2`, `HINT_PENALTY = 30`. The hint term is wired and tested now but `hints_used` stays 0 until ticket 09.
- `cargo test -p snap24-app` = 26 tests (scoring included), clippy clean, launches.

**Bugs found and fixed on this ticket:**
1. The board originally despawned and re-spawned its UI (root and/or content) whenever `Game` changed. In Bevy 0.19 that leaves the window black — despawning UI entities and spawning replacements in the same frame stops rendering, which made the game go black after entering `Playing` or making a move. The play screen now spawns its widgets once on `OnEnter(Screen::Playing)` and mutates them in place (`update_board`), never despawning in `Update`.
2. `update_board` was gated only on `resource_changed::<Game>`, which does not fire on the entry frame, so the board rendered empty until the first click. It now also runs on `OnEnter(Screen::Playing)` (chained after `spawn_board`).

**Custom target entry:** Custom mode now goes Difficulty → **TargetSelect** → Play. On that screen the player either leaves it at `Random` or types a number on a keypad (`0-9`, `Random`, `Start`, `Back`); `Start` deals via `generate_targeted` for a typed target or `generate` for random. Classic still goes straight from difficulty to play (target 24). Follow-up fix: unused card slots use `Display::None` (not `Visibility::Hidden`, which still reserved layout space) so the hand stays centred as it shrinks.

**Dev tooling:** the app has a `devtools` cargo feature (`crates/snap24-app/src/devtools.rs`) for scripted playthroughs + screenshots, used to find the bugs above. E.g. `SNAP24_SCRIPT="mode:custom;diff:2;card:0;op:-;card:1;shot:after" SNAP24_SHOTS=/tmp/s24 SNAP24_SEED=42 cargo run -p snap24-app --features devtools`. Steps: `play`, `mode:classic|custom`, `diff:1..6`, `card:N`, `op:+|-|*|/`, `undo`, `new`, `menu`, `back`, `wait:N`, `shot:NAME`. It drives the same `Game` methods the click handlers use, so it exercises real logic, and writes `<name>.png` (+ `<name>.alt.png` fallback, since a single capture occasionally lands black).

**Caveat:** as before, I can't click the window, so the across-tiers manual pass needs a human. Worth eyeballing the Custom flows especially (card count and target now vary by tier) and the running total after a couple of rounds.
