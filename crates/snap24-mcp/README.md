# snap24-mcp

The Snap 24 MCP server: the game as a ChatGPT/Codex plugin. It exposes the
solver-backed tools and an optional MCP Apps board widget, all built on
`snap24-core`.

## Tools

The published contract — names, parameters, result shapes, annotations — is
frozen in **[`docs/mcp-surface.md`](../../docs/mcp-surface.md)** and enforced by
`crates/snap24-mcp/tests/surface.rs`. Every tool returns a human summary plus
`structuredContent`; changing either is a release, not a refactor.

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
cargo run -p snap24-mcp            # stdio MCP server (local tools, Codex)
cargo test -p snap24-mcp           # protocol test: handshake, tools, resources, widget meta
```

### Streamable HTTP (what ChatGPT connectors need)

```sh
cargo run -p snap24-mcp -- --http 127.0.0.1:8899
# MCP endpoint: POST http://127.0.0.1:8899/mcp
```

For a public deployment, name your host — rmcp only accepts loopback `Host`
headers by default, to block DNS-rebinding attacks:

```sh
SNAP24_MCP_ALLOWED_HOSTS=snap24.marcustut.me \
  cargo run -p snap24-mcp -- --http 127.0.0.1:8899
```

Requests with any other `Host` get `403`. `SNAP24_MCP_ALLOWED_HOSTS` is a
comma-separated list; it replaces the default `localhost,127.0.0.1,::1`.

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

## Deploy (NixOS)

Live at **`https://snap24.marcustut.me/mcp`** (streamable HTTP, behind nginx with
an ACME cert on `marcus-server`).

The repo ships a flake + NixOS module, so a host only needs:

```nix
# flake.nix
inputs.snap24.url = "git+ssh://git@github.com/marcustut/snap24";

# the host's extraModules
inputs.snap24.nixosModules.default

# hosts/<host>/snap24.nix
services.snap24-mcp = {
  enable = true;
  domain = "snap24.marcustut.me";
};
```

That builds the server from this repo (workspace-aware: only `-p snap24-mcp` is
compiled, so the Bevy app's GPU/X11 dependencies stay out of the closure), runs
it under systemd on `127.0.0.1:8788` with `DynamicUser` and hardening turned on,
and adds an nginx `location /mcp` with `proxy_buffering off` — MCP answers over
SSE, so buffered responses would hang.

`domain` also feeds `SNAP24_MCP_ALLOWED_HOSTS`: rmcp only accepts loopback
`Host` headers by default, so a public deployment that forgets it answers `403`.

## Testing it in ChatGPT (dev mode)

This is the part that needs your OpenAI account and a reachable server:

1. **Deploy over streamable HTTP** — done: `https://snap24.marcustut.me/mcp`
   (see "Deploy (NixOS)" above).
2. In ChatGPT, enable **Developer mode** and add the MCP server URL as a
   connector.
3. Ask it to *"play Snap 24"* → it calls `start_puzzle` then `render_board`, and
   the board should appear inline. Tap **Pop out** to check PiP stays responsive
   while you keep chatting, **Hint**/**Reveal** to check the round-trips, and
   **Close** to return to inline.
