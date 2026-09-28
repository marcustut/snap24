# snap24-mcp

The Snap 24 MCP server: the game as a ChatGPT/Codex plugin. It exposes the
solver-backed tools and an optional MCP Apps board widget, all built on
`snap24-core`.

## Tools

| Tool | Read-only | Does |
|---|---|---|
| `start_puzzle` | no | deal Classic (target 24) or Custom (random **or** a typed target), any tier, optional `seed` |
| `submit_solution` | yes | validate the player's expression server-side (exact arithmetic) |
| `hint` | yes | progressive hint (cards → operator → result → full), one level per call |
| `reveal` | yes | distinct canonical solutions |
| `explain` | yes | step-by-step walkthrough of a solution |
| `render_board` | yes | returns the MCP Apps widget for a dealt `puzzle_id` |

Puzzles live in memory keyed by an opaque id with a 30-minute TTL. Every tool
works **without UI**, so the model can coach and validate in plain chat.

## The board widget

`ui/board.html` is served as an MCP Apps resource (`ui://snap24/board.html`,
`text/html;profile=mcp-app`) and linked to `render_board` via
`_meta.ui.resourceUri`. It is a single self-contained HTML file — no React, no
build step, no WASM, no external assets — and talks to the host over the MCP
Apps bridge (`postMessage` JSON-RPC).

- Board, card taps (infix), operator keys, the view timer and undo run **in the
  widget**, with exact rational maths.
- Only the round's final expression, `hint`, `reveal`, and a fresh deal
  round-trip to the server. The final result is **validated by the server**.
- `_meta` declares `openai/ui.availableDisplayModes = ["inline", "pip"]`, an
  empty CSP (nothing external is loaded, and it makes no network calls), and a
  widget description. The widget requests picture-in-picture via
  `window.openai.requestDisplayMode({ mode: "pip" })` (feature-detected) and
  closes with `window.openai.requestClose()`.

## Run

```sh
cargo run -p snap24-mcp            # stdio MCP server
cargo test -p snap24-mcp           # protocol test: handshake, tools, resources, widget meta
```

## Local end-to-end (host simulator)

`tests/host_sim.mjs` plays the role of ChatGPT: it spawns the server, serves the
widget in an iframe, bridges `tools/call` to the server, and drives a full round
in headless Chromium — verifying the board renders from tool results, merges and
undo stay local, hints/reveal/validation round-trip, PiP is requested and the
session closes.

```sh
cargo build -p snap24-mcp
npm i playwright && npx playwright install chromium   # once
node crates/snap24-mcp/tests/host_sim.mjs
```

If Playwright isn't resolvable from the repo, point at it:
`PLAYWRIGHT=/path/to/playwright/index.mjs node crates/snap24-mcp/tests/host_sim.mjs`.

## Testing it in ChatGPT (dev mode)

This is the part that needs your OpenAI account and a reachable server:

1. **Deploy over streamable HTTP.** ChatGPT needs a remote HTTPS endpoint;
   stdio is for local tools/Codex. Add
   `rmcp = { features = ["transport-streamable-http-server"] }` and serve
   `StreamableHttpService` (a small `axum`/`hyper` binary), then put it behind
   HTTPS — `snap24.marcustut.me` is the natural host.
2. In ChatGPT, enable **Developer mode** and add the MCP server URL as a
   connector.
3. Ask it to *"play Snap 24"* → it calls `start_puzzle` then `render_board`, and
   the board should appear inline. Tap **Pop out** to check PiP stays responsive
   while you keep chatting, **Hint**/**Reveal** to check the round-trips, and
   **Close** to return to inline.
