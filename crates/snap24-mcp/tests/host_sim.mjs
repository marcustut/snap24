// Local MCP Apps host simulator.
//
// Acts like the ChatGPT host: spawns the real snap24-mcp server over stdio,
// serves the widget in an iframe, bridges `tools/call` back to the server, and
// drives a full round in a headless browser. This is the closest we can get to
// the ChatGPT dev-mode test without an OpenAI account.
//
//   cargo build -p snap24-mcp
//   npm i playwright && npx playwright install chromium   # once
//   node crates/snap24-mcp/tests/host_sim.mjs
//
// Set PLAYWRIGHT=/path/to/playwright/index.mjs if it isn't resolvable locally.

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const { chromium } = await import(process.env.PLAYWRIGHT ?? "playwright");

const HERE = dirname(fileURLToPath(import.meta.url));
const SERVER = process.env.SNAP24_MCP ?? join(HERE, "..", "..", "..", "target", "debug", "snap24-mcp");

const checks = [];
const check = (name, ok, detail = "") => {
  checks.push({ name, ok });
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok || !detail ? "" : ` — ${detail}`}`);
};

// ---- MCP client over stdio -------------------------------------------------
function startServer() {
  const child = spawn(SERVER, [], { stdio: ["pipe", "pipe", "inherit"] });
  let id = 0;
  let buffer = "";
  const pending = new Map();
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    buffer += chunk;
    let nl;
    while ((nl = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, nl).trim();
      buffer = buffer.slice(nl + 1);
      if (!line) continue;
      let message;
      try { message = JSON.parse(line); } catch { continue; }
      const p = message.id !== undefined && pending.get(message.id);
      if (p) {
        pending.delete(message.id);
        message.error ? p.reject(new Error(message.error.message)) : p.resolve(message.result);
      }
    }
  });
  const send = (m) => child.stdin.write(JSON.stringify(m) + "\n");
  const request = (method, params) =>
    new Promise((resolve, reject) => {
      const i = ++id;
      pending.set(i, { resolve, reject });
      send({ jsonrpc: "2.0", id: i, method, params });
    });
  const tool = async (name, args) => {
    const result = await request("tools/call", { name, arguments: args });
    const text = (result.content ?? []).filter((c) => c.type === "text").map((c) => c.text).join("\n");
    return { result, text };
  };
  return { child, send, request, tool };
}

const mcp = startServer();
await mcp.request("initialize", {
  protocolVersion: "2026-07-28",
  capabilities: {},
  clientInfo: { name: "host-sim", version: "0" },
});
mcp.send({ jsonrpc: "2.0", method: "notifications/initialized" });

const dealt = await mcp.tool("start_puzzle", { mode: "classic", difficulty: "medium", seed: 5 });
const puzzleId = /puzzle_id:\s*(\S+)/.exec(dealt.text)?.[1];
check("start_puzzle returns a puzzle_id", Boolean(puzzleId), dealt.text);
const view = (await mcp.tool("render_board", { puzzle_id: puzzleId })).result.structuredContent;
check("render_board returns structured content", Boolean(view?.cards?.length), JSON.stringify(view));

// ---- serve the widget + a host page that bridges to the server -------------
const boardHtml = await readFile(join(HERE, "..", "ui", "board.html"), "utf8");
const hostHtml = `<!doctype html><meta charset="utf-8">
<body style="margin:0"><iframe id="w" src="/board.html" style="width:430px;height:780px;border:0"></iframe>
<script>
  // Host side of the MCP Apps bridge: relay tools/call to the MCP server.
  window.addEventListener("message", async (event) => {
    const m = event.data;
    if (!m || m.jsonrpc !== "2.0" || m.id === undefined) return;
    try {
      const result = await window.mcpCall(m.method === "tools/call" ? m.params.name : "", m.params?.arguments ?? {});
      event.source.postMessage({ jsonrpc: "2.0", id: m.id, result }, "*");
    } catch (error) {
      event.source.postMessage({ jsonrpc: "2.0", id: m.id, error: { message: String(error) } }, "*");
    }
  });
</script>`;
const http = createServer((req, res) => {
  res.setHeader("content-type", "text/html; charset=utf-8");
  res.end(req.url?.startsWith("/board.html") ? boardHtml : hostHtml);
});
await new Promise((r) => http.listen(0, r));
const port = http.address().port;

const browser = await chromium.launch();
const context = await browser.newContext();
const calls = [];
await context.exposeBinding("mcpCall", async (_source, name, args) => {
  calls.push(name);
  if (!name) return {};
  const result = await mcp.tool(name, args);
  return result.result; // full CallToolResult: content + structuredContent
});
await context.addInitScript(() => {
  window.__displayMode = null;
  window.__closed = false;
  window.openai = {
    displayMode: "inline",
    requestDisplayMode: async ({ mode }) => {
      window.__displayMode = mode;
      window.openai.displayMode = mode;
      // A real host mirrors the change back through the globals event.
      window.dispatchEvent(new CustomEvent("openai:set_globals", { detail: { globals: { displayMode: mode } } }));
      return { mode };
    },
    requestClose: async () => {
      window.__closed = true;
    },
  };
});
const page = await context.newPage();
await page.goto(`http://localhost:${port}/`);

// The host delivers the initial tool result (what ChatGPT does after a tool call).
await page.waitForSelector("#w");
await page.waitForTimeout(600); // let the iframe boot its bridge
await page.evaluate((v) => {
  const w = document.getElementById("w").contentWindow;
  w.postMessage({ jsonrpc: "2.0", method: "ui/notifications/tool-result", params: { structuredContent: v } }, "*");
}, view);

const widget = page.frameLocator("#w");
const frame = () => page.frames().find((f) => f.url().includes("board.html"));
const text = (sel) => widget.locator(sel).innerText();

// 1. Renders from the tool result.
await widget.locator(".card").first().waitFor({ timeout: 5000 });
check("board renders the target from the tool result", (await text("#target")) === "24");

// 2. Timer is running (Medium) and the hide mechanic exists.
check("countdown is visible", /hiding in/i.test(await text("#timer")), await text("#timer"));

// 2b. Every view mode: Blind hides from the start, Easy never hides, Medium counts down.
// (Easy reports view_seconds: null, which a naive Number() coercion reads as 0 = hidden.)
async function showRound(difficulty) {
  const round = await mcp.tool("start_puzzle", { mode: "classic", difficulty, seed: 9 });
  const id = /puzzle_id:\s*(\S+)/.exec(round.text)?.[1];
  const payload = (await mcp.tool("render_board", { puzzle_id: id })).result.structuredContent;
  await page.evaluate((v) => {
    const w = document.getElementById("w").contentWindow;
    w.postMessage({ jsonrpc: "2.0", method: "ui/notifications/tool-result", params: { structuredContent: v } }, "*");
  }, payload);
  await widget.locator(".card").first().waitFor();
  await page.waitForTimeout(200);
  return payload;
}

const blind = await showRound("blind");
check("blind: view_seconds is 0", blind.view_seconds === 0, JSON.stringify(blind.view_seconds));
const blindRanks = await widget.locator(".card .rank").allInnerTexts();
check("blind: every card is face down", blindRanks.length > 0 && blindRanks.every((r) => r === "?"), blindRanks.join(" "));
check("blind: no countdown", (await text("#timer")) === "", await text("#timer"));

const easy = await showRound("easy");
check("easy: view_seconds is null", easy.view_seconds === null, JSON.stringify(easy.view_seconds));
const easyRanks = await widget.locator(".card .rank").allInnerTexts();
check("easy: cards are face up", easyRanks.length > 0 && easyRanks.every((r) => r !== "?"), easyRanks.join(" "));
check("easy: no countdown", (await text("#timer")) === "", await text("#timer"));

// Back to a Medium hand for the rest of the run.
await showRound("medium");
await page.waitForTimeout(200);

// 3. Taps merge locally: two cards + an operator shrink the hand.
const before = await widget.locator(".card").count();
await widget.locator(".card").nth(0).click();
await widget.locator(".key").nth(0).click(); // +
await widget.locator(".card").nth(1).click();
await widget.locator(".card").first().waitFor();
check("tap, operator, tap merges locally", (await widget.locator(".card").count()) === before - 1);
check("merge did not call the server", !calls.includes("submit_solution"), calls.join(","));

// 4. Undo is local too.
await widget.locator("#undo").click();
check("undo restores the hand", (await widget.locator(".card").count()) === before);

// 5. Hints/reveal round-trip to the server.
await widget.locator("#hint").click();
await page.waitForFunction(() => document.querySelector("#w").contentWindow.document.querySelector("#msg").textContent.length > 0);
check("hint round-trips", calls.includes("hint"), calls.join(","));
await widget.locator("#reveal").click();
await page.waitForTimeout(300);
check("reveal round-trips", calls.includes("reveal"), calls.join(","));

// 6. Play the round out (any merges) — the final result is validated server-side.
for (let guard = 0; guard < 12; guard += 1) {
  const cards = await widget.locator(".card").count();
  if (cards < 2) break;
  await widget.locator(".card").nth(0).click();
  await widget.locator(".key").nth(0).click(); // +
  await widget.locator(".card").nth(1).click();
  await page.waitForTimeout(80);
}
check("round reached a single card", (await widget.locator(".card").count()) === 1);
await page.waitForTimeout(400);
check("final result is validated by the server", calls.includes("submit_solution"), calls.join(","));
check("outcome is shown", /solved|not 24|valid|accepted/i.test(await text("#msg")), await text("#msg"));

// 7. Picture-in-picture + clean close.
await widget.locator("#pip").click();
await page.waitForTimeout(200);
check("widget requests PiP", (await frame().evaluate(() => window.__displayMode)) === "pip");
await widget.locator("#close").click();
check("widget can close cleanly", (await frame().evaluate(() => window.__closed)) === true);

await browser.close();
http.close();
mcp.child.kill();

const failed = checks.filter((c) => !c.ok);
console.log(`\n${checks.length - failed.length}/${checks.length} checks passed`);
process.exit(failed.length === 0 ? 0 : 1);
