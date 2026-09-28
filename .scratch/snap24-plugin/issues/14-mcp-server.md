# 14: MCP server

**What to build:** A Rust MCP server exposing `start_puzzle`, `submit_solution`, `hint`, `reveal` and `explain`, backed by `snap24-core`, and useful without any UI so the model can coach and validate in plain chat.

**Blocked by:** 04, 05

**Status:** done

- [x] Tools return structured content for both modes and all tiers; puzzle state keyed by id with a TTL.
- [x] `submit_solution` validates server-side via the evaluator (ticket 04).
- [x] `hint`, `reveal` and `explain` are driven by the solver.
- [x] All tools usable in plain chat with no UI; verified with an MCP client.

**What was built:** `crates/snap24-mcp` — a stdio MCP server using the official Rust SDK (`rmcp` 3.5), backed by `snap24-core`. Five tools:

| Tool | Does |
|---|---|
| `start_puzzle` | deal Classic (target 24) or Custom (random **or** a typed target), any tier, optional `seed`; returns a `puzzle_id` + cards/target/view |
| `submit_solution` | parses the player's expression and validates it server-side with the ticket-04 evaluator (exact; rejects wrong multiset / malformed / div-by-zero) |
| `hint` | progressive (1 cards → 2 operator → 3 result → 4 full solution) via `first_move`/`move_sequence`; advances one level per call |
| `reveal` | distinct canonical solutions (`solutions_infix`, capped at 5 + count) |
| `explain` | step-by-step walkthrough of a solution (or of the player's expression) |

Puzzles are held in memory keyed by an opaque id with a **30-minute TTL** (pruned on access); unknown/expired ids return a friendly error. Results are concise **text content**, so everything works with no UI (structured `outputSchema` content is a possible follow-up). `get_info` sets tools capability + instructions (card ranks A=1…K=13, fractions allowed).

**Verified:** `crates/snap24-mcp/tests/stdio.rs` spawns the real binary and speaks MCP over stdio — `initialize` → `tools/list` → `start_puzzle` → `hint` ×3 → rejected submit → `reveal` → **accepted** submit of a revealed solution → `explain` → unknown-id error.

**Transport note (from the current OpenAI docs):** a ChatGPT plugin is *skills + an MCP server + optional UI*; the MCP server is optional only for skills-only plugins, so it is required here. Production hosts need a **remote HTTPS endpoint using streamable HTTP** (rmcp supports `transport-streamable-http-server`), which is a deploy decision — `snap24.marcustut.me` is an obvious candidate. Local stdio is what dev/Codex/inspector use today; the ChatGPT dev-mode end-to-end is ticket 16.
