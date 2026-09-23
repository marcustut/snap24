# 09: Hints + reveal

**What to build:** A progressive hint system and a solution reveal, both driven by `snap24-core`, so a stuck player has a path and a failed player sees the answer.

**Blocked by:** 04, 06

**Status:** ready-for-agent

- [ ] Hint advances one level per use: which two cards → the operator → the sub-result → the full solution.
- [ ] Each hint costs score.
- [ ] Reveal lists distinct solutions (canonical, de-duplicated) for the current puzzle.
- [ ] Hints never contradict the evaluator; reveal output re-validates through the evaluator.
- [ ] Manual test on a known hand.
