# 04: Submission evaluator

**What to build:** A function that parses a player-submitted expression (e.g. `(9 - 1) * 3`), verifies it uses exactly the dealt cards once each, and returns either the exact value or a typed error. This is the trust boundary for submissions — the server never trusts a client's "correct" claim.

**Blocked by:** 01, 03

**Status:** done

- [x] Parser handles `+ - * /`, parentheses and multi-digit numbers; rejects malformed input with a typed error.
- [x] Rejects expressions using a value not in the hand, or a dealt value more times than it was dealt.
- [x] Returns the exact rational value; accepts iff it equals the target.
- [x] Unit tests cover malformed input, wrong multiset, division by zero, and correct/incorrect values.
- [x] `cargo test` green.

**Notes:** `pub fn evaluate(cards: &[i64], input: &str) -> Result<Rational, EvalError>` in `crates/snap24-core/src/submission.rs`, re-exported from the crate root. Recursive-descent parser (precedence `* /` over `+ -`, parentheses, whitespace ignored, multi-digit integers) with exact `Rational` arithmetic; the leaf multiset must equal the dealt cards exactly (duplicates included). Typed `EvalError` (`Empty`, `UnexpectedChar`, `UnexpectedEnd`, `NumberTooLarge`, `UnexpectedRParen`, `MissingRParen`, `TrailingInput`, `DivisionByZero`, `WrongCards { used, dealt }`) implements `Display` + `Error`. The caller accepts a submission iff the returned value equals the target (server-side, ticket 14).
