# 03: Solver

**What to build:** The `snap24-core` public solver API — `reachable(cards)`, `is_solvable(cards, target)`, `solve(cards, target)` — returning canonical, de-duplicated expressions with exact rational arithmetic, and passing a differential test matrix against the oracle and fixtures.

**Blocked by:** 01, 02

**Status:** done

- [x] `reachable()` returns the set of exactly-reachable values; `is_solvable()` early-exits; `solve()` returns each distinct canonical solution exactly once.
- [x] Arithmetic is exact (integers/rationals) — no float tolerance, no `isclose`.
- [x] Differential tests: for every fixture, `reachable`, `is_solvable`, and the distinct solution set all match the oracle.
- [x] Performance: a full `solve` on a 5-card hand completes well under 10 ms in release (the naive Python takes ~116 ms).
- [x] Property tests over random hands: soundness (every returned expression evaluates to the target using exactly the dealt multiset) and uniqueness (no two returned solutions are equivalent).
- [x] `cargo test` green.

**Notes:** Public API in `crates/snap24-core/src/lib.rs`: `reachable(&[i64]) -> BTreeSet<Rational>`, `is_solvable(&[i64], impl Into<Rational>) -> bool`, `solve(&[i64], impl Into<Rational>) -> Vec<String>` (canonical serializations, sorted). Exact `Rational` (i64 num/den, always reduced, positive denominator) so `[1,1,3,4]` → `7/2` works exactly. `is_solvable` skips expression-tree construction (that is its early exit); `reachable` is a value-only subset DP; `solve` is a subset DP over canonical trees deduped structurally. Release `solve` for 5 cards is well under the 10 ms bar (asserted by `solve_five_cards_is_fast`). Differential test at `tests/fixtures.rs` reads `tests/fixtures.json` (`serde_json` dev-dependency). `cargo clippy --all-targets` is clean.
