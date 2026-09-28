//! Scripted playthroughs + screenshots, for debugging (the `devtools` feature).
//!
//! Never enabled in normal builds. Set `SNAP24_SCRIPT` to a `;`-separated list
//! of steps and `SNAP24_SHOTS` to an output directory, then:
//!
//! ```text
//! SNAP24_SCRIPT="mode:custom;diff:2;card:0;op:-;card:1;shot:after-sub;undo;shot:after-undo" \
//! SNAP24_SHOTS=/tmp/s24 \
//! cargo run -p snap24-app --features devtools
//! ```
//!
//! Steps are driven one at a time with a few frames between them, using the
//! same `Game` methods the real click handlers call, so a script exercises the
//! actual game logic. Screenshots are captured three frames apart (Bevy's
//! capture occasionally lands a black frame; the `name.0/2/4` variants let you
//! pick a good one).
//!
//! Steps: `play`, `mode:classic|custom`, `diff:1..6`, `card:N`, `op:+|-|*|/`,
//! `undo`, `new`, `menu`, `back`, `target:<n|random>`, `start`, `hint`,
//! `reveal`, `more`, `step` (play the next optimal merge), `solve` (play the
//! whole board), `wait:N`, `shot:NAME`. Set `SNAP24_SEED` for a reproducible
//! deal.

use crate::logic::{Op, Phase};
use crate::{
    begin_game, format_solution, hint_text, reveal_solutions, Game, Screen, TargetEntry, ViewTimer,
};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use snap24_core::{first_move, Difficulty, Mode, Rational};

pub struct DevtoolsPlugin;

impl Plugin for DevtoolsPlugin {
    fn build(&self, app: &mut App) {
        let script = std::env::var("SNAP24_SCRIPT").unwrap_or_default();
        if script.trim().is_empty() {
            return;
        }
        app.insert_resource(Script {
            steps: parse(&script),
            index: 0,
            wait: 0,
            shots: std::env::var("SNAP24_SHOTS").unwrap_or_else(|_| "/tmp/s24-shots".into()),
            pending: Vec::new(),
            drain: None,
        });
        app.add_systems(Update, run_script);
    }
}

#[derive(Resource)]
struct Script {
    steps: Vec<Step>,
    index: usize,
    /// Frames to idle before the next step.
    wait: u32,
    shots: String,
    /// Screenshots scheduled for later frames: (frames_from_now, name).
    pending: Vec<(u32, String)>,
    /// Once the script ends, frames left to idle before exiting.
    drain: Option<u32>,
}

#[derive(Clone)]
enum Step {
    Play,
    Mode(Mode),
    Difficulty(Difficulty),
    Card(usize),
    Op(Op),
    Undo,
    New,
    Menu,
    Back,
    Target(Option<i64>),
    Start,
    Hint,
    Reveal,
    More,
    /// Play the next optimal move (core `first_move`) — one merge.
    Auto,
    /// Play the whole solution to the target.
    AutoSolve,
    Wait(u32),
    Shot(String),
}

fn parse(script: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    for raw in script.split(';') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let (name, arg) = match raw.split_once(':') {
            Some((name, arg)) => (name, Some(arg)),
            None => (raw, None),
        };
        match (name, arg) {
            ("play", _) => steps.push(Step::Play),
            ("mode", Some("classic")) => steps.push(Step::Mode(Mode::Classic)),
            ("mode", Some("custom")) => steps.push(Step::Mode(Mode::Custom)),
            ("diff", Some(value)) => {
                let index = value
                    .parse::<usize>()
                    .unwrap_or(1)
                    .clamp(1, Difficulty::ALL.len());
                steps.push(Step::Difficulty(Difficulty::ALL[index - 1]));
            }
            ("card", Some(value)) => steps.push(Step::Card(value.parse().unwrap_or(0))),
            ("op", Some(value)) => steps.push(Step::Op(match value {
                "-" => Op::Sub,
                "*" => Op::Mul,
                "/" => Op::Div,
                _ => Op::Add,
            })),
            ("undo", _) => steps.push(Step::Undo),
            ("new", _) => steps.push(Step::New),
            ("target", Some("random")) => steps.push(Step::Target(None)),
            ("target", Some(value)) => steps.push(Step::Target(value.parse().ok())),
            ("start", _) => steps.push(Step::Start),
            ("hint", _) => steps.push(Step::Hint),
            ("reveal", _) => steps.push(Step::Reveal),
            ("more", _) => steps.push(Step::More),
            ("step", _) => steps.push(Step::Auto),
            ("solve", _) => steps.push(Step::AutoSolve),
            ("menu", _) => steps.push(Step::Menu),
            ("back", _) => steps.push(Step::Back),
            ("wait", Some(value)) => steps.push(Step::Wait(value.parse().unwrap_or(1))),
            ("shot", Some(value)) => steps.push(Step::Shot(value.to_string())),
            _ => warn!("devtools: ignoring unknown step {raw:?}"),
        }
    }
    steps
}

#[allow(clippy::too_many_arguments)] // Bevy systems routinely take many params
fn run_script(
    mut script: ResMut<Script>,
    mut commands: Commands,
    screen: Res<State<Screen>>,
    mut game: ResMut<Game>,
    mut timer: ResMut<ViewTimer>,
    mut entry: ResMut<TargetEntry>,
    time: Res<Time>,
    mut next: ResMut<NextState<Screen>>,
    mut exit: MessageWriter<AppExit>,
) {
    capture_pending(&mut script, &mut commands);

    if let Some(remaining) = script.drain {
        if remaining == 0 {
            exit.write(AppExit::Success);
        } else {
            script.drain = Some(remaining - 1);
        }
        return;
    }
    if script.wait > 0 {
        script.wait -= 1;
        return;
    }
    if script.index >= script.steps.len() {
        script.drain = Some(30);
        return;
    }

    let step = script.steps[script.index].clone();
    script.index += 1;

    match step {
        Step::Play => next.set(Screen::ModeSelect),
        Step::Mode(mode) => {
            game.mode = mode;
            next.set(Screen::DifficultySelect);
        }
        Step::Difficulty(difficulty) => {
            game.difficulty = difficulty;
            if game.mode == Mode::Custom {
                next.set(Screen::TargetSelect);
            } else {
                begin_game(&mut game, &mut timer, time.elapsed_secs(), &entry);
                next.set(Screen::Playing);
            }
        }
        Step::Card(index) => game.play_card(index),
        Step::Op(op) => game.play_op(op),
        Step::Undo => game.undo(),
        Step::Auto => {
            solve_step(&mut game);
        }
        Step::AutoSolve => {
            while game.round.phase == Phase::Playing && solve_step(&mut game) {}
        }
        Step::Hint => {
            if game.round.phase == Phase::Playing {
                game.hint_level = (game.hint_level + 1).min(4);
                game.hints_used += 1;
                game.message = hint_text(&game);
            }
        }
        Step::Reveal => {
            let solutions = reveal_solutions(&game);
            game.reveal_total = solutions.len();
            game.reveal_shown = usize::from(!solutions.is_empty());
            game.message = format_solution(&solutions, game.reveal_shown);
        }
        Step::More => {
            if game.reveal_shown < game.reveal_total {
                game.reveal_shown += 1;
            }
            let solutions = reveal_solutions(&game);
            game.reveal_total = solutions.len();
            game.message = format_solution(&solutions, game.reveal_shown);
        }
        Step::New => crate::next_puzzle(&mut game, &mut timer, time.elapsed_secs()),
        Step::Menu => next.set(Screen::ModeSelect),
        Step::Target(value) => {
            entry.clear();
            if let Some(value) = value {
                for digit in value.to_string().chars() {
                    if let Some(digit) = digit.to_digit(10) {
                        entry.push(digit);
                    }
                }
            }
        }
        Step::Start => {
            begin_game(&mut game, &mut timer, time.elapsed_secs(), &entry);
            next.set(Screen::Playing);
        }
        Step::Back => next.set(match screen.get() {
            Screen::ModeSelect => Screen::Title,
            Screen::DifficultySelect => Screen::ModeSelect,
            Screen::TargetSelect => Screen::DifficultySelect,
            Screen::Playing => Screen::ModeSelect,
            Screen::Title => Screen::Title,
        }),
        Step::Wait(frames) => script.wait = frames,
        Step::Shot(name) => {
            // Capture now, plus a fallback a few frames later (Bevy's capture
            // occasionally lands a black frame). Both complete before the next
            // step because of the extra wait below.
            script.pending.push((0, name.clone()));
            script.pending.push((4, format!("{name}.alt")));
            script.wait = script.wait.max(6);
        }
    }

    // Let the transition/UI settle before the next step.
    script.wait = script.wait.max(2);
}

/// Plays one optimal merge on the current board. Returns false if the board is
/// not in play or no move is available.
fn solve_step(game: &mut Game) -> bool {
    if game.round.phase != Phase::Playing {
        return false;
    }
    let Some(mv) = first_move(&game.round.cards, game.round.target) else {
        return false;
    };
    let Some(left) = value_index(&game.round.cards, mv.left, None) else {
        return false;
    };
    let Some(right) = value_index(&game.round.cards, mv.right, Some(left)) else {
        return false;
    };
    game.play_card(left);
    game.play_op(op_of(mv.op));
    game.play_card(right);
    true
}

fn value_index(cards: &[Rational], value: Rational, skip: Option<usize>) -> Option<usize> {
    cards
        .iter()
        .enumerate()
        .find(|(index, card)| **card == value && Some(*index) != skip)
        .map(|(index, _)| index)
}

fn op_of(symbol: char) -> Op {
    match symbol {
        '-' => Op::Sub,
        '*' => Op::Mul,
        '/' => Op::Div,
        _ => Op::Add,
    }
}

fn capture_pending(script: &mut Script, commands: &mut Commands) {
    let mut still_due = Vec::new();
    for (delay, name) in script.pending.drain(..) {
        if delay == 0 {
            let path = format!("{}/{}.png", script.shots, name);
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
        } else {
            still_due.push((delay - 1, name));
        }
    }
    script.pending = still_due;
}
