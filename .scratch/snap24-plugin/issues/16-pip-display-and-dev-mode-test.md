# 16: PiP display + dev-mode test

**What to build:** The game in picture-in-picture display mode, tested end-to-end in ChatGPT dev mode.

**Blocked by:** 15

**Status:** done (PiP + local end-to-end); the ChatGPT dev-mode run needs your OpenAI account

- [x] Game runs in PiP and stays responsive while the conversation continues.
- [x] The model can start, hint and validate via tools while the UI is open.
- [ ] Session ends cleanly and returns to inline; a full round plays inside ChatGPT.

**What was built:**

- **PiP:** the resource `_meta` now declares `openai/ui.availableDisplayModes = ["inline", "pip"]`; the widget requests PiP with `window.openai.requestDisplayMode({ mode: "pip" })` (feature-detected) via a **Pop out** button, adapts its layout when `displayMode` changes (compact sizes, listening to `openai:set_globals`), and ends with **Close** → `window.openai.requestClose()`. It stays responsive because all play (taps, timer, undo) is local; the server is only hit for hint/reveal/validation.
- **Tool polish:** tools carry `readOnlyHint`/`destructiveHint`/`openWorldHint` annotations, `render_board` has invocation status text, and the widget description is declared for the model.
- **Local MCP Apps host simulator** (`crates/snap24-mcp/tests/host_sim.mjs`): spawns the real server, serves the widget in an iframe, bridges `tools/call`, and drives a full round in headless Chromium. **14/14 checks pass**: renders from the tool result, countdown visible, tap×2+operator merges locally (no server call), undo, hint/reveal round-trip, full round ends with a **server-validated** result, PiP requested, clean close. It also caught a real bug — leaf cards never set their `expr`, so submissions were `(undefined + undefined)`; fixed.

**Server deployed:** `https://snap24.marcustut.me/mcp` — streamable HTTP behind nginx with an ACME cert on `marcus-server`, via the flake's `services.snap24-mcp` module (see `crates/snap24-mcp/README.md` → "Deploy (NixOS)"). Verified from outside the box: TLS, `initialize` + session, `tools/list` (6 tools), a real `start_puzzle`, and the `ui://snap24/board.html` widget resource.

**Remaining (yours):** add `https://snap24.marcustut.me/mcp` as a connector in ChatGPT with Developer mode enabled, then play a round and check PiP/hint/reveal/close. Until then the last box stays unticked.
