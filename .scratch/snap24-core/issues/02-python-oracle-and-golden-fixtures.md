# 02: Python oracle + golden fixtures

**What to build:** An independent, deliberately-simple Python oracle that finds puzzle solutions, and a committed golden fixture set mapping `(cards, target)` → distinct canonical solutions, generated from it. This is the correctness reference the Rust solver is later diffed against — deliberately a *different* algorithm so bugs don't correlate.

**Blocked by:** 01 (needs the canonical-form spec)

**Status:** ready-for-agent

- [ ] Oracle enumerates naively (permutations × binary trees × operator assignments), applies the canonical form from ticket 01, and de-duplicates.
- [ ] Oracle reproduces the ticket-01 worked example under the agreed canonical form (`[1,1,1,1,8]` → 24 = 10 distinct if commutativity/associativity only; equals whatever ticket 01 records).
- [ ] `fixtures.json` records the canonical form it was generated with, so a fixture and the code that checks it can never silently disagree.
- [ ] `fixtures.json` committed, covering 3/4/5/6-card hands, target 24 and custom targets, duplicate ranks, and known-unsolvable hands (including `[9,10,9,9,1]` → 24 = 0).
- [ ] A documented command regenerates `fixtures.json` from the oracle.
