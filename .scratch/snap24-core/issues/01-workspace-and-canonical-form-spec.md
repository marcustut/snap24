# 01: Workspace + canonical-form spec

**What to build:** A `snap24` Cargo workspace with a `snap24-core` library crate that compiles, plus a committed one-page spec defining when two solutions to a puzzle count as "the same" (canonical form). From the developer's perspective: `cargo build` succeeds and the canonical-form spec is the agreed reference that every later ticket (oracle, fixtures, solver, UI dedup) implements.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] Cargo workspace at the repo root with `crates/snap24-core` as a library crate; `cargo build` and `cargo test` succeed.
- [x] `eval.py` preserved as `crates/snap24-core/tests/reference/eval.py` (reference only, not compiled).
- [x] Canonical-form spec committed as markdown (`docs/canonical-form.md`), covering: commutative operand ordering for `+` and `*`, `a - b` vs `b - a` direction, how division and association are treated, and how duplicate-value cards are collapsed.
- [x] Spec includes worked examples. For `[1,1,1,1,8]` → 24: **264** raw trees, **18** string-distinct, **10** canonical. The chosen form and its number are stated explicitly.
- [x] The spec records that algebraic identities (`x*1`, `x/1`, `x+0`) are **not** normalized away.
- [x] The spec defines a deterministic total order for sorting commutative operands (byte-wise over canonical serialization).

**Notes:** spec hard-codes no fixture value that the spec itself doesn't derive. Repo is not a Git repo, so "committed" means written to disk.
