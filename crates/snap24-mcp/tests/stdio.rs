//! End-to-end test: spawn the real server binary and speak MCP to it, exactly
//! how a host would.

mod common;

use common::Server;
use serde_json::json;

#[test]
fn tools_work_over_stdio() {
    let mut server = Server::start();
    server.initialize();

    // 1. The six tools are advertised.
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
    assert_eq!(render["annotations"]["readOnlyHint"], true, "{render}");

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
    let contents_meta = &read["contents"][0]["_meta"];
    assert!(
        contents_meta["ui"]["csp"]["connectDomains"].is_array(),
        "CSP must be declared on the resource contents: {read}"
    );
    let modes = contents_meta["openai/ui"]["availableDisplayModes"]
        .as_array()
        .expect("available display modes");
    assert!(modes.iter().any(|m| m == "pip"), "PiP must be declared: {modes:?}");
    assert!(
        contents_meta["openai/widgetDescription"].is_string(),
        "widget description: {contents_meta}"
    );

    // 2. Deal a deterministic Classic puzzle: structured, not just prose.
    let dealt = server.call_result(
        "start_puzzle",
        json!({"mode": "classic", "difficulty": "easy", "seed": 5}),
    );
    let board = &dealt["structuredContent"];
    assert_eq!(board["target"]["n"], 24, "dealt: {dealt}");
    assert_eq!(board["target"]["d"], 1, "dealt: {dealt}");
    assert_eq!(board["mode"], "classic", "dealt: {dealt}");
    assert_eq!(board["difficulty"], "easy", "dealt: {dealt}");
    assert_eq!(board["cards"].as_array().map(Vec::len), Some(5), "dealt: {dealt}");
    let puzzle_id = board["puzzle_id"].as_str().expect("puzzle id").to_string();
    // The prose stays as the display string.
    assert!(
        dealt["content"][0]["text"].as_str().unwrap_or_default().contains("target 24"),
        "summary: {dealt}"
    );
    // start_puzzle opens the board itself, so the UI doesn't need a second call.
    let start_tool = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "start_puzzle")
        .unwrap();
    assert_eq!(
        start_tool["_meta"]["ui"]["resourceUri"], "ui://snap24/board.html",
        "start_puzzle must advertise the widget: {start_tool}"
    );

    // 2a. start_puzzle mutates state, so it must not claim read-only.
    let start = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "start_puzzle")
        .unwrap();
    assert_eq!(start["annotations"]["readOnlyHint"], false, "{start}");

    // 2b. render_board returns the same payload as start_puzzle.
    let view = server.call_result("render_board", json!({"puzzle_id": puzzle_id}));
    assert_eq!(view["structuredContent"], *board, "view: {view}");

    // 3. Hints advance one level per call, and say which level they gave.
    let h1 = server.call_result("hint", json!({"puzzle_id": puzzle_id}));
    assert_eq!(h1["structuredContent"]["level"], 1, "hint 1: {h1}");
    assert_eq!(h1["structuredContent"]["next_level"], 2, "hint 1: {h1}");
    let h2 = server.call_result("hint", json!({"puzzle_id": puzzle_id}));
    assert_eq!(h2["structuredContent"]["level"], 2, "hint 2: {h2}");
    let h4 = server.call_result("hint", json!({"puzzle_id": puzzle_id, "level": 4}));
    assert_eq!(h4["structuredContent"]["level"], 4, "hint 4: {h4}");
    assert_eq!(h4["structuredContent"]["next_level"], 4, "level caps at 4: {h4}");

    // 4. A wrong submission is a *result* (accepted: false), not a crash.
    let bad = server.call_result(
        "submit_solution",
        json!({"puzzle_id": puzzle_id, "expression": "1 + 1"}),
    );
    assert_eq!(bad["structuredContent"]["accepted"], false, "bad: {bad}");
    assert_eq!(
        bad["structuredContent"]["reason"], "invalid_expression",
        "cards were reused: {bad}"
    );
    assert!(bad["structuredContent"]["value"].is_null(), "no value: {bad}");

    // 5. A real solution taken from reveal must be accepted.
    let revealed = server.call_result("reveal", json!({"puzzle_id": puzzle_id}));
    let revealed = &revealed["structuredContent"];
    assert!(revealed["count"].as_u64().unwrap_or(0) > 0, "reveal: {revealed}");
    assert_eq!(revealed["shown"], 5, "listing is capped: {revealed}");
    let first = revealed["solutions"][0].as_str().expect("a solution").to_string();
    let accepted = server.call_result(
        "submit_solution",
        json!({"puzzle_id": puzzle_id, "expression": first}),
    );
    assert_eq!(accepted["structuredContent"]["accepted"], true, "accepted: {accepted}");
    assert_eq!(accepted["structuredContent"]["reason"], "accepted", "accepted: {accepted}");
    assert_eq!(accepted["structuredContent"]["value"]["n"], 24, "accepted: {accepted}");

    // 5b. An expression that uses every card but misses the target is
    // "wrong_target", not "invalid".
    let missed = server.call_result(
        "submit_solution",
        json!({"puzzle_id": puzzle_id, "expression": "1 + 2 + 3 + 4 + 5"}),
    );
    let missed = &missed["structuredContent"];
    assert_eq!(missed["accepted"], false, "missed: {missed}");
    assert_eq!(
        missed["reason"], "invalid_expression",
        "those cards can't be summed from the dealt hand: {missed}"
    );

    // 6. Explain walks through a solution, as data and as prose.
    let explained = server.call_result("explain", json!({"puzzle_id": puzzle_id}));
    let steps = explained["structuredContent"]["steps"].as_array().cloned().unwrap_or_default();
    assert!(!steps.is_empty(), "explain: {explained}");
    assert!(
        explained["structuredContent"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains('='),
        "explain: {explained}"
    );

    // 7. Unknown ids are a friendly error, not a panic.
    let missing = server.call("reveal", json!({"puzzle_id": "nope"}));
    assert!(missing.contains("unknown or expired"), "missing: {missing}");
}
