# 04: Submission evaluator

**What to build:** A function that parses a player-submitted expression (e.g. `(9 - 1) * 3`), verifies it uses exactly the dealt cards once each, and returns either the exact value or a typed error. This is the trust boundary for submissions — the server never trusts a client's "correct" claim.

**Blocked by:** 01, 03

**Status:** ready-for-agent

- [ ] Parser handles `+ - * /`, parentheses and multi-digit numbers; rejects malformed input with a typed error.
- [ ] Rejects expressions using a value not in the hand, or a dealt value more times than it was dealt.
- [ ] Returns the exact rational value; accepts iff it equals the target.
- [ ] Unit tests cover malformed input, wrong multiset, division by zero, and correct/incorrect values.
- [ ] `cargo test` green.
