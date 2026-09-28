# 15: MCP Apps UI

**What to build:** A thin in-iframe UI that renders the board, runs the timer, handles card taps, and calls the server tools only for validation and hints.

**Blocked by:** 14

**Status:** done

- [x] UI resource declared with a `ui://` URI and the MCP Apps MIME type, rendering alongside the conversation.
- [x] Board, timer and card taps work; only validation/hints round-trip to the server.
- [x] Renders from tool results via the MCP Apps bridge and treats all input as untrusted.
- [x] No WASM and no heavy bundle; the CSP declares exactly what is needed.

**What was built:** the board widget at `crates/snap24-mcp/ui/board.html`, served by the MCP server as an MCP Apps resource:
- Resource `ui://snap24/board.html`, MIME `text/html;profile=mcp-app`, returned by `resources/read` with `_meta.ui.csp` = empty `connectDomains`/`resourceDomains` (the widget loads nothing external).
- A **render tool**, `render_board(puzzle_id)`, carries `_meta.ui.resourceUri` (the standard field) so the host mounts the widget; it returns `structuredContent` (`puzzle_id`, `mode`, `difficulty`, `cards`, `target_n/d`, `view_seconds`) via `Json<BoardView>`. Data tools stay UI-free, per the decoupled pattern (`start_puzzle` → `render_board`).
- The widget is a **single inline HTML file** — no React/esbuild step, no WASM, no external assets or CDNs — that talks to the host over the **MCP Apps bridge** (`postMessage` JSON-RPC): renders from `ui/notifications/tool-result`, calls tools with `tools/call`.
- **Local play, server only for truth:** card taps (infix), operator keys, timer countdown/hide, undo and merges all run in the widget with exact rational maths; it round-trips only for `submit_solution` (builds the fully-parenthesised expression and lets the **server** decide), `hint`, `reveal`, and a new `start_puzzle` + `render_board` on "New puzzle"/"Give up".
- **Untrusted input:** all dynamic values are written with `textContent` (never `innerHTML`) and numbers coerced; nothing is trusted from the tool result beyond validating shape (`puzzle_id` present).

**Verified:** the stdio integration test now also asserts the widget resource is listed with the MCP Apps MIME type, that `resources/read` returns the HTML with the bridge hooks (`ui/notifications/tool-result`, `tools/call`) and a declared CSP, that `render_board` advertises `_meta.ui.resourceUri`, and that it returns the expected `structuredContent`. The widget's JS passes `node --check`.

**Caveat:** rendering *inside ChatGPT* is ticket 16 (PiP + dev-mode test); here it's verified at the protocol level.
