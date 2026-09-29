//! Snap 24 MCP server.
//!
//! Exposes the game to an MCP host (ChatGPT, Codex, Claude, the MCP inspector)
//! over stdio, backed by `snap24-core`. Everything works in plain chat with no
//! UI: the model can deal a puzzle, validate a submission, give progressive
//! hints, reveal the solutions and explain one.
//!
//! Tools:
//!   start_puzzle    — deal a Classic or Custom puzzle
//!   submit_solution — validate a player's expression (server-side, exact)
//!   hint            — progressive hint (which cards → operator → result → full)
//!   reveal          — distinct solutions for the current puzzle
//!   explain         — a step-by-step solution in words
//!   render_board    — returns the MCP Apps widget (`ui://snap24/board.html`)
//!
//! Puzzles live in memory keyed by an opaque id with a 30-minute TTL.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ListResourcesResult, MetaObject,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    Resource, ResourceContents, ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::stdio;
use rmcp::{
    schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use snap24_core::{
    evaluate, first_move, generate, generate_targeted, move_sequence, solutions_infix, Difficulty,
    Mode, Puzzle, Rational, Rng,
};

const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// Cap how many distinct solutions we print; sets can run to dozens.
const REVEAL_LIMIT: usize = 5;

struct Session {
    puzzle: Puzzle,
    created: Instant,
    /// How many hints have been given, so `hint` can advance one level per use.
    hint_level: u32,
}

#[derive(Clone)]
struct Snap24 {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    seq: Arc<AtomicU64>,
}

// --------------------------------------------------------------------------- //
// MCP Apps UI                                                                  //
// --------------------------------------------------------------------------- //

/// The widget resource. Treat it as a cache key: bump `v1` on a breaking change.
const UI_URI: &str = "ui://snap24/board.html";
/// MCP Apps UI MIME type.
const UI_MIME: &str = "text/html;profile=mcp-app";
const BOARD_HTML: &str = include_str!("../ui/board.html");

/// `_meta` linking a tool to the widget (`_meta.ui.resourceUri`).
fn tool_meta() -> MetaObject {
    meta(serde_json::json!({
        "ui": { "resourceUri": UI_URI },
        "openai/toolInvocation/invoking": "Dealing…",
        "openai/toolInvocation/invoked": "Board ready"
    }))
}

/// `_meta` on the resource contents: CSP, display modes and a model-facing
/// description of what the widget shows.
fn resource_meta() -> MetaObject {
    meta(serde_json::json!({
        // The widget makes no network calls and loads no external assets (it
        // talks to the host over postMessage only), so both allowlists are empty.
        "ui": {
            "prefersBorder": true,
            "csp": { "connectDomains": [], "resourceDomains": [] }
        },
        // Inline by default; the player can pop the board into picture-in-picture
        // so it stays visible while the conversation continues.
        "openai/ui": { "availableDisplayModes": ["inline", "pip"] },
        "openai/widgetDescription": "Interactive Snap 24 board: tap two cards and an operator to merge them toward the target."
    }))
}

fn meta(value: serde_json::Value) -> MetaObject {
    MetaObject(value.as_object().expect("object").clone())
}

/// An exact rational on the wire. The game is fraction-exact, so `1/3` must not
/// become `0.333…`.
#[derive(Serialize, schemars::JsonSchema)]
struct ExactValue {
    n: i64,
    d: i64,
}

impl From<Rational> for ExactValue {
    fn from(value: Rational) -> Self {
        let (n, d) = value.parts();
        Self { n, d }
    }
}

/// The board, as machine-readable state. Returned by both `start_puzzle` and
/// `render_board`, so a host can deal and render from one payload.
#[derive(Serialize, schemars::JsonSchema)]
struct BoardPayload {
    /// Opaque handle; pass it to the other tools.
    puzzle_id: String,
    /// "classic" | "custom" — matches the `mode` input.
    mode: String,
    /// "easy" | "medium" | "hard" | "expert" | "insane" | "blind".
    difficulty: String,
    /// Dealt ranks: A=1, J=11, Q=12, K=13.
    cards: Vec<i64>,
    /// What the hand must reach.
    target: ExactValue,
    /// `null` = visible indefinitely, `0` = never shown (blind).
    view_seconds: Option<u32>,
}

/// Why an expression was not accepted.
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SolveReason {
    /// Every card used once and the result equals the target.
    Accepted,
    /// A valid expression over the right cards, but it lands elsewhere.
    WrongTarget,
    /// Cards reused, cards missing, unparseable, or division by zero.
    InvalidExpression,
}

/// Verdict for a submitted expression.
#[derive(Serialize, schemars::JsonSchema)]
struct SolveVerdict {
    /// True only when every card is used exactly once and the result equals the target.
    accepted: bool,
    /// The expression's exact value; `null` when it could not be evaluated.
    value: Option<ExactValue>,
    /// Always present, so the caller never has to infer "why not".
    reason: SolveReason,
    /// Human summary (safe to show the player).
    message: String,
}

/// One progressive hint.
#[derive(Serialize, schemars::JsonSchema)]
struct HintPayload {
    /// Level this hint is (1 = which two cards, 2 = operator, 3 = sub-result, 4 = full solution).
    level: u32,
    /// Level the next `hint` call will give, capped at 4.
    next_level: u32,
    hint: String,
}

/// The distinct-solution listing.
#[derive(Serialize, schemars::JsonSchema)]
struct RevealPayload {
    /// Total distinct canonical solutions.
    count: usize,
    /// How many are listed in `solutions`.
    shown: usize,
    /// True when `count > shown`.
    truncated: bool,
    solutions: Vec<String>,
    message: String,
}

/// A walkthrough of one solution.
#[derive(Serialize, schemars::JsonSchema)]
struct ExplainPayload {
    /// The expression being explained, when the player supplied one.
    expression: Option<String>,
    /// Ordered moves: first the merge steps, then the result.
    steps: Vec<String>,
    message: String,
}

// --------------------------------------------------------------------------- //
// tool parameters                                                             //
// --------------------------------------------------------------------------- //

/// Puzzle recipe. Enum, not a free string, so the contract is unambiguous.
#[derive(Debug, Clone, Copy, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ModeArg {
    #[default]
    Classic,
    Custom,
}

impl From<ModeArg> for Mode {
    fn from(value: ModeArg) -> Self {
        match value {
            ModeArg::Classic => Mode::Classic,
            ModeArg::Custom => Mode::Custom,
        }
    }
}

/// Difficulty tier.
#[derive(Debug, Clone, Copy, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum DifficultyArg {
    #[default]
    Easy,
    Medium,
    Hard,
    Expert,
    Insane,
    Blind,
}

impl From<DifficultyArg> for Difficulty {
    fn from(value: DifficultyArg) -> Self {
        match value {
            DifficultyArg::Easy => Difficulty::Easy,
            DifficultyArg::Medium => Difficulty::Medium,
            DifficultyArg::Hard => Difficulty::Hard,
            DifficultyArg::Expert => Difficulty::Expert,
            DifficultyArg::Insane => Difficulty::Insane,
            DifficultyArg::Blind => Difficulty::Blind,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct StartParams {
    /// Which puzzle recipe to deal: "classic" (target 24, five cards) or
    /// "custom" (card count from the ladder, target drawn from the hand).
    #[serde(default)]
    mode: ModeArg,
    /// Difficulty tier. Higher tiers hide the cards sooner.
    #[serde(default)]
    difficulty: DifficultyArg,
    /// Custom only: a specific target the dealt hand must be able to reach.
    target: Option<i64>,
    /// Reproducible deal.
    seed: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct IdParams {
    /// The `puzzle_id` returned by start_puzzle.
    puzzle_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SubmitParams {
    puzzle_id: String,
    /// A player's expression using every card exactly once, e.g. "(9 - 1) * 3".
    expression: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct HintParams {
    puzzle_id: String,
    /// 1 = which two cards, 2 = operator, 3 = result, 4 = full solution.
    /// 0 (the default) advances one level per call.
    #[serde(default)]
    level: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExplainParams {
    puzzle_id: String,
    /// Optional expression to explain; defaults to one solution of the puzzle.
    expression: Option<String>,
}

// --------------------------------------------------------------------------- //
// helpers                                                                     //
// --------------------------------------------------------------------------- //

fn rank_label(value: i64) -> String {
    match value {
        1 => "A".into(),
        11 => "J".into(),
        12 => "Q".into(),
        13 => "K".into(),
        v => v.to_string(),
    }
}

/// Machine-readable label (matches the `mode` input enum).
fn mode_key(mode: Mode) -> &'static str {
    match mode {
        Mode::Classic => "classic",
        Mode::Custom => "custom",
    }
}

/// Machine-readable label (matches the `difficulty` input enum).
fn difficulty_key(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Easy => "easy",
        Difficulty::Medium => "medium",
        Difficulty::Hard => "hard",
        Difficulty::Expert => "expert",
        Difficulty::Insane => "insane",
        Difficulty::Blind => "blind",
    }
}

/// A tool result that carries both the human summary and the machine-readable
/// payload. The prose is a display string; `structuredContent` is the contract.
fn ok<T: Serialize>(message: String, payload: &T) -> Result<CallToolResult, String> {
    let value = serde_json::to_value(payload).map_err(|e| e.to_string())?;
    let mut result = CallToolResult::structured(value);
    result.content = vec![ContentBlock::text(message)];
    Ok(result)
}

/// The board payload, shared by `start_puzzle` and `render_board`.
fn board_payload(id: &str, puzzle: &Puzzle) -> BoardPayload {
    BoardPayload {
        puzzle_id: id.to_string(),
        mode: mode_key(puzzle.mode).to_string(),
        difficulty: difficulty_key(puzzle.difficulty).to_string(),
        cards: puzzle.cards.clone(),
        target: puzzle.target.into(),
        view_seconds: puzzle.difficulty.view_seconds(),
    }
}

fn view_description(difficulty: Difficulty) -> String {
    match difficulty.view_seconds() {
        None => "unlimited view".into(),
        Some(0) => "never shown (blind)".into(),
        Some(s) => format!("{s}s view"),
    }
}

fn card_list(cards: &[i64]) -> String {
    cards.iter().map(|c| rank_label(*c)).collect::<Vec<_>>().join(" ")
}

fn move_line(step: &snap24_core::Move) -> String {
    format!("{} {} {} = {}", step.left, step.op, step.right, step.result)
}

// --------------------------------------------------------------------------- //
// server                                                                      //
// --------------------------------------------------------------------------- //

impl Snap24 {
    fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            seq: Arc::new(AtomicU64::new(1)),
        }
    }

    fn prune(&self, sessions: &mut HashMap<String, Session>) {
        let now = Instant::now();
        sessions.retain(|_, s| now.duration_since(s.created) < SESSION_TTL);
    }

    /// Run `f` against a live session, or return a friendly error string.
    fn with_session<T>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Session) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut sessions = self.sessions.lock().unwrap();
        self.prune(&mut sessions);
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| format!("unknown or expired puzzle_id {id:?}; call start_puzzle"))?;
        f(session)
    }

    fn count_hints(session: &mut Session, level: u32) -> u32 {
        session.hint_level = match level {
            0 => (session.hint_level + 1).min(4),
            l => l.clamp(1, 4),
        };
        session.hint_level
    }
}

#[tool_router]
impl Snap24 {
    #[tool(
        description = "Deal a Snap 24 puzzle. Returns a puzzle_id plus the dealt cards and target, and shows the interactive board.",
        meta = tool_meta(),
        output_schema = rmcp::handler::server::tool::schema_for_output::<BoardPayload>(),
        annotations(title = "Deal a puzzle", read_only_hint = false, destructive_hint = false, open_world_hint = false)
    )]
    fn start_puzzle(&self, Parameters(p): Parameters<StartParams>) -> Result<CallToolResult, String> {
        let mode: Mode = p.mode.into();
        let difficulty = Difficulty::from(p.difficulty);
        let seed = p.seed.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x5EED)
        });
        let mut rng = Rng::new(seed);

        let puzzle = match (mode, p.target) {
            (Mode::Custom, Some(target)) => {
                generate_targeted(mode, difficulty, Rational::from(target), &mut rng)
            }
            _ => generate(mode, difficulty, &mut rng),
        };

        let id = {
            let n = self.seq.fetch_add(1, Ordering::Relaxed);
            format!("p{seed:x}-{n}")
        };
        let cards = puzzle.cards.clone();
        let target = puzzle.target;
        let summary = format!(
            "{:<7} {:>2} cards  target {}\ncards:  {}\nview:   {}\npuzzle_id: {}",
            mode.label(),
            cards.len(),
            target,
            card_list(&cards),
            view_description(difficulty),
            id
        );

        let payload = board_payload(&id, &puzzle);
        self.sessions.lock().unwrap().insert(
            id,
            Session {
                puzzle,
                created: Instant::now(),
                hint_level: 0,
            },
        );
        ok(summary, &payload)
    }

    #[tool(
        description = "Validate a player's expression against the dealt cards and target (exact arithmetic, server-side).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<SolveVerdict>(),
        annotations(title = "Check a solution", read_only_hint = true, destructive_hint = false, open_world_hint = false)
    )]
    fn submit_solution(&self, Parameters(p): Parameters<SubmitParams>) -> Result<CallToolResult, String> {
        let id = p.puzzle_id.clone();
        self.with_session(&id, |session| {
            let cards = session.puzzle.cards.clone();
            let target = session.puzzle.target;
            let trimmed = p.expression.trim();
            let verdict = match evaluate(&cards, trimmed) {
                Ok(value) if value == target => SolveVerdict {
                    accepted: true,
                    value: Some(value.into()),
                    reason: SolveReason::Accepted,
                    message: format!("accepted: {trimmed} = {target} — solved."),
                },
                Ok(value) => SolveVerdict {
                    accepted: false,
                    value: Some(value.into()),
                    reason: SolveReason::WrongTarget,
                    message: format!("valid expression but {trimmed} = {value}, not {target}. Keep going."),
                },
                Err(err) => SolveVerdict {
                    accepted: false,
                    value: None,
                    reason: SolveReason::InvalidExpression,
                    message: format!("rejected: {err}"),
                },
            };
            ok(verdict.message.clone(), &verdict)
        })
    }

    #[tool(
        description = "Progressive hint: 1 = which two cards, 2 = the operator, 3 = the sub-result, 4 = the full solution. Advances one level per call unless a level is given.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<HintPayload>(),
        annotations(title = "Give a hint", read_only_hint = true, destructive_hint = false, open_world_hint = false)
    )]
    fn hint(&self, Parameters(p): Parameters<HintParams>) -> Result<CallToolResult, String> {
        let id = p.puzzle_id.clone();
        self.with_session(&id, |session| {
            let level = Self::count_hints(session, p.level);
            let board = session
                .puzzle
                .cards
                .iter()
                .copied()
                .map(Rational::from)
                .collect::<Vec<_>>();
            let target = session.puzzle.target;
            let hint = match first_move(&board, target) {
                None => "No solution from here — call start_puzzle for a fresh deal.".to_string(),
                Some(step) => match level {
                    1 => format!("Hint {level} — combine {} and {}.", step.left, step.right),
                    2 => format!("Hint {level} — use '{}' on {} and {}.", step.op, step.left, step.right),
                    3 => format!(
                        "Hint {level} — {} {} {} = {}",
                        step.left, step.op, step.right, step.result
                    ),
                    _ => {
                        let steps = move_sequence(&board, target);
                        format!(
                            "Hint {level} — full solution: {}",
                            steps.iter().map(move_line).collect::<Vec<_>>().join(" ; ")
                        )
                    }
                },
            };
            let payload = HintPayload {
                level,
                next_level: (level + 1).min(4),
                hint: hint.clone(),
            };
            ok(hint, &payload)
        })
    }

    #[tool(
        description = "List distinct solutions for the current puzzle (canonical, de-duplicated).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<RevealPayload>(),
        annotations(title = "Reveal solutions", read_only_hint = true, destructive_hint = false, open_world_hint = false)
    )]
    fn reveal(&self, Parameters(p): Parameters<IdParams>) -> Result<CallToolResult, String> {
        let id = p.puzzle_id.clone();
        self.with_session(&id, |session| {
            let board = session
                .puzzle
                .cards
                .iter()
                .copied()
                .map(Rational::from)
                .collect::<Vec<_>>();
            let target = session.puzzle.target;
            let all = solutions_infix(&board, target);
            let solutions: Vec<String> = all.iter().take(REVEAL_LIMIT).cloned().collect();
            let count = all.len();
            let shown = solutions.len();
            let more = count.saturating_sub(shown);
            let listed = solutions.join(" ; ");
            let message = if count == 0 {
                "No solutions — this hand can't reach the target.".to_string()
            } else if more > 0 {
                format!("{count} solutions (showing {shown}, +{more} more): {listed}")
            } else {
                format!("{count} solutions: {listed}")
            };
            let payload = RevealPayload {
                count,
                shown,
                truncated: more > 0,
                solutions,
                message: message.clone(),
            };
            ok(message, &payload)
        })
    }

    #[tool(
        description = "Explain a solution step by step in words (uses the player's expression if given, otherwise one solution).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<ExplainPayload>(),
        annotations(title = "Explain a solution", read_only_hint = true, destructive_hint = false, open_world_hint = false)
    )]
    fn explain(&self, Parameters(p): Parameters<ExplainParams>) -> Result<CallToolResult, String> {
        let id = p.puzzle_id.clone();
        self.with_session(&id, |session| {
            let cards = session.puzzle.cards.clone();
            let board = cards.iter().copied().map(Rational::from).collect::<Vec<_>>();
            let target = session.puzzle.target;

            if let Some(expr) = p.expression.as_deref() {
                let message = match evaluate(&cards, expr) {
                    Ok(value) if value == target => format!(
                        "'{}' uses every card once and equals {target}. ({} cards from {}.)",
                        expr.trim(),
                        cards.len(),
                        card_list(&cards)
                    ),
                    Ok(value) => format!("'{}' evaluates to {value}, not {target}.", expr.trim()),
                    Err(err) => format!("Can't explain '{}': {err}", expr.trim()),
                };
                let payload = ExplainPayload {
                    expression: Some(expr.trim().to_string()),
                    steps: Vec::new(),
                    message: message.clone(),
                };
                return ok(message, &payload);
            }

            let steps = move_sequence(&board, target);
            if steps.is_empty() {
                let payload = ExplainPayload {
                    expression: None,
                    steps: Vec::new(),
                    message: "No solution to explain for this hand.".to_string(),
                };
                return ok(payload.message.clone(), &payload);
            }
            let lines = steps.iter().map(move_line).collect::<Vec<_>>();
            let message = format!(
                "One way to reach {target} with {}:\n  1. start with {}\n  {}\nCombine the two left-over values last.",
                cards.len(),
                card_list(&cards),
                lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{}. {l}", i + 2))
                    .collect::<Vec<_>>()
                    .join("\n  ")
            );
            let payload = ExplainPayload {
                expression: None,
                steps: lines.iter().map(|l| format!("{l} = {target}")).collect(),
                message: message.clone(),
            };
            ok(message, &payload)
        })
    }

    #[tool(
        description = "Show the interactive Snap 24 board for a puzzle (re-shows it after start_puzzle, or to bring the board back).",
        meta = tool_meta(),
        output_schema = rmcp::handler::server::tool::schema_for_output::<BoardPayload>(),
        annotations(title = "Show the board", read_only_hint = true, destructive_hint = false, open_world_hint = false)
    )]
    fn render_board(&self, Parameters(p): Parameters<IdParams>) -> Result<CallToolResult, String> {
        let id = p.puzzle_id.clone();
        self.with_session(&id, |session| {
            let payload = board_payload(&id, &session.puzzle);
            let message = format!(
                "Board: {} cards {} → {}. Pass puzzle_id to submit_solution, hint, reveal or explain.",
                payload.cards.len(),
                card_list(&payload.cards),
                payload.target.n
            );
            ok(message, &payload)
        })
    }
}

#[tool_handler]
impl ServerHandler for Snap24 {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("snap24", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "Snap 24: the 24 game. You deal a hand with start_puzzle, the player combines every \
             card exactly once with + - * / to reach the target, and submit_solution checks it \
             with exact arithmetic. Use hint for progressive help, reveal for the solution set \
             and explain to walk through one. Cards are ranks; A=1, J=11, Q=12, K=13. Targets \
             and sub-results can be fractions. start_puzzle and render_board both open the \
             interactive board, so the player can tap cards themselves; call render_board again \
             to bring the board back, and prefer its puzzle_id for every follow-up call.",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(vec![Resource::new(
            UI_URI,
            "snap24-board",
        )
        .with_title("Snap 24 board")
        .with_description("Interactive Snap 24 game board")
        .with_mime_type(UI_MIME)]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if request.uri != UI_URI {
            return Err(ErrorData::resource_not_found(
                format!("no such resource: {}", request.uri),
                None,
            ));
        }
        Ok(ReadResourceResult::new(vec![ResourceContents::TextResourceContents {
            uri: UI_URI.to_string(),
            mime_type: Some(UI_MIME.to_string()),
            text: BOARD_HTML.to_string(),
            meta: Some(resource_meta()),
        }])
        .into())
    }
}

/// Serve the MCP endpoint over streamable HTTP (what ChatGPT connectors need).
///
/// Public deployments must name their host: rmcp only allows loopback hosts by
/// default, to block DNS-rebinding attacks.
async fn serve_http(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };

    let allowed_hosts: Vec<String> = std::env::var("SNAP24_MCP_ALLOWED_HOSTS")
        .unwrap_or_else(|_| "localhost,127.0.0.1,::1".to_string())
        .split(',')
        .map(|host| host.trim().to_string())
        .filter(|host| !host.is_empty())
        .collect();

    let config = StreamableHttpServerConfig::default().with_allowed_hosts(allowed_hosts);
    let service: StreamableHttpService<Snap24, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(Snap24::new()),
            Arc::new(LocalSessionManager::default()),
            config,
        );

    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("snap24-mcp serving streamable HTTP on http://{addr}/mcp");

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        // Default: stdio, for local tools and Codex.
        None => {
            let service = Snap24::new().serve(stdio()).await?;
            service.waiting().await?;
        }
        Some("--http") => {
            let addr = args.next().unwrap_or_else(|| "127.0.0.1:8787".to_string());
            serve_http(&addr).await?;
        }
        Some(other) => {
            eprintln!("usage: snap24-mcp [--http [ADDR]]  (got {other:?})");
            std::process::exit(2);
        }
    }
    Ok(())
}
