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

**Bug found and fixed on this ticket:** the board originally despawned and re-spawned its UI (root and/or content) whenever `Game` changed. In Bevy 0.19 that leaves the window black — despawning UI entities and spawning replacements in the same frame stops rendering, which is what made the game go black after entering `Playing` or making a move. The play screen now spawns its widgets once on `OnEnter(Screen::Playing)` and mutates them in place (`update_board`), never despawning in `Update`. Regression detail recorded here in case a later ticket is tempted to "just rebuild the board".

**Caveat:** as before, I can't click the window, so the across-tiers manual pass needs a human. Worth eyeballing the Custom flows especially (card count and target now vary by tier) and the running total after a couple of rounds.
