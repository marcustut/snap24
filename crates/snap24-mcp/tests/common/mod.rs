//! Shared harness: spawn the real server binary and speak MCP (JSON-RPC 2.0 over
//! stdio, newline-delimited) to it, exactly how a host would.
//!
//! Used by `stdio.rs` (behaviour) and `surface.rs` (frozen contract).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

pub struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Server {
    pub fn start() -> Self {
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

    pub fn send(&mut self, message: Value) {
        writeln!(self.stdin, "{message}").expect("write");
        self.stdin.flush().expect("flush");
    }

    /// Send a request and read until its response (skipping notifications).
    pub fn request(&mut self, method: &str, params: Value) -> Value {
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

    pub fn initialize(&mut self) {
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
    pub fn call_result(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
    }

    /// Call a tool and return its first text content block.
    pub fn call(&mut self, tool: &str, arguments: Value) -> String {
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
