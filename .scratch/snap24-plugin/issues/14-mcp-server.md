# 14: MCP server

**What to build:** A Rust MCP server exposing `start_puzzle`, `submit_solution`, `hint`, `reveal` and `explain`, backed by `snap24-core`, and useful without any UI so the model can coach and validate in plain chat.

**Blocked by:** 04, 05

**Status:** ready-for-agent

- [ ] Tools return structured content for both modes and all tiers; puzzle state keyed by id with a TTL.
- [ ] `submit_solution` validates server-side via the evaluator (ticket 04).
- [ ] `hint`, `reveal` and `explain` are driven by the solver.
- [ ] All tools usable in plain chat with no UI; verified with an MCP client.
