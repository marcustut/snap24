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
//! `undo`, `new`, `menu`, `back`, `wait:N`, `shot:NAME`. Set `SNAP24_SEED` for a
//! reproducible deal.

use crate::logic::Op;
use crate::{deal, deal_custom, Game, Screen, TargetEntry, ViewTimer};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use snap24_core::{Difficulty, Mode};

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
                deal(&mut game, &mut timer, time.elapsed_secs());
                next.set(Screen::Playing);
            }
        }
        Step::Card(index) => game.play_card(index),
        Step::Op(op) => game.play_op(op),
        Step::Undo => game.undo(),
        Step::New => deal(&mut game, &mut timer, time.elapsed_secs()),
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
            if game.mode == Mode::Custom {
                deal_custom(&mut game, &mut timer, time.elapsed_secs(), &entry);
            } else {
                deal(&mut game, &mut timer, time.elapsed_secs());
            }
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
