# Snap 24 — Canonical form for solutions

This spec defines when two expressions that solve the same puzzle count as **the same solution**.
The Python oracle, the Rust solver, the golden fixtures, and the UI's "all solutions" screen must all
agree on it.

## Representation

A solution is a binary expression tree:

- **leaf** — one dealt card value (`A=1 … K=13`).
- **node** — `(op, left, right)` where `op ∈ {+, -, *, /}`.

Duplicate ranks in a hand are indistinguishable: swapping two equal-valued leaves does not produce a
new solution.

## Three levels of sameness

For `[1, 1, 1, 1, 8]` → 24:

| Level | Definition | Count |
|---|---|---|
| Raw trees | every distinct tree that evaluates to 24 | **264** |
| Exact string | `eval.py`'s fully-parenthesized rendering, compared as text | **18** |
| **Canonical** | trees differing only by commutativity/associativity of `+` and `*` are equal | **10** |

**We use the canonical level.** It is the one that matches what a player would call "a different way
to solve it": `((1+1)+1)+1` and `1+(1+(1+1))` are one solution, not four.

## Canonicalization rules

1. A leaf canonicalizes to its value.
2. `+` and `*` are associative and commutative. Flatten nested chains of the *same* operator into an
   n-ary node and sort its operands by rule 5.
3. `-` and `/` are neither associative nor commutative. They stay binary and **ordered**: `a - b` and
   `b - a` are different solutions; so are `a / b` and `b / a`.
4. **No other simplification.** Identities (`x * 1`, `x / 1`, `x + 0`) are *not* normalized away, and
   neither are distributive rewrites. So `8 * (1 + 1 + 1*1)` and `8 * (1 + 1 + 1/1)` are two
   different solutions even though both equal 24. We accept this for predictability.
5. **Total order for commutative operands.** Define the canonical serialization `S`:
   - leaf `v` → the decimal string of `v`;
   - n-ary node → `"(" + op + children.map(S).join(",") + ")"`.
   Sort a commutative node's operands by **byte-wise lexicographic comparison of `S`** (computing each
   child's serialization first). This order is deterministic and language-independent.
6. Two solutions are the same **iff** their canonical serializations are equal.

## Worked examples

- `[1,1,1,1,8]` → 24 → **10** distinct canonical solutions (264 raw, 18 by exact string).
- `[9,10,9,9,1]` → 24 → **0** (unsolvable — note this is the example hard-coded in the original
  `eval.py`, which silently reports nothing).

## Non-goals

- Algebraic identities.
- Distributive rewrites (`a*(b+c)` vs `a*b + a*c`).
- Rewriting `a / b` as `a * (1/b)`.

Changing any rule here invalidates every golden fixture, so it is a deliberate, versioned decision.
