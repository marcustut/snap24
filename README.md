# Snap 24

The 24 game: deal a hand, combine every card exactly once with `+ - * /`, and
land on the target. Four things live here — the game logic, a native app, an MCP
server that plays it inside ChatGPT, and the NixOS deployment that serves it.

- **Play it in ChatGPT:** <https://snap24.marcustut.me> (plugin) — MCP endpoint
  `https://snap24.marcustut.me/mcp`
- **Play it on iPhone:** the app in `crates/snap24-app` (see
  [`ios/README.md`](crates/snap24-app/ios/README.md))

## Layout

| Path | What |
|---|---|
| `crates/snap24-core` | exact-rational solver, submission evaluator, puzzle generator |
| `crates/snap24-app` | the Bevy game (desktop + iOS), fonts/SFX baked in |
| `crates/snap24-mcp` | MCP server: six tools, plus the interactive board widget |
| `plugin/` | ChatGPT plugin package (manifest, icons, screenshots, onboarding skill) |
| `site/` | landing, support, privacy and terms pages served from the box |
| `nix/` | NixOS module that runs the server behind nginx with ACME |
| `docs/` | the frozen MCP surface contract |
| `.claude/skills/deploy-snap24-mcp` | how to deploy the server (read it before touching prod) |

## Build and test

```sh
cargo test          # core solver, app logic, MCP protocol, surface contract
cargo clippy --all-targets
cargo run -p snap24-app          # the desktop game
```

The widget has its own end-to-end harness — it acts as an MCP Apps host in a
headless browser and drives a full round:

```sh
cargo build -p snap24-mcp
npm i playwright && npx playwright install chromium    # once
node crates/snap24-mcp/tests/host_sim.mjs              # 20 checks
```

## The MCP server

```sh
cargo run -p snap24-mcp                             # stdio (local tools, Codex)
cargo run -p snap24-mcp -- --http 127.0.0.1:8899    # streamable HTTP at POST /mcp
```

Six tools: `start_puzzle` and `render_board` (both open the board),
`submit_solution`, `hint`, `reveal`, `explain`. Every tool returns a human
summary **and** structured content.

**The tool surface is a published contract.** Names, parameters and result
shapes are frozen in [`docs/mcp-surface.md`](docs/mcp-surface.md) and enforced by
`crates/snap24-mcp/tests/surface.rs`, because changing them is a release for
every host. Add fields; never rename or repurpose one.

## Deploy

`nix/snap24-mcp.nix` is a NixOS module: it builds the server from this flake,
runs it under systemd on `127.0.0.1:8788`, serves the listing pages and demo
media, and puts nginx in front with an ACME certificate.

```nix
inputs.snap24.url = "github:marcustut/snap24";
# host: inputs.snap24.nixosModules.default
services.snap24-mcp = { enable = true; domain = "snap24.marcustut.me"; };
```

Full runbook, including the traps: [`.claude/skills/deploy-snap24-mcp`](.claude/skills/deploy-snap24-mcp/SKILL.md).

## Publishing

- **ChatGPT plugin:** the uploadable ZIP is built from `plugin/`
  (`cd plugin && zip -qr snap24-1.0.0.zip plugin.json mcp.json skills assets`).
  The submission checklist is in
  [`.scratch/snap24-plugin/issues/17-submission-prep.md`](.scratch/snap24-plugin/issues/17-submission-prep.md).
- **Demo video and listing screenshots:** `node crates/snap24-mcp/tests/record_demo.mjs`
  records them against the deployed server.

## Licence

MIT — see [`LICENSE`](LICENSE). The Snap 24 name, the board's visual design and
its icons are not covered by that licence.
