# 10: Feel pass

**What to build:** Card animation, transitions, audio and general polish so the loop feels good rather than functional.

**Blocked by:** 07, 08, 09

**Status:** in-progress — visuals + motion done; audio pending

- [x] Deal, merge, flip and result transitions are animated.
- [ ] Sounds for deal / merge / win / lose, with a mute option.
- [x] No frame-rate or input regressions; a full round plays through without visual glitches.

**Notes (visual direction agreed with the user, `dark slate modern` + real poker cards):**

- Near-black table, flat dark pill buttons (hover/press tint), thin green outlined timer pill, gold accents.
- **Cards are poker cards**: white rounded faces, rank + suit in the top-left, red for ♥♦ and ink for ♠♣. Suits are cosmetic and assigned per deal (`logic::Suit`), mixing the rank in so a hand of distinct ranks isn't all one suit and duplicates of a rank get different suits. Merged values have **no** suit and render as a **gold "value token"** (dark tile, gold rank + `●`), so cards and computed results read differently at a glance.
- Bundled **DejaVu Sans** (`assets/fonts/DejaVuSans.ttf`) and override Bevy's default font so the ♠♥♦♣ glyphs render (the built-in font has no suit glyphs).
- Motion: staggered deal pop, merge-result pop, horizontal squish on face-down flip, result pop; hover/press tint on buttons. Driven by `Pop`/`Flip` components on `UiTransform`, fired by `fx_system` comparing successive `Game` snapshots.
- Menu screens sit in a framed panel.
- Tests: 31 app / 27 core / 2 fixtures; clippy clean (default and `devtools`).

**Visual direction (chosen with the user): "A · Parlour"** — after exploring three directions (`design/snap24-directions.html`) and an A-vs-C comparison (`design/decide-a-vs-c.html`), the user picked A:
- **Type**: **Fraunces** (display — wordmark, target, card ranks, fraction values) + **Space Grotesk** (body, the global default) + **DejaVu Sans** (only for the ♠♥♦♣ suit glyphs the other two lack). All bundled in `assets/fonts/`, loaded at app-build time (`install_fonts`) so the first `OnEnter` already has real handles.
- **Surface**: warm near-black room, ivory cards, single **brass** accent, circular brass-hairline operator keys; selection reads as a brass ring rather than a fill.
- **Fractions render stacked (vertical)**, not `a/b`: merged fractional values show numerator / rule / denominator (e.g. 5 over 6) inside the token. Integers stay a single numeral.
- Board header split: wordmark left, `Mode · Tier` chip right, `Target` label + large numeral centred.

**Menu-flow redesign (after the board):** a critique of Title → Mode → Difficulty → Target found the menus reading like a *different app* — a centred rounded "modal card" with low-contrast pill buttons and body copy set in heavy Fraunces. Reworked to match the board:
- Removed the panel; all screens now share the board's chrome (`menu_screen`): vignette background, two-tone `SNAP 24` wordmark top-left, tracked context label top-right, open centred content.
- Type roles fixed: Fraunces only for titles / the tier name / the target value; descriptions and labels are Space Grotesk, muted.
- Controls unified with the board: **brass primary** (`Play`, `Next`, `Start`) + **ghost underlined** secondary (`Back`, `Random`).
- Mode is now two selectable **choice cards** (title + real description) instead of stacked pills; the difficulty screen shows the tier big with its **real parameters** ("5 CARDS · UNLIMITED VIEW" from `card_count`/`view_seconds`); the target screen uses a hairline keypad with a tracked `TARGET` + large value.

**Caveat:** the devtools screenshot capture is unreliable immediately after a state transition (black frames), so the animations aren't proven pixel-by-pixel — they should be eyeballed in the running app. Audio (with mute) is the remaining piece.
