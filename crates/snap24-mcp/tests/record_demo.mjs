// Records the reviewer demo walkthrough for the ChatGPT plugin submission.
//
// Hosts the widget exactly like an MCP Apps host does, but bridges `tools/call`
// to the *deployed* server, drives the flow from the submission's positive test
// cases, and captures video + listing screenshots.
//
//   node crates/snap24-mcp/tests/record_demo.mjs
//
// Env: SNAP24_MCP_URL (default https://snap24.marcustut.me/mcp),
//      SNAP24_DEMO_DIR (default /tmp/snap24-demo),
//      SNAP24_SHOTS_DIR (screenshots for the plugin listing),
//      PLAYWRIGHT=/path/to/playwright/index.mjs if it isn't resolvable.

import { createServer } from "node:http";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

const { chromium } = await import(process.env.PLAYWRIGHT ?? "playwright");

const ENDPOINT = process.env.SNAP24_MCP_URL ?? "https://snap24.marcustut.me/mcp";
const OUT = process.env.SNAP24_DEMO_DIR ?? "/tmp/snap24-demo";
const SHOTS = process.env.SNAP24_SHOTS_DIR ?? "";
const VIDEO_SIZE = { width: 720, height: 980 };

// ---- MCP over streamable HTTP (to the live server) -------------------------
let session = null;
let rpcId = 0;

async function rpc(method, params) {
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      accept: "application/json, text/event-stream",
      ...(session ? { "mcp-session-id": session } : {}),
    },
    body: JSON.stringify({ jsonrpc: "2.0", id: String(++rpcId), method, params }),
  });
  session ||= res.headers.get("mcp-session-id");
  const body = await res.text();
  for (const line of body.split("\n")) {
    if (line.startsWith("data: ") && line.trim() !== "data:") {
      const message = JSON.parse(line.slice(6));
      if (message.result) return message.result;
    }
  }
  throw new Error(`no result for ${method}`);
}

const callTool = (name, args) => rpc("tools/call", { name, arguments: args });

await rpc("initialize", {
  protocolVersion: "2026-07-28",
  capabilities: {},
  clientInfo: { name: "snap24-demo-recorder", version: "1" },
});
await fetch(ENDPOINT, {
  method: "POST",
  headers: { "content-type": "application/json", accept: "application/json, text/event-stream", "mcp-session-id": session },
  body: JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }),
});

// The widget comes from the deployed server, so the recording shows what a host
// actually serves.
const resource = await rpc("resources/read", { uri: "ui://snap24/board.html" });
const boardHtml = resource.contents[0].text;

const hostHtml = `<!doctype html><meta charset="utf-8">
<body style="margin:0;background:#0f0c0a;color:#f3ece0">
<div id="caption" style="box-sizing:border-box;width:${VIDEO_SIZE.width}px;height:80px;padding:16px 20px;border-bottom:1px solid #2c241c;font:600 13px/1.5 ui-sans-serif,system-ui;letter-spacing:.1em;text-transform:uppercase;color:#c9a24a">
  Snap 24 &middot; MCP Apps board
  <div id="step" style="margin-top:4px;font:400 13px/1.4 ui-sans-serif,system-ui;letter-spacing:normal;text-transform:none;color:#d8cebc"></div>
</div>
<iframe id="w" src="/board.html" style="width:${VIDEO_SIZE.width}px;height:900px;border:0;display:block"></iframe>
<script>
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

const server = createServer((req, res) => {
  res.setHeader("content-type", "text/html; charset=utf-8");
  res.end(req.url?.startsWith("/board.html") ? boardHtml : hostHtml);
});
await new Promise((r) => server.listen(0, r));
const port = server.address().port;

await mkdir(OUT, { recursive: true });
if (SHOTS) await mkdir(SHOTS, { recursive: true });

const browser = await chromium.launch();
const context = await browser.newContext({
  viewport: VIDEO_SIZE,
  recordVideo: { dir: OUT, size: VIDEO_SIZE },
  deviceScaleFactor: 2,
});
await context.exposeBinding("mcpCall", async (_source, name, args) =>
  name ? await callTool(name, args) : {},
);
await context.addInitScript(() => {
  window.openai = {
    displayMode: "inline",
    requestDisplayMode: async ({ mode }) => {
      window.openai.displayMode = mode;
      window.dispatchEvent(new CustomEvent("openai:set_globals", { detail: { globals: { displayMode: mode } } }));
      return { mode };
    },
    requestClose: async () => { window.__closed = true; },
  };
});

const page = await context.newPage();
await page.goto(`http://localhost:${port}/`);
const widget = page.frameLocator("#w");
const frame = () => page.frames().find((f) => f.url().includes("board.html"));

const settle = (ms = 700) => page.waitForTimeout(ms);
const caption = (text) => page.evaluate((t) => { document.getElementById("step").textContent = t; }, text);
const shot = async (name) => {
  if (SHOTS) await page.screenshot({ path: join(SHOTS, name) });
};

// ---------------------------------------------------------------- test case 1
// "Deal me a Snap 24 hand" — start_puzzle opens the board by itself.
const deal = await callTool("start_puzzle", { mode: "classic", difficulty: "easy" });
const board = deal.structuredContent;
console.log(`dealt: ${board.cards.join(" ")} → ${board.target.n}`);

await page.waitForSelector("#w");
await settle(600); // let the iframe boot its bridge
await page.evaluate((payload) => {
  const w = document.getElementById("w").contentWindow;
  w.postMessage({ jsonrpc: "2.0", method: "ui/notifications/tool-result", params: { structuredContent: payload } }, "*");
}, board);
await widget.locator(".card").first().waitFor({ timeout: 5000 });
await caption("start_puzzle with no arguments — dealing a Classic hand. The tool opens the board itself.");
await settle(3000); // hold on the dealt hand
await shot("screenshot-board.png");

// ------------------------------------------------------- the player merges
// The board plays locally: tap a card, an operator, another card. `hand` mirrors
// the widget's card list so the server's own solution can be replayed as taps.
const OP_KEY = { "+": 0, "-": 1, "\u2212": 1, "*": 2, "\u00d7": 2, "/": 3, "\u00f7": 3 };
const parseRational = (text) => {
  const [n, d = "1"] = text.split("/");
  return { n: Number(n), d: Number(d) };
};
const sameValue = (a, b) => a.n * b.d === b.n * a.d;
let hand = board.cards.map((n) => ({ n, d: 1 }));

async function applyStep(step, pace = 900) {
  const [lhs, result] = step.split(" = ");
  const [left, op, right] = lhs.trim().split(/\s+/);
  const i = hand.findIndex((c) => sameValue(c, parseRational(left)));
  const j = hand.findIndex((c, k) => k !== i && sameValue(c, parseRational(right)));
  if (i < 0 || j < 0) throw new Error(`cannot find ${left} and ${right} in ${JSON.stringify(hand)}`);

  await widget.locator(".card").nth(i).click();
  await settle(320);
  await widget.locator(".key").nth(OP_KEY[op]).click();
  await settle(320);
  await widget.locator(".card").nth(j).click();
  await settle(pace);

  const lo = Math.min(i, j);
  const hi = Math.max(i, j);
  hand.splice(hi, 1);
  hand.splice(lo, 1);
  hand.splice(lo, 0, parseRational(result));
}

// The server's solution drives the taps, so the recorded hand finishes on the
// target rather than on whatever the first two clicks happened to add up to.
const solution = await callTool("explain", { puzzle_id: board.puzzle_id });
const steps = solution.structuredContent.steps;
console.log(`solution: ${steps.join(" ; ")}`);

await caption("The board plays locally: tap a card, tap an operator, tap another card to merge them.");
await applyStep(steps[0], 1600);
console.log(`after a merge: ${await widget.locator(".card").count()} cards left`);

// ---------------------------------------------------------------- test case 3
// "Give me a hint" — progressive, without spoiling.
await caption("\u201cGive me a hint\u201d — one level at a time: which two cards, without spoiling the rest.");
await widget.locator("#hint").click();
await settle(3000);

// ---------------------------------------------------------------- test case 5
// "I'm stuck" — reveal, then the hand is finished with local taps and the final
// result validated by the server.
await caption("\u201cI'm stuck\u201d — reveal lists the distinct solutions and the true total.");
await widget.locator("#reveal").click();
await settle(3000);

await caption("Finishing the hand with local taps; only the final expression goes to the server.");
for (const step of steps.slice(1)) await applyStep(step, 900);
await settle(600);
await caption("submit_solution validates with exact arithmetic — fractions and negatives included.");
await settle(1400);
console.log(`verdict: ${await widget.locator("#msg").innerText()}`);
await shot("screenshot-solved.png");

// Pop the board out so it stays visible while chatting, then close it.
const hasPip = await widget.locator("#pip").count();
if (hasPip) {
  await caption("Pop out keeps the board visible while the conversation continues.");
  await widget.locator("#pip").click();
  await settle(2400);
  await widget.locator("#close").click();
  await settle(1600);
}

await context.close(); // flushes the video
await browser.close();
server.close();

const video = await page.video()?.path();
console.log(`video: ${video}`);
if (video) await writeFile(join(OUT, "path.txt"), video);
