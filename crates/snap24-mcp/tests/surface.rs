//! The published surface, locked.
//!
//! ChatGPT/plugin review means a tool rename, a param change or a dropped field
//! is a *release*, not a refactor. This test fails the moment the surface drifts
//! from `docs/mcp-surface.md`, so accidental changes are caught at commit time
//! instead of in review.

mod common;

use common::Server;
use serde_json::{json, Value};

fn tool<'a>(tools: &'a Value, name: &str) -> &'a Value {
    tools["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("tool {name} is missing"))
}

fn schema(t: &Value) -> &Value {
    t.get("inputSchema").expect("inputSchema")
}

fn param<'a>(t: &'a Value, name: &str) -> &'a Value {
    schema(t)
        .get("properties")
        .and_then(|p| p.get(name))
        .unwrap_or_else(|| panic!("param {name} on {}", t["name"]))
}

fn required(t: &Value) -> Vec<String> {
    schema(t)["required"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Follow a `$ref` into `$defs` (schemars hoists named enums out of the inline schema).
fn deref<'a>(t: &'a Value, node: &'a Value) -> &'a Value {
    match node.get("$ref").and_then(Value::as_str).and_then(|r| r.rsplit('/').next()) {
        Some(name) => &schema(t)["$defs"][name],
        None => node,
    }
}

/// True when the field is `ty`, or a nullable union containing it (optional
/// primitives are `["integer", "null"]`, which is OpenAI's documented idiom).
fn is_type(node: &Value, ty: &str) -> bool {
    match &node["type"] {
        Value::String(s) => s == ty,
        Value::Array(a) => a.iter().any(|v| v == ty),
        _ => false,
    }
}

fn enums(t: &Value, name: &str) -> Vec<String> {
    deref(t, param(t, name))["enum"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

#[test]
fn the_surface_matches_the_published_contract() {
    let mut server = Server::start();
    server.initialize();

    // --- identity -----------------------------------------------------------
    let info = server.request(
        "initialize",
        json!({
            "protocolVersion": "2026-07-28",
            "capabilities": {},
            "clientInfo": {"name": "surface-test", "version": "0"}
        }),
    );
    assert_eq!(info["serverInfo"]["name"], "snap24", "server name is user-visible: {info}");
    assert_eq!(
        info["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION"),
        "version must track the crate: {info}"
    );
    assert!(
        info["instructions"].as_str().unwrap_or_default().contains("render_board"),
        "instructions must tell the model how to open the board: {info}"
    );

    // --- the tool set is exactly this ---------------------------------------
    let tools = server.request("tools/list", json!({}));
    let mut names: Vec<String> = tools["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
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
        ],
        "renaming or adding a tool changes the published surface"
    );

    // Every tool declares an output schema (its structured result is the contract).
    for name in &names {
        let t = tool(&tools, name);
        assert!(
            t.get("outputSchema").is_some(),
            "{name} must declare outputSchema"
        );
        assert!(
            t["description"].as_str().is_some_and(|d| d.len() > 20),
            "{name} needs a real model-facing description"
        );
        assert_eq!(
            schema(t)["additionalProperties"], false,
            "{name} must reject unknown arguments"
        );
    }

    // --- each tool's parameters --------------------------------------------
    let start = tool(&tools, "start_puzzle");
    assert!(required(start).is_empty(), "start_puzzle takes no required args");
    assert_eq!(enums(start, "mode"), vec!["classic", "custom"], "mode is a closed enum");
    assert_eq!(
        enums(start, "difficulty"),
        vec!["easy", "medium", "hard", "expert", "insane", "blind"],
        "difficulty is a closed enum"
    );
    assert!(is_type(param(start, "target"), "integer"), "target: {start}");
    assert!(is_type(param(start, "seed"), "integer"), "seed: {start}");

    let submit = tool(&tools, "submit_solution");
    assert_eq!(required(submit), vec!["puzzle_id", "expression"]);
    assert!(is_type(param(submit, "expression"), "string"), "expression: {submit}");

    for name in ["hint", "reveal", "explain", "render_board"] {
        let t = tool(&tools, name);
        assert_eq!(required(t), vec!["puzzle_id"], "{name} needs only puzzle_id");
    }
    assert_eq!(param(tool(&tools, "hint"), "level")["format"], "uint32");

    // --- annotations --------------------------------------------------------
    for name in &names {
        let t = tool(&tools, name);
        assert_eq!(t["annotations"]["destructiveHint"], false, "{name}");
        assert_eq!(t["annotations"]["openWorldHint"], false, "{name}");
        assert!(t["annotations"]["title"].as_str().is_some(), "{name} needs a title");
        let expected_read_only = name != "start_puzzle";
        assert_eq!(
            t["annotations"]["readOnlyHint"], expected_read_only,
            "{name} readOnlyHint"
        );
    }

    // --- the widget ---------------------------------------------------------
    for name in ["start_puzzle", "render_board"] {
        let t = tool(&tools, name);
        assert_eq!(
            t["_meta"]["ui"]["resourceUri"], "ui://snap24/board.html",
            "{name} must advertise the board"
        );
        assert!(
            t["_meta"]["openai/toolInvocation/invoking"].is_string(),
            "{name} needs invocation status text"
        );
    }

    let resources = server.request("resources/list", json!({}));
    let listed = resources["resources"]
        .as_array()
        .expect("resources")
        .iter()
        .find(|r| r["uri"] == "ui://snap24/board.html")
        .expect("the board resource must be listed");
    assert_eq!(listed["mimeType"], "text/html;profile=mcp-app");

    let read = server.request("resources/read", json!({"uri": "ui://snap24/board.html"}));
    let meta = &read["contents"][0]["_meta"];
    assert!(meta["ui"]["csp"]["connectDomains"].is_array(), "CSP: {meta}");
    assert!(meta["ui"]["csp"]["resourceDomains"].is_array(), "CSP: {meta}");
    let modes: Vec<String> = meta["openai/ui"]["availableDisplayModes"]
        .as_array()
        .expect("display modes")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    assert_eq!(modes, vec!["inline", "pip"], "display modes: {meta}");
    assert!(
        meta["openai/widgetDescription"].is_string(),
        "the model needs to know what the widget shows: {meta}"
    );

    // --- structured results, one shape per tool -----------------------------
    let dealt = server.call_result("start_puzzle", json!({"mode": "classic", "seed": 7}));
    let board = &dealt["structuredContent"];
    assert_eq!(board["mode"], "classic");
    assert_eq!(board["target"]["n"], 24);
    assert!(board["cards"].as_array().is_some_and(|c| c.len() == 5));
    assert!(board["view_seconds"].is_null() || board["view_seconds"].is_u64());
    assert!(board["puzzle_id"].as_str().is_some());
    let id = board["puzzle_id"].as_str().unwrap().to_string();

    let verdict = server.call_result(
        "submit_solution",
        json!({"puzzle_id": id, "expression": "1 + 1"}),
    );
    let verdict = &verdict["structuredContent"];
    for key in ["accepted", "value", "reason", "message"] {
        assert!(verdict.get(key).is_some(), "SolveVerdict needs {key}: {verdict}");
    }
    assert_eq!(verdict["accepted"], false);
    assert_eq!(
        verdict["reason"], "invalid_expression",
        "reason is a closed enum: {verdict}"
    );

    let hint = server.call_result("hint", json!({"puzzle_id": id}));
    for key in ["level", "next_level", "hint"] {
        assert!(hint["structuredContent"].get(key).is_some(), "HintPayload needs {key}");
    }

    let reveal = server.call_result("reveal", json!({"puzzle_id": id}));
    for key in ["count", "shown", "truncated", "solutions", "message"] {
        assert!(reveal["structuredContent"].get(key).is_some(), "RevealPayload needs {key}");
    }

    let explain = server.call_result("explain", json!({"puzzle_id": id}));
    for key in ["expression", "steps", "message"] {
        assert!(explain["structuredContent"].get(key).is_some(), "ExplainPayload needs {key}");
    }

    // Unknown ids stay a friendly tool error rather than a protocol error.
    let missing = server.call("reveal", json!({"puzzle_id": "nope"}));
    assert!(missing.contains("unknown or expired"), "missing: {missing}");
}
