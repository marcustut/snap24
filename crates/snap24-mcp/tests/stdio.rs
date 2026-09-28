//! End-to-end test: spawn the real server binary and speak MCP (JSON-RPC 2.0
//! over stdio, newline-delimited) to it, exactly how a host would.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Server {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_snap24-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn server");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Server { child, stdin, stdout, next_id: 0 }
    }

    fn send(&mut self, message: Value) {
        writeln!(self.stdin, "{message}").expect("write");
        self.stdin.flush().expect("flush");
    }

    /// Send a request and read until its response (skipping notifications).
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read");
            assert!(read > 0, "server closed the stream during {method}");
            let value: Value = serde_json::from_str(&line).expect("valid json-rpc");
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = value.get("error") {
                    panic!("{method} returned an error: {error}");
                }
                return value["result"].clone();
            }
            // otherwise it's a notification (e.g. logging) — keep reading
        }
    }

    fn initialize(&mut self) {
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion": "2026-07-28",
                "capabilities": {},
                "clientInfo": {"name": "snap24-test", "version": "0"}
            }),
        );
        assert!(result.get("serverInfo").is_some(), "initialize: {result}");
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    }

    /// Call a tool and return the whole result (for structured content).
    fn call_result(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
    }

    /// Call a tool and return its first text content block.
    fn call(&mut self, tool: &str, arguments: Value) -> String {
        let result = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        result["content"]
            .as_array()
            .and_then(|blocks| {
                blocks.iter().find_map(|b| {
                    (b["type"] == "text").then(|| b["text"].as_str().unwrap_or_default().to_string())
                })
            })
            .unwrap_or_default()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn tools_work_over_stdio() {
    let mut server = Server::start();
    server.initialize();

    // 1. The five tools are advertised.
    let tools = server.request("tools/list", json!({}));
    let mut names: Vec<String> = tools["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "explain",
            "hint",
            "render_board",
            "reveal",
            "start_puzzle",
            "submit_solution"
        ]
    );

    // 1b. render_board advertises the MCP Apps widget via _meta.ui.resourceUri.
    let render = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "render_board")
        .expect("render_board tool");
    assert_eq!(render["_meta"]["ui"]["resourceUri"], "ui://snap24/board.html");

    // 1c. The widget resource is listed with the MCP Apps MIME type and reads back.
    let resources = server.request("resources/list", json!({}));
    assert!(
        resources["resources"].as_array().unwrap().iter().any(|r| {
            r["uri"] == "ui://snap24/board.html"
                && r["mimeType"] == "text/html;profile=mcp-app"
        }),
        "resources: {resources}"
    );
    let read = server.request(
        "resources/read",
        json!({"uri": "ui://snap24/board.html"}),
    );
    let html = read["contents"][0]["text"].as_str().unwrap_or_default();
    assert!(html.contains("ui/notifications/tool-result"), "bridge missing");
    assert!(html.contains("tools/call"), "bridge missing");
    assert!(
        read["contents"][0]["_meta"]["ui"]["csp"]["connectDomains"].is_array(),
        "CSP must be declared on the resource contents: {read}"
    );

    // 2. Deal a deterministic Classic puzzle.
    let dealt = server.call("start_puzzle", json!({"mode": "classic", "difficulty": "easy", "seed": 5}));
    assert!(dealt.contains("target 24"), "dealt: {dealt}");
    let puzzle_id = dealt
        .lines()
        .find_map(|l| l.strip_prefix("puzzle_id: "))
        .expect("puzzle id")
        .trim()
        .to_string();

    // 2b. render_board returns structured content for the widget.
    let view = server.call_result("render_board", json!({"puzzle_id": puzzle_id}));
    assert_eq!(view["structuredContent"]["target_n"], 24, "view: {view}");
    assert_eq!(view["structuredContent"]["puzzle_id"], puzzle_id);
    assert!(view["structuredContent"]["view_seconds"].is_null(), "Easy is unlimited: {view}");

    // 3. Hints advance one level per call, and an explicit level works.
    let h1 = server.call("hint", json!({"puzzle_id": puzzle_id}));
    assert!(h1.starts_with("Hint 1"), "hint 1: {h1}");
    let h2 = server.call("hint", json!({"puzzle_id": puzzle_id}));
    assert!(h2.starts_with("Hint 2"), "hint 2: {h2}");
    let h4 = server.call("hint", json!({"puzzle_id": puzzle_id, "level": 4}));
    assert!(h4.contains("full solution"), "hint 4: {h4}");

    // 4. A wrong submission is rejected, not crashed.
    let bad = server.call("submit_solution", json!({"puzzle_id": puzzle_id, "expression": "1 + 1"}));
    assert!(bad.starts_with("rejected"), "bad: {bad}");

    // 5. A real solution taken from reveal must be accepted.
    let revealed = server.call("reveal", json!({"puzzle_id": puzzle_id}));
    let first = revealed
        .split(": ")
        .nth(1)
        .expect("solution list")
        .split(" ; ")
        .next()
        .expect("first solution")
        .trim()
        .to_string();
    let accepted = server.call(
        "submit_solution",
        json!({"puzzle_id": puzzle_id, "expression": first}),
    );
    assert!(accepted.starts_with("accepted"), "accepted: {accepted}");

    // 6. Explain walks through a solution.
    let explained = server.call("explain", json!({"puzzle_id": puzzle_id}));
    assert!(explained.contains('='), "explain: {explained}");

    // 7. Unknown ids are a friendly error, not a panic.
    let missing = server.call("reveal", json!({"puzzle_id": "nope"}));
    assert!(missing.contains("unknown or expired"), "missing: {missing}");
}
