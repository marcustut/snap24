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
    ListResourcesResult, MetaObject, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, ServerCapabilities,
    ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::stdio;
use rmcp::{
    schemars, tool, tool_handler, tool_router, ErrorData, Json, ServerHandler, ServiceExt,
};
use serde::Deserialize;
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

/// `_meta` that links a tool to the widget (`_meta.ui.resourceUri`).
fn ui_meta() -> MetaObject {
    // The widget makes no network calls and loads no external assets (it talks
    // to the host over postMessage only), so the CSP allowlists are empty.
    let value = serde_json::json!({
        "ui": {
            "resourceUri": UI_URI,
            "prefersBorder": true,
            "csp": { "connectDomains": [], "resourceDomains": [] }
        }
    });
    MetaObject(value.as_object().expect("object").clone())
}

/// What `render_board` hands the widget (and mirrors as text for the model).
#[derive(serde::Serialize, schemars::JsonSchema)]
struct BoardView {
    puzzle_id: String,
    mode: String,
    difficulty: String,
    cards: Vec<i64>,
    target_n: i64,
    target_d: i64,
    /// `None` = visible indefinitely, `0` = never shown (Blind).
    view_seconds: Option<u32>,
}

// --------------------------------------------------------------------------- //
// tool parameters                                                             //
// --------------------------------------------------------------------------- //

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct StartParams {
    /// "classic" (target 24) or "custom" (target drawn from the hand).
    mode: Option<String>,
    /// easy | medium | hard | expert | insane | blind (or 1..6).
    difficulty: Option<String>,
    /// Custom only: a specific target the dealt hand must be able to reach.
    target: Option<i64>,
    /// Reproducible deal.
    seed: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct IdParams {
    /// The `puzzle_id` returned by start_puzzle.
    puzzle_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SubmitParams {
    puzzle_id: String,
    /// A player's expression using every card exactly once, e.g. "(9 - 1) * 3".
    expression: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct HintParams {
    puzzle_id: String,
    /// 1 = which two cards, 2 = operator, 3 = result, 4 = full solution.
    /// Omit to advance one level per call.
    level: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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

fn parse_mode(s: Option<&str>) -> Mode {
    match s.unwrap_or("classic").to_ascii_lowercase().as_str() {
        "custom" => Mode::Custom,
        _ => Mode::Classic,
    }
}

fn parse_difficulty(s: Option<&str>) -> Result<Difficulty, String> {
    let name = s.unwrap_or("easy").to_ascii_lowercase();
    match name.as_str() {
        "easy" | "1" => Ok(Difficulty::Easy),
        "medium" | "2" => Ok(Difficulty::Medium),
        "hard" | "3" => Ok(Difficulty::Hard),
        "expert" | "4" => Ok(Difficulty::Expert),
        "insane" | "5" => Ok(Difficulty::Insane),
        "blind" | "6" => Ok(Difficulty::Blind),
        other => Err(format!(
            "unknown difficulty {other:?}; use easy/medium/hard/expert/insane/blind"
        )),
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

    fn count_hints(session: &mut Session, level: Option<u32>) -> u32 {
        session.hint_level = match level {
            Some(l) => l.clamp(1, 4),
            None => (session.hint_level + 1).min(4),
        };
        session.hint_level
    }
}

#[tool_router]
impl Snap24 {
    #[tool(description = "Deal a Snap 24 puzzle. Returns a puzzle_id plus the dealt cards and target.")]
    fn start_puzzle(&self, Parameters(p): Parameters<StartParams>) -> Result<String, String> {
        let mode = parse_mode(p.mode.as_deref());
        let difficulty = parse_difficulty(p.difficulty.as_deref())?;
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

        self.sessions.lock().unwrap().insert(
            id,
            Session {
                puzzle,
                created: Instant::now(),
                hint_level: 0,
            },
        );
        Ok(summary)
    }

    #[tool(description = "Validate a player's expression against the dealt cards and target (exact arithmetic, server-side).")]
    fn submit_solution(&self, Parameters(p): Parameters<SubmitParams>) -> Result<String, String> {
        let id = p.puzzle_id.clone();
        self.with_session(&id, |session| {
            let cards = session.puzzle.cards.clone();
            let target = session.puzzle.target;
            match evaluate(&cards, &p.expression) {
                Ok(value) if value == target => Ok(format!(
                    "accepted: {} = {target} — solved.",
                    p.expression.trim()
                )),
                Ok(value) => Ok(format!(
                    "valid expression but {} = {value}, not {target}. Keep going.",
                    p.expression.trim()
                )),
                Err(err) => Ok(format!("rejected: {err}")),
            }
        })
    }

    #[tool(description = "Progressive hint: 1 = which two cards, 2 = the operator, 3 = the sub-result, 4 = the full solution. Advances one level per call unless a level is given.")]
    fn hint(&self, Parameters(p): Parameters<HintParams>) -> Result<String, String> {
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
            let Some(step) = first_move(&board, target) else {
                return Ok("No solution from here — call start_puzzle for a fresh deal.".into());
            };
            Ok(match level {
                1 => format!("Hint 1 — combine {} and {}.", step.left, step.right),
                2 => format!("Hint 2 — use '{}' on {} and {}.", step.op, step.left, step.right),
                3 => format!("Hint 3 — {} {} {} = {}", step.left, step.op, step.right, step.result),
                _ => {
                    let steps = move_sequence(&board, target);
                    format!(
                        "Hint 4 — full solution: {}",
                        steps.iter().map(move_line).collect::<Vec<_>>().join(" ; ")
                    )
                }
            })
        })
    }

    #[tool(description = "List distinct solutions for the current puzzle (canonical, de-duplicated).")]
    fn reveal(&self, Parameters(p): Parameters<IdParams>) -> Result<String, String> {
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
            if all.is_empty() {
                return Ok("No solutions — this hand can't reach the target.".into());
            }
            let shown = all.iter().take(REVEAL_LIMIT).cloned().collect::<Vec<_>>().join(" ; ");
            let more = all.len().saturating_sub(REVEAL_LIMIT);
            Ok(if more > 0 {
                format!("{} solutions (showing {REVEAL_LIMIT}, +{more} more): {shown}", all.len())
            } else {
                format!("{} solutions: {shown}", all.len())
            })
        })
    }

    #[tool(description = "Explain a solution step by step in words (uses the player's expression if given, otherwise one solution).")]
    fn explain(&self, Parameters(p): Parameters<ExplainParams>) -> Result<String, String> {
        let id = p.puzzle_id.clone();
        self.with_session(&id, |session| {
            let cards = session.puzzle.cards.clone();
            let board = cards.iter().copied().map(Rational::from).collect::<Vec<_>>();
            let target = session.puzzle.target;

            if let Some(expr) = p.expression.as_deref() {
                return Ok(match evaluate(&cards, expr) {
                    Ok(value) if value == target => format!(
                        "'{}' uses every card once and equals {target}. ({} cards from {}.)",
                        expr.trim(),
                        cards.len(),
                        card_list(&cards)
                    ),
                    Ok(value) => format!("'{}' evaluates to {value}, not {target}.", expr.trim()),
                    Err(err) => format!("Can't explain '{}': {err}", expr.trim()),
                });
            }

            let steps = move_sequence(&board, target);
            if steps.is_empty() {
                return Ok("No solution to explain for this hand.".into());
            }
            let lines = steps.iter().map(move_line).collect::<Vec<_>>();
            Ok(format!(
                "One way to reach {target} with {}:\n  1. start with {}\n  {}\nCombine the two left-over values last.",
                cards.len(),
                card_list(&cards),
                lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{}. {l}", i + 2))
                    .collect::<Vec<_>>()
                    .join("\n  ")
            ))
        })
    }

    #[tool(
        description = "Render the Snap 24 board widget. Deal with start_puzzle first, then pass its puzzle_id here to show the interactive board.",
        meta = ui_meta()
    )]
    fn render_board(&self, Parameters(p): Parameters<IdParams>) -> Result<Json<BoardView>, String> {
        let id = p.puzzle_id.clone();
        self.with_session(&id, |session| {
            let (n, d) = session.puzzle.target.parts();
            Ok(Json(BoardView {
                puzzle_id: id.clone(),
                mode: session.puzzle.mode.label().to_string(),
                difficulty: session.puzzle.difficulty.label().to_string(),
                cards: session.puzzle.cards.clone(),
                target_n: n,
                target_d: d,
                view_seconds: session.puzzle.difficulty.view_seconds(),
            }))
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
        .with_instructions(
            "Snap 24: the 24 game. You deal a hand with start_puzzle, the player combines every \
             card exactly once with + - * / to reach the target, and submit_solution checks it \
             with exact arithmetic. Use hint for progressive help, reveal for the solution set \
             and explain to walk through one. Cards are ranks; A=1, J=11, Q=12, K=13. Targets \
             and sub-results can be fractions.",
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
            meta: Some(ui_meta()),
        }])
        .into())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // stdout is the MCP channel, so never print anything else to it.
    let service = Snap24::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
