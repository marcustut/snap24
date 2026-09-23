# 03: Solver

**What to build:** The `snap24-core` public solver API — `reachable(cards)`, `is_solvable(cards, target)`, `solve(cards, target)` — returning canonical, de-duplicated expressions with exact rational arithmetic, and passing a differential test matrix against the oracle and fixtures.

**Blocked by:** 01, 02

**Status:** ready-for-agent

- [ ] `reachable()` returns the set of exactly-reachable values; `is_solvable()` early-exits; `solve()` returns each distinct canonical solution exactly once.
- [ ] Arithmetic is exact (integers/rationals) — no float tolerance, no `isclose`.
- [ ] Differential tests: for every fixture, `reachable`, `is_solvable`, and the distinct solution set all match the oracle.
- [ ] Performance: a full `solve` on a 5-card hand completes well under 10 ms in release (the naive Python takes ~116 ms).
- [ ] Property tests over random hands: soundness (every returned expression evaluates to the target using exactly the dealt multiset) and uniqueness (no two returned solutions are equivalent).
- [ ] `cargo test` green.
