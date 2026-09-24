# 09: Hints + reveal

**What to build:** A progressive hint system and a solution reveal, both driven by `snap24-core`, so a stuck player has a path and a failed player sees the answer.

**Blocked by:** 04, 06

**Status:** done

- [x] Hint advances one level per use: which two cards → the operator → the sub-result → the full solution.
- [x] Each hint costs score.
- [x] Reveal lists distinct solutions (canonical, de-duplicated) for the current puzzle.
- [x] Hints never contradict the evaluator; reveal output re-validates through the evaluator.
- [x] Manual test on a known hand.

**Notes / decisions:**

- New core primitives in `crates/snap24-core/src/lib.rs`: `reachable_values`/`is_solvable_values`/`solve_values`/`solutions_infix` over `Rational` board values (mid-game cards can be fractions), `first_move` (searches pairs×operators and returns the first move whose resulting board is still solvable, so a hint can never strand the player), `move_sequence`, and `Move { left, right, op, result }`. Also `Expr::to_infix`, which renders a solution in standard `+ - * /` so it can be fed back through the submission `evaluate` (ticket 04).
- Hint levels (one per press, capped at 4): 1 "combine X and Y", 2 "use '<op>'", 3 "X <op> Y = Z", 4 the whole move sequence from the current board. Each press does `hints_used += 1`; `round_score` already subtracts `HINT_PENALTY` per hint.
- Reveal lists the distinct solutions for the **original dealt hand** (kept in `Game.dealt`), each filtered through `evaluate` before display; it shows up to 4 plus "(+N more)" so a 44-solution hand doesn't shove the board off screen.
- A bug the devtools harness caught: `first_move` recorded operands in board order, not the order that produced the result, so `-`/`/` equations rendered reversed (`8 / 11 = 11/8`). Fixed by making `candidate_moves` carry the operand order; core test asserts `left <op> right == result` for every step.
- `cargo test` = 28 app / 27 core / 2 fixtures; clippy clean (default and `devtools`). The harness gained `hint`/`reveal` steps, so the whole flow is script-verifiable.
