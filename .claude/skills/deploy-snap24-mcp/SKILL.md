---
name: deploy-snap24-mcp
description: Deploy, redeploy, or smoke-test the Snap 24 MCP server (crates/snap24-mcp) on the marcus-server NixOS box, where it runs behind nginx with an ACME cert at https://snap24.marcustut.me/mcp. Use when asked to deploy, ship, release, update, restart, or check the MCP server, or when a ChatGPT/Codex connector needs a fresh build.
---

# Deploying the Snap 24 MCP server

The server runs on the `marcus-server` NixOS box as a systemd service, fronted
by nginx with a Let's Encrypt cert. Everything is declared in two git repos — no
manual steps on the box beyond `nixos-rebuild`.

| Fact | Value |
|---|---|
| Public endpoint | `https://snap24.marcustut.me/mcp` (streamable HTTP) |
| Box | NixOS 26.05, aarch64 EC2, hostname `server` |
| Public IP / tailnet | `54.254.97.240` / `100.64.0.25` |
| SSH | `ssh marcustut/marcus-server` (alias → user `root`) |
| Unit / port | `snap24-mcp.service`, loopback `127.0.0.1:8788` |
| App repo | `github.com/marcustut/snap24` (private) — `flake.nix`, `nix/snap24-mcp.nix` |
| Infra repo | `/etc/nixos` on the box (git, flake, host `server`) |
| TLS | ACME cert at `/var/lib/acme/snap24.marcustut.me/` |

## Access

Root's GitHub key is passphrase-protected but already loaded in the box's
ssh-agent. Non-interactive SSH has no `SSH_AUTH_SOCK`, so nix can't fetch the
private flake inputs until you export one:

```sh
SOCK=$(ssh marcustut/marcus-server 'ls -t /root/.ssh/agent/* | head -1')
# then prefix remote commands with the export, e.g.:
ssh marcustut/marcus-server "export SSH_AUTH_SOCK=$SOCK; <command>"
```

Non-interactive SSH also has no `nix-command`/`flakes` feature enabled — always
pass `--extra-experimental-features "nix-command flakes"`.

## Redeploy after changing server code (the common case)

1. Build and test locally: `cargo test -p snap24-mcp` (`tests/stdio.rs` covers
   the protocol; `tests/host_sim.mjs` drives the widget in a headless browser).
2. Push `main`.
3. On the box, move the pinned input to the new commit, then switch:

   ```sh
   ssh marcustut/marcus-server "export SSH_AUTH_SOCK=$SOCK;
     nix --extra-experimental-features 'nix-command flakes' flake update snap24 --flake /etc/nixos &&
     nixos-rebuild switch --flake /etc/nixos#server"
   ```

   `nix flake update snap24` is **required**: `/etc/nixos/flake.lock` pins the
   app repo, so a rebuild alone keeps building the old revision.
4. Verify (below).

## First-time setup on a new host

Three edits, matching how `axis` and `puchong` are wired:

```nix
# /etc/nixos/flake.nix — input
snap24.url = "git+ssh://git@github.com/marcustut/snap24";

# /etc/nixos/flake.nix — the `server` host's extraModules
inputs.snap24.nixosModules.default

# /etc/nixos/hosts/server/snap24.nix — enabled per host
services.snap24-mcp = { enable = true; domain = "snap24.marcustut.me"; };
```

Then add `./snap24.nix` to `hosts/server/configuration.nix` imports and rebuild.

DNS is a **grey-cloud** `A` record → `54.254.97.240` (Let's Encrypt HTTP-01
needs port 80 to reach the box; Proxying it breaks issuance). `mail.marcustut.me`
is the reference record.

## Verify

```sh
# service + cert
ssh marcustut/marcus-server 'systemctl is-active snap24-mcp; ls /var/lib/acme/snap24.marcustut.me/'

# end to end, from outside the box (TLS, session, a real tool call)
curl -sS -D /tmp/h -X POST https://snap24.marcustut.me/mcp \
  -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"curl","version":"1"}}}'
SID=$(grep -i '^mcp-session-id' /tmp/h | awk '{print $2}' | tr -d '\r')
curl -sS -X POST https://snap24.marcustut.me/mcp -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' -H "Mcp-Session-Id: $SID" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"start_puzzle","arguments":{"mode":"classic"}}}'
```

A `GET /mcp` returning **406** is correct — streamable HTTP wants POST.

## Local HTTP mode (no box needed)

```sh
cargo run -p snap24-mcp -- --http 127.0.0.1:8899   # endpoint: POST /mcp
```

Use port **8899**: `8787` is taken on this Mac by `collie`.

## Gotchas that have bitten us

- **`git+ssh://git@github.com/marcustut/snap24`** — a *slash* after the host.
  The scp-style colon (`github.com:marcustut/...`) is not parseable in the
  `scheme://` form and nix silently falls back to treating the ref as a local
  path (`getting status of "/root/git+ssh:/..."`).
- **Don't pass `?rev=` in a flake ref** — likewise parses as a path. Move the
  input with `nix flake update snap24` instead.
- The box's rustc is **older than dev machines**: avoid freshly-stabilised std
  APIs (`mask.isolate_lowest_one()` failed there). Build once on the box after
  touching `snap24-core`.
- **nginx must not buffer**: `proxy_buffering off` in the `/mcp` location, or
  SSE responses hang. `services.snap24-mcp` already sets this.
- `SNAP24_MCP_ALLOWED_HOSTS` must contain the public host — rmcp only accepts
  loopback `Host` headers by default and otherwise answers **403**. The module
  sets it from `domain`.
- `nginx` is **not on root's PATH** on the box, so `nginx -T | grep …` silently
  returns nothing. Read the active config instead:
  `systemctl cat nginx.service | grep -o "/nix/store/[^ ]*nginx.conf"` then grep
  that file.
- The `/mcp` location is rate limited (120r/m per IP, burst 60 → `429`). It is
  generous on purpose: hosts share egress IPs. Test it from the box over
  **HTTPS on loopback** (`--resolve snap24.marcustut.me:443:127.0.0.1`), since
  plain HTTP just redirects and never reaches the location.
- **`/etc/nixos` often has uncommitted changes** (sops re-encryption, other
  hosts). A local-path flake builds the *working tree*, so a rebuild applies
  them. Commit only the files you changed, and never touch or print anything
  under `/etc/nixos/secrets/` (sops-encrypted, and the working tree may hold
  decrypted copies).
- Check back with `git -C /etc/nixos status --short` before and after; leave
  other people's dirty files alone.

## Bundle size note

The flake builds the workspace with `cargoBuildFlags = [ "-p" "snap24-mcp" ]`,
so the Bevy app's GPU/audio/X11 dependencies stay out of the closure. Adding a
workspace member that `snap24-mcp` depends on is fine; making `snap24-mcp`
depend on `snap24-app` would pull the whole Bevy tree into the server.
