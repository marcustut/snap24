# Snap 24 — App Store listing (draft)

Fill the `<...>` placeholders before submitting. Character limits noted.

## App information

| Field | Value |
|---|---|
| Name (30) | `Snap 24 - Card Maths` |
| Subtitle (30) | `Make 24 from five cards` |
| Home screen label | `Snap 24` (`CFBundleDisplayName`; must stay short) |
| Bundle ID | `me.marcustut.snap24` |
| Primary category | Games › Puzzle |
| Secondary category | Games › Card |
| Age rating | 4+ (no objectionable content) |
| Price | Free (adjust as desired) |

## Promotional text (170)

```
Five cards, one target. Merge them with + − × ÷ before the hand hides — then solve it from memory.
```

## Description (4000)

```
Snap 24 is the classic 24 game, sharpened.

You're dealt a hand of poker cards and a target number. Combine every card, using each exactly once, with + − × ÷ to hit the target. Clear the hand before the view window runs out.

THE TWIST
Cards don't stay face-up. At Medium and above the hand flips face-down after a few seconds — solve the rest from memory. In higher tiers you never see the cards at all; you deduce them from the results of the merges you make.

PLAY YOUR WAY
• Classic — the 24 game: make 24, every time.
• Custom — choose your own target, or let the game pick one.
• Six tiers from Easy (unlimited time, five cards) to Blind (three cards, never shown).

BUILT FOR THE COUNT
• Exact arithmetic — fractions matter, and they're shown properly, stacked.
• Hints that guide without spoiling: which two cards, then the operator, then the result, then the whole solution.
• Undo anything, including a wrong merge.
• Reveal every distinct solution when you're stuck (it costs you).
• Score with tier multipliers and a time bonus.

No accounts. No ads. No network. Just cards, numbers and a clock.
```

## Keywords (100, comma-separated)

```
make 24,math,card,puzzle,arithmetic,mental math,numbers,brain,memory,solitaire,poker,quiz
```

## URLs

| Field | Value |
|---|---|
| Support URL | https://snap24.marcustut.me/support |
| Marketing URL | https://snap24.marcustut.me |
| Privacy policy URL | https://snap24.marcustut.me/privacy |

## App Privacy

- **Data collection:** none. The app has no network access and no analytics, so the
  privacy answers are "Data Not Collected" across the board.

## Screenshots

`ios/app-store/screenshots/` — 6.9" (1320×2868), captured from the running app:

| File | Shows |
|---|---|
| `01-title.png` | title |
| `02-mode.png` | mode select |
| `03-difficulty.png` | difficulty slider |
| `04-custom-target.png` | custom target keypad |
| `05-board.png` | a Classic hand in play |
| `06-face-down.png` | the hide mechanic |
| `07-solved.png` | a solved round |

## Notes before submission

- **The name.** Bare `Snap 24` is taken in App Store Connect, so the store name is
  `Snap 24 - Card Maths` — only the 30-char Name field must be unique; the subtitle
  and home-screen label don't, so the icon still reads `Snap 24`. Confirm the exact
  string is free in App Store Connect when you create the app record.
- **Don't title it "24 Game"** — Suntex International owns that trademark (they ship
  "24 Game – Math Card Puzzle"). Avoid the phrase in the name and, ideally, the
  keywords too; descriptive "24"/"make 24" is fine.
- **Signing is required and not done here** — see `ios/README.md` ("App Store
  packaging"). You need an Apple Developer team, a distribution certificate and a
  provisioning profile.
- **Verify the timer on a real device.** The hide countdown could not be verified
  in the headless simulator (the app is suspended when not foregrounded).
- Consider a screen recording for the preview video (optional).
