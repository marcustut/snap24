# 07: Timer + hide mechanic

**What to build:** The view-window timer and face-down solving mode — the mechanic that makes Snap 24 its own game. Medium/hard tiers memorize then solve from memory, with card values hidden but slot positions stable.

**Blocked by:** 06

**Status:** ready-for-agent

- [ ] Timer driven by the tier's view time; Easy stays visible indefinitely.
- [ ] On expiry, cards flip face-down but keep stable slot positions so recall is fair.
- [ ] Merging works on face-down cards; the merged result is revealed per the agreed rule (decide and record: show the computed merge result).
- [ ] Blind tier never shows the cards.
- [ ] Countdown is visible during viewing; each tier manually tested.
