# 17: Submission prep

**What to build:** A Snap 24 plugin submission that passes review.

**Blocked by:** 16

**Status:** package + assets + review materials done; submission needs the OpenAI dashboard

- [x] Tools, skills and UI packaged per the plugin guidelines; metadata and descriptions written.
- [x] CSP and iframe policy compliant; security/privacy notes written.
- [ ] Submitted and passing validation, with review errors resolved.

## What is ready

`plugin/` is the uploadable package (`snap24-1.0.0.zip`, gitignored, rebuild with
`cd plugin && zip -qr snap24-1.0.0.zip plugin.json mcp.json skills assets`):

| Piece | Where |
|---|---|
| `plugin.json` (Agent Plugins format) | display name, listing text, four required URLs, icons, screenshots, brand colours, 5 positive + 3 negative review cases, demo URL, onboarding skill |
| `mcp.json` | `https://snap24.marcustut.me/mcp` (streamable HTTP) |
| `skills/get-started/SKILL.md` | how a model should run a game |
| `assets/` | 512 logo, 256 composer icon, two 1440×1960 screenshots (from the recording) |

Hosted on the box (see `nix/snap24-mcp.nix`):

- `/` `/support` `/privacy` `/terms` — the four URLs the MCP review requires, same
  publisher, served from `site/`.
- `/media/snap24-demo.mp4` — 23s captioned walkthrough, recorded by
  `crates/snap24-mcp/tests/record_demo.mjs` against the deployed server. It deals,
  merges on the board, shows a hint, reveals, finishes the hand with local taps,
  and shows the server accepting the final expression.
- `/.well-known/openai-apps-challenge` — served once
  `services.snap24-mcp.openaiChallengeToken` is set to the dashboard's token.

The 8 review cases were run against the live server (`8/8`), covering deal, board
re-open, hint progression, a reused-card rejection, reveal→submit acceptance, and
that no tool can do the three negative asks.

## Left for a human

1. Verified developer identity + org permissions in the OpenAI dashboard.
2. Upload the ZIP; work through metadata/skills findings.
3. Connect the MCP server: paste the dashboard's token into
   `services.snap24-mcp.openaiChallengeToken`, redeploy, then verify the challenge
   URL, connect and scan.
4. Demo video URL is already hosted; paste it in Review details if it is not
   imported from the ZIP.
5. Confirm `support@marcustut.me` is a real mailbox (used on the privacy, terms
   and support pages).
6. Attestations → submit.
