# 15: MCP Apps UI

**What to build:** A thin in-iframe UI that renders the board, runs the timer, handles card taps, and calls the server tools only for validation and hints.

**Blocked by:** 14

**Status:** ready-for-agent

- [ ] UI resource declared with a `ui://` URI and the MCP Apps MIME type, rendering alongside the conversation.
- [ ] Board, timer and card taps work; only validation/hints round-trip to the server.
- [ ] Renders from tool results via the MCP Apps bridge and treats all input as untrusted.
- [ ] No WASM and no heavy bundle; the CSP declares exactly what is needed.
