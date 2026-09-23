# 07: Timer + hide mechanic

**What to build:** The view-window timer and face-down solving mode — the mechanic that makes Snap 24 its own game. Medium/hard tiers memorize then solve from memory, with card values hidden but slot positions stable.

**Blocked by:** 06

**Status:** done

- [x] Timer driven by the tier's view time; Easy stays visible indefinitely.
- [x] On expiry, cards flip face-down but keep stable slot positions so recall is fair.
- [x] Merging works on face-down cards; the merged result is revealed per the agreed rule (decide and record: show the computed merge result).
- [x] Blind tier never shows the cards.
- [x] Countdown is visible during viewing; each tier manually tested.

**Notes / decisions:**

- View model in `logic::ViewPhase` (`Unlimited` = Easy, `Visible { remaining }`, `Hidden` = Blind). `from_view_seconds(Difficulty::view_seconds())` drives it; `tick(dt)` returns `true` only on the tick that ends the window, so the app flips the cards exactly once.
- **Agreed reveal rule (recorded):** the computed merge result is always shown face-up, even when both operands were face-down; the untouched cards keep their state. Undo restores the previous reveal states too.
- `Round` now carries a `revealed: Vec<bool>` parallel to `cards`; `conceal()` flips all without moving slots. Logic tests cover conceal, result-reveal, undo-of-reveal, and the timer.
- The countdown lives in its own `ViewTimer` resource so the per-frame tick does not mark `Game` changed and rebuild the board every frame; a `CountdownLabel` text entity is updated in place.
- **Dev keybinds (until ticket 08):** number keys `1`–`6` pick Easy/Medium/Hard/Expert/Insane/Blind and deal immediately, so each tier can be exercised. `cargo test -p snap24-app` = 22 tests, clippy clean, launches.

**Caveat:** as with ticket 06, I cannot click the window — the per-tier manual check needs a human. Easy (no timer), Medium (10s countdown then flip), and Blind (starts as `?`) are the ones to eyeball.
