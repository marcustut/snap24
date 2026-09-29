# Snap 24 MCP surface (v1)

The published contract. Tool names, parameters, annotations and result shapes are
what hosts and models see; changing any of them is a **release**, not a
refactor. `crates/snap24-mcp/tests/surface.rs` fails the build if this document
and the server drift apart.

Endpoint: `https://snap24.marcustut.me/mcp` (streamable HTTP) or stdio.

## Rules for changing it

- **Additive only.** Add new optional parameters and new fields; never rename or
  remove. A renamed tool or parameter is a breaking change for every host.
- **Add a field, don't repurpose one.** Deprecated fields keep their meaning and
  keep being sent.
- **Prose is display, `structuredContent` is the contract.** Every tool returns
  both: `content[0].text` is a human/model summary that may be reworded freely,
  `structuredContent` follows the schemas below.
- **Bump `UI_URI`** (`ui://snap24/board.html` → `v2`) if the widget's expectation
  of its payload changes; hosts cache resources by URI.
- Bump `version` in `serverInfo` (`snap24` + crate version) on every surface
  change. `Cargo.toml` is the single source of truth.

## Identity

| Field | Value |
|---|---|
| `serverInfo.name` | `snap24` |
| `serverInfo.version` | crate version (`snap24-mcp`) |
| Capabilities | `tools`, `resources` |
| Instructions | deal → submit/hint/reveal/explain, `A=1 J=11 Q=12 K=13`, fractions allowed, `start_puzzle`/`render_board` open the board |

## Tools

Every tool: `additionalProperties: false`, an `outputSchema`, a `title`,
`destructiveHint: false`, `openWorldHint: false`, and `readOnlyHint: true`
except `start_puzzle` (it creates server state).

| Tool | Required | Optional | readOnly |
|---|---|---|---|
| `start_puzzle` | — | `mode`, `difficulty`, `target`, `seed` | no |
| `render_board` | `puzzle_id` | — | yes |
| `hint` | `puzzle_id` | `level` | yes |
| `reveal` | `puzzle_id` | — | yes |
| `explain` | `puzzle_id` | `expression` | yes |
| `submit_solution` | `puzzle_id`, `expression` | — | yes |

`mode`: `enum ["classic", "custom"]` (default `classic`).
`difficulty`: `enum ["easy", "medium", "hard", "expert", "insane", "blind"]`
(default `easy`).
`target`: integer, custom mode only — a target the dealt hand can actually reach.
`seed`: integer, for a reproducible deal.
`level`: `0` (default) = advance one level per call, else `1..4`.

`start_puzzle` and `render_board` both carry
`_meta.ui.resourceUri = ui://snap24/board.html`, so a host can open the board on
the deal, and both return the **same** payload — the widget renders from either.

## Results

`BoardPayload` — `start_puzzle`, `render_board`:

```json
{
  "puzzle_id": "p18d9c84ec3851359-1",
  "mode": "classic",
  "difficulty": "easy",
  "cards": [8, 12, 1, 6, 10],
  "target": { "n": 24, "d": 1 },
  "view_seconds": null
}
```

`view_seconds`: `null` = face-up indefinitely, `0` = never shown (blind),
otherwise the seconds before the hand hides.

`SolveVerdict` — `submit_solution`:

```json
{
  "accepted": false,
  "value": { "n": 37, "d": 1 },
  "reason": "wrong_target",
  "message": "valid expression but 8 + 12 + 1 + 6 + 10 = 37, not 24. Keep going."
}
```

`reason`: `accepted` | `wrong_target` | `invalid_expression`. `value` is `null`
when the expression could not be evaluated at all. A rejected solution is a
**successful tool call** (`isError: false`) — the player was wrong, the tool
worked.

`HintPayload` — `hint`: `{ "level": 2, "next_level": 3, "hint": "Hint 2 — use '+' on 11 and 6." }`

`RevealPayload` — `reveal`:
`{ "count": 730, "shown": 5, "truncated": true, "solutions": ["..."], "message": "..." }`
(`shown` is capped at 5; `count` is the true total.)

`ExplainPayload` — `explain`:
`{ "expression": null, "steps": ["11 + 6 = 17", "..."], "message": "One way to reach 24 …" }`
`expression` echoes the player's own expression when one was supplied, in which
case `steps` is empty and `message` says whether it works.

## The board widget

| Property | Value |
|---|---|
| Resource URI | `ui://snap24/board.html` |
| MIME | `text/html;profile=mcp-app` |
| Contents `_meta.ui.csp` | `connectDomains: []`, `resourceDomains: []` (self-contained: no network, no external assets) |
| Contents `_meta.openai/ui.availableDisplayModes` | `["inline", "pip"]` |
| Contents `_meta.openai/widgetDescription` | one-line description for the model |
| Tool `_meta.openai/toolInvocation/*` | `invoking` / `invoked` status text |

The widget plays locally (taps, timer, undo, exact rational arithmetic) and
round-trips only `hint`, `reveal`, `start_puzzle` and the final
`submit_solution` for validation.

## Errors and sessions

- Unknown or expired `puzzle_id` → tool-level error (`isError: true`) with
  `unknown or expired puzzle_id "<id>"; call start_puzzle` — never a protocol
  error, so the model can recover.
- Puzzles live in server memory for **30 minutes** and are pruned on access.
  Ids do not survive a redeploy: after a deploy the model must deal again.
- The endpoint is unauthenticated and shared; ids are random, and a puzzle is
  refused when it has expired.

## Known deliberate choices

- Optional primitives (`target`, `seed`, `explain.expression`,
  `BoardPayload.view_seconds`) are nullable via `type: ["integer", "null"]`.
  That is legal JSON Schema and OpenAI's documented idiom for optional fields;
  a strict OpenAPI-dialect host (e.g. Gemini function declarations) prefers
  `anyOf: [{type: …}, {type: "null"}]`. Splitting them is a per-field
  `schema_with` change if we ever target such a host.
- `hint.level` uses `0` = "advance" rather than a nullable integer, so the
  schema needs no null branch.
- `mode`/`difficulty` are closed enums rather than free strings, so an invalid
  value is rejected by schema validation instead of silently defaulting.
