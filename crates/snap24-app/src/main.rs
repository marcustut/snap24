//! Snap 24 desktop app — playable core loop with mode/difficulty flow and
//! scoring.
//!
//! Bevy owns rendering, input and the screen flow; [`logic::Round`] owns the
//! rules and [`logic::round_score`] the scoring. The board is rebuilt from the
//! [`Game`] resource whenever it changes.

mod logic;

use bevy::prelude::*;
use logic::{round_score, MergeError, Op, Phase, Round, ViewPhase};
use snap24_core::{generate, Difficulty, Mode, Rng};
use std::time::{SystemTime, UNIX_EPOCH};

const BG: Color = Color::srgb(0.07, 0.08, 0.11);
const CARD: Color = Color::srgb(0.16, 0.18, 0.24);
const CARD_SELECTED: Color = Color::srgb(0.20, 0.55, 0.32);
const KEY: Color = Color::srgb(0.20, 0.23, 0.30);
const BORDER: Color = Color::srgb(0.30, 0.34, 0.42);
const TEXT: Color = Color::srgb(0.93, 0.95, 0.98);
const MUTED: Color = Color::srgb(0.72, 0.76, 0.84);

/// Screen flow: Title → Mode → Difficulty → Play.
#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum Screen {
    #[default]
    Title,
    ModeSelect,
    DifficultySelect,
    Playing,
}

#[derive(Resource)]
struct Game {
    round: Round,
    rng: Rng,
    mode: Mode,
    difficulty: Difficulty,
    error: Option<String>,
    hints_used: u32,
    started_at: f32,
    settled: bool,
    last_score: Option<i32>,
    total_score: i32,
}

impl Game {
    fn new() -> Self {
        Game {
            round: Round::new(Vec::new(), 0i64.into()),
            rng: Rng::new(seed_from_time()),
            mode: Mode::Classic,
            difficulty: Difficulty::Easy,
            error: None,
            hints_used: 0,
            started_at: 0.0,
            settled: false,
            last_score: None,
            total_score: 0,
        }
    }

    fn status(&self) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        match self.round.phase {
            Phase::Playing => match (self.round.first, self.round.op) {
                (None, _) => format!("{} cards left — pick a card", self.round.cards.len()),
                (Some(_), None) => "Pick an operator".to_string(),
                (Some(_), Some(_)) => "Pick the second card".to_string(),
            },
            Phase::Won => format!("Correct! You made {}.", self.round.target),
            Phase::Lost => format!(
                "Not {}. You made {}.",
                self.round.target, self.round.cards[0]
            ),
        }
    }
}

/// The view window lives in its own resource so the per-frame countdown does
/// not mark `Game` changed and rebuild the board every frame.
#[derive(Resource)]
struct ViewTimer {
    phase: ViewPhase,
}

impl Default for ViewTimer {
    fn default() -> Self {
        ViewTimer {
            phase: ViewPhase::Unlimited,
        }
    }
}

fn deal(game: &mut Game, timer: &mut ViewTimer, now: f32) {
    let puzzle = generate(game.mode, game.difficulty, &mut game.rng);
    game.round = Round::new(puzzle.cards, puzzle.target);
    timer.phase = ViewPhase::from_view_seconds(game.difficulty.view_seconds());
    if timer.phase == ViewPhase::Hidden {
        game.round.conceal();
    }
    game.error = None;
    game.hints_used = 0;
    game.started_at = now;
    game.settled = false;
    game.last_score = None;
}

fn seed_from_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED)
}

// --------------------------------------------------------------------------- //
// components                                                                   //
// --------------------------------------------------------------------------- //

#[derive(Component)]
struct ScreenRoot;

#[derive(Component)]
struct Board;

#[derive(Component)]
struct PlayButton;

#[derive(Component)]
struct ModeButton(Mode);

#[derive(Component)]
struct DifficultyButton(Difficulty);

#[derive(Component)]
struct BackButton;

#[derive(Component)]
struct CardButton(usize);

#[derive(Component)]
struct OperatorButton(Op);

#[derive(Component)]
struct UndoButton;

#[derive(Component)]
struct NewPuzzleButton;

#[derive(Component)]
struct CountdownLabel;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Snap 24".to_string(),
                ..default()
            }),
            ..default()
        }))
        .init_state::<Screen>()
        .insert_resource(Game::new())
        .init_resource::<ViewTimer>()
        .add_systems(Startup, setup)
        .add_systems(OnEnter(Screen::Title), spawn_title)
        .add_systems(OnEnter(Screen::ModeSelect), spawn_mode_select)
        .add_systems(OnEnter(Screen::DifficultySelect), spawn_difficulty_select)
        .add_systems(OnExit(Screen::Title), cleanup_screen)
        .add_systems(OnExit(Screen::ModeSelect), cleanup_screen)
        .add_systems(OnExit(Screen::DifficultySelect), cleanup_screen)
        .add_systems(OnExit(Screen::Playing), cleanup_screen)
        .add_systems(
            Update,
            (
                play_button,
                mode_buttons,
                difficulty_buttons,
                back_buttons,
            ),
        )
        .add_systems(
            Update,
            (
                tick_view,
                card_click,
                operator_click,
                undo_click,
                new_puzzle,
                settle_round,
                rebuild_board.run_if(resource_changed::<Game>),
                update_countdown,
            )
                .chain()
                .run_if(in_state(Screen::Playing)),
        )
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

// --------------------------------------------------------------------------- //
// menu screens                                                                 //
// --------------------------------------------------------------------------- //

fn cleanup_screen(mut commands: Commands, roots: Query<Entity, With<ScreenRoot>>) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}

fn root(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            ScreenRoot,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(22),
                padding: UiRect::all(px(24)),
                ..default()
            },
            BackgroundColor(BG),
        ))
        .id()
}

fn button<M: Bundle>(parent: &mut ChildSpawnerCommands, label: &str, marker: M) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                padding: UiRect::axes(px(28), px(14)),
                border: UiRect::all(px(3)),
                border_radius: BorderRadius::all(px(12)),
                min_width: px(180),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(KEY),
            BorderColor::all(BORDER),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(TEXT),
            ));
        });
}

fn heading(parent: &mut ChildSpawnerCommands, text: &str, size: f32) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(TEXT),
    ));
}

fn spawn_title(mut commands: Commands) {
    let root = root(&mut commands);
    commands.entity(root).with_children(|ui| {
        heading(ui, "SNAP 24", 72.0);
        heading(ui, "Make the target from every card.", 24.0);
        button(ui, "Play", PlayButton);
    });
}

fn spawn_mode_select(mut commands: Commands) {
    let root = root(&mut commands);
    commands.entity(root).with_children(|ui| {
        heading(ui, "Choose mode", 40.0);
        heading(ui, "Classic: five cards, target 24.", 20.0);
        heading(ui, "Custom: tiers change the card count and target.", 20.0);
        button(ui, "Classic", ModeButton(Mode::Classic));
        button(ui, "Custom", ModeButton(Mode::Custom));
        button(ui, "Back", BackButton);
    });
}

fn spawn_difficulty_select(mut commands: Commands) {
    let root = root(&mut commands);
    commands.entity(root).with_children(|ui| {
        heading(ui, "Choose difficulty", 40.0);
        for difficulty in Difficulty::ALL {
            button(ui, difficulty.label(), DifficultyButton(difficulty));
        }
        button(ui, "Back", BackButton);
    });
}

fn play_button(
    interactions: Query<&Interaction, (Changed<Interaction>, With<PlayButton>)>,
    mut next: ResMut<NextState<Screen>>,
) {
    if pressed(&interactions) {
        next.set(Screen::ModeSelect);
    }
}

fn mode_buttons(
    interactions: Query<(&Interaction, &ModeButton), Changed<Interaction>>,
    mut game: ResMut<Game>,
    mut next: ResMut<NextState<Screen>>,
) {
    for (interaction, mode) in &interactions {
        if *interaction == Interaction::Pressed {
            game.mode = mode.0;
            next.set(Screen::DifficultySelect);
        }
    }
}

fn difficulty_buttons(
    interactions: Query<(&Interaction, &DifficultyButton), Changed<Interaction>>,
    mut game: ResMut<Game>,
    mut timer: ResMut<ViewTimer>,
    time: Res<Time>,
    mut next: ResMut<NextState<Screen>>,
) {
    for (interaction, difficulty) in &interactions {
        if *interaction == Interaction::Pressed {
            game.difficulty = difficulty.0;
            deal(&mut game, &mut timer, time.elapsed_secs());
            next.set(Screen::Playing);
        }
    }
}

fn back_buttons(
    interactions: Query<&Interaction, (Changed<Interaction>, With<BackButton>)>,
    screen: Res<State<Screen>>,
    mut next: ResMut<NextState<Screen>>,
) {
    if !pressed(&interactions) {
        return;
    }
    next.set(match screen.get() {
        Screen::ModeSelect => Screen::Title,
        Screen::DifficultySelect => Screen::ModeSelect,
        Screen::Playing => Screen::ModeSelect,
        Screen::Title => Screen::Title,
    });
}

fn pressed<F: bevy::ecs::query::QueryFilter>(query: &Query<&Interaction, F>) -> bool {
    query
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
}

// --------------------------------------------------------------------------- //
// play systems                                                                 //
// --------------------------------------------------------------------------- //

/// Counts down the view window and flips the cards face-down on expiry.
fn tick_view(time: Res<Time>, mut timer: ResMut<ViewTimer>, mut game: ResMut<Game>) {
    if timer.phase.tick(time.delta_secs()) {
        game.round.conceal();
    }
}

fn settle_round(time: Res<Time>, mut game: ResMut<Game>) {
    if game.settled || game.round.phase == Phase::Playing {
        return;
    }
    let won = game.round.phase == Phase::Won;
    let elapsed = (time.elapsed_secs() - game.started_at).max(0.0);
    let score = round_score(
        game.difficulty.score_multiplier(),
        elapsed,
        game.hints_used,
        won,
    );
    game.total_score += score;
    game.last_score = Some(score);
    game.settled = true;
}

fn card_click(
    mut game: ResMut<Game>,
    interactions: Query<(&Interaction, &CardButton), Changed<Interaction>>,
) {
    for (interaction, card) in &interactions {
        if *interaction == Interaction::Pressed {
            game.error = match game.round.click_card(card.0) {
                Ok(()) => None,
                Err(MergeError::DivideByZero) => Some("Can't divide by zero".to_string()),
                Err(MergeError::PickCardFirst) => Some("Pick a card first".to_string()),
            };
        }
    }
}

fn operator_click(
    mut game: ResMut<Game>,
    interactions: Query<(&Interaction, &OperatorButton), Changed<Interaction>>,
) {
    for (interaction, operator) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        game.error = match game.round.click_op(operator.0) {
            Ok(()) => None,
            Err(MergeError::DivideByZero) => Some("Can't divide by zero".to_string()),
            Err(MergeError::PickCardFirst) => Some("Pick a card first".to_string()),
        };
    }
}

fn undo_click(
    mut game: ResMut<Game>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<UndoButton>)>,
) {
    if pressed(&interactions) {
        game.round.undo();
        game.error = None;
    }
}

fn new_puzzle(
    mut game: ResMut<Game>,
    mut timer: ResMut<ViewTimer>,
    time: Res<Time>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<NewPuzzleButton>)>,
) {
    if pressed(&interactions) {
        deal(&mut game, &mut timer, time.elapsed_secs());
    }
}

fn update_countdown(timer: Res<ViewTimer>, mut labels: Query<&mut Text, With<CountdownLabel>>) {
    let text = match timer.phase.seconds_left() {
        Some(seconds) => format!("Remember! Hiding in {seconds}s"),
        None => String::new(),
    };
    for mut label in &mut labels {
        **label = text.clone();
    }
}

fn rebuild_board(mut commands: Commands, game: Res<Game>, existing: Query<Entity, With<Board>>) {
    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let root = commands
        .spawn((
            ScreenRoot,
            Board,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(18),
                padding: UiRect::all(px(24)),
                ..default()
            },
            BackgroundColor(BG),
        ))
        .id();

    commands.entity(root).with_children(|ui| {
        heading(
            ui,
            &format!(
                "Target: {}   ({} · {})",
                game.round.target,
                game.mode.label(),
                game.difficulty.label()
            ),
            40.0,
        );

        ui.spawn((
            CountdownLabel,
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(22.0),
                ..default()
            },
            TextColor(Color::srgb(0.98, 0.75, 0.35)),
        ));

        ui.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(14),
            flex_wrap: FlexWrap::Wrap,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            for (index, label) in game.round.card_labels().iter().enumerate() {
                let selected = game.round.is_first(index);
                let shown = if game.round.is_revealed(index) {
                    label.clone()
                } else {
                    "?".to_string()
                };
                row.spawn((
                    Button,
                    CardButton(index),
                    Node {
                        width: px(96),
                        height: px(132),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(14)),
                        ..default()
                    },
                    BackgroundColor(if selected { CARD_SELECTED } else { CARD }),
                    BorderColor::all(if selected { TEXT } else { BORDER }),
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new(shown),
                        TextFont {
                            font_size: FontSize::Px(48.0),
                            ..default()
                        },
                        TextColor(TEXT),
                    ));
                });
            }
        });

        ui.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(14),
            ..default()
        })
        .with_children(|row| {
            for operator in Op::ALL {
                let pending = game.round.op == Some(operator);
                row.spawn((
                    Button,
                    OperatorButton(operator),
                    Node {
                        width: px(76),
                        height: px(58),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(12)),
                        ..default()
                    },
                    BackgroundColor(if pending { CARD_SELECTED } else { KEY }),
                    BorderColor::all(if pending { TEXT } else { BORDER }),
                ))
                .with_children(|key| {
                    key.spawn((
                        Text::new(operator.symbol().to_string()),
                        TextFont {
                            font_size: FontSize::Px(30.0),
                            ..default()
                        },
                        TextColor(TEXT),
                    ));
                });
            }
        });

        ui.spawn((
            Text::new(game.status()),
            TextFont {
                font_size: FontSize::Px(24.0),
                ..default()
            },
            TextColor(MUTED),
        ));

        let score_line = match game.last_score {
            Some(score) => format!("Round score: +{score}    Total: {}", game.total_score),
            None => format!("Score: {}", game.total_score),
        };
        ui.spawn((
            Text::new(score_line),
            TextFont {
                font_size: FontSize::Px(22.0),
                ..default()
            },
            TextColor(MUTED),
        ));

        ui.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(14),
            ..default()
        })
        .with_children(|row| {
            let undo_enabled = game.round.can_undo();
            row.spawn((
                Button,
                UndoButton,
                Node {
                    padding: UiRect::axes(px(24), px(12)),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::all(px(12)),
                    ..default()
                },
                BackgroundColor(if undo_enabled { KEY } else { BG }),
                BorderColor::all(if undo_enabled { BORDER } else { BG }),
            ))
            .with_children(|button| {
                button.spawn((
                    Text::new("Undo"),
                    TextFont {
                        font_size: FontSize::Px(24.0),
                        ..default()
                    },
                    TextColor(if undo_enabled { TEXT } else { MUTED }),
                ));
            });

            button(row, "New Puzzle", NewPuzzleButton);
            button(row, "Menu", BackButton);
        });
    });
}
