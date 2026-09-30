---
name: snap24-get-started
description: How to run a game of Snap 24 with this plugin — deal a hand, play it on the board, and use hints, reveal and validation. Use when someone wants to play Snap 24, or asks how it works.
---

# Playing Snap 24

The goal: combine **every** dealt card exactly once, using `+ - * /`, to reach the
target. In Classic that target is 24 with five cards; Custom deals a different
number of cards and a target drawn from the hand. Cards are ranks — A=1, J=11,
Q=12, K=13 — and intermediate results may be fractions.

## Running a game

1. **Deal.** Call `start_puzzle` (say which `mode` and `difficulty` if the player
   asked). It returns the cards and target **and opens the interactive board**, so
   the player can tap two cards and an operator to merge them.
2. **Let them play.** The board does the merging locally. Don't solve it for them
   unless they ask — the game is the arithmetic.
3. **Hint when asked.** `hint` gives one level at a time: which two cards, then
   the operator, then the sub-result, then the full solution. Use it before
   `reveal` — it keeps the game going.
4. **Validate a solution.** Pass the player's expression to `submit_solution`.
   It uses exact arithmetic: `(10 - 8) * 6 * 1 * 2` is checked card by card, so
   near-misses and repeated cards are both caught. Report the verdict plainly.
5. **When they are stuck.** `reveal` lists the distinct solutions (the count is
   usually large — a hand can have hundreds). `explain` walks one through as
   ordered merge steps.
6. **Next hand.** They can ask for another deal at any time; call `start_puzzle`
   again with the difficulty they want, or without arguments to keep it simple.

## Things worth knowing

- Higher difficulties hide the cards after a few seconds (`view_seconds`); that
  is the challenge, not a defect. Blind never shows them, so describe the hand
  by its card count and target only.
- A `puzzle_id` is required for every follow-up call, and it is the handle for
  hints, reveal, explain and validation. If a call reports an unknown or expired
  id, deal a fresh hand and say so.
- Hands are not stored: nothing about a game outlives it, and no account is
  needed.
- `render_board` re-opens the board if it was closed or the player scrolled away.
