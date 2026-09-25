//! Snap 24 desktop app — playable core loop with mode/difficulty flow and
//! scoring.
//!
//! Bevy owns rendering, input and the screen flow; [`logic::Round`] owns the
//! rules and [`logic::round_score`] the scoring.
//!
//! The board root and its widgets are spawned once on `OnEnter(Screen::Playing)`
//! and then *updated in place* from the [`Game`] resource. We deliberately do
//! not despawn and re-spawn UI entities in the same frame: in Bevy 0.19 that
//! leaves the UI unrendered (black window), so the play loop only mutates
//! existing nodes.

mod logic;

#[cfg(feature = "devtools")]
mod devtools;

use bevy::color::Mix;
use bevy::input_focus::tab_navigation::{TabIndex, TabNavigationPlugin};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{
    observe, slider_self_update, Slider, SliderOrientation, SliderRange, SliderStep, SliderThumb,
    SliderValue, TrackClick,
};
use logic::{round_score, MergeError, Op, Phase, Round, Suit, ViewPhase};
use snap24_core::{
    evaluate, first_move, generate, generate_targeted, move_sequence, solutions_infix,
    solutions_steps, Difficulty, Mode, Move, Puzzle, Rational, Rng,
};
use std::time::{SystemTime, UNIX_EPOCH};

const BG: Color = Color::srgb(0.043, 0.043, 0.047);
const PANEL: Color = Color::srgb(0.082, 0.082, 0.090);
const CARD: Color = Color::srgb(0.97, 0.97, 0.96);
const CARD_INK: Color = Color::srgb(0.063, 0.063, 0.071);
const CARD_RED: Color = Color::srgb(0.82, 0.20, 0.18);
const CARD_BACK: Color = Color::srgb(0.15, 0.16, 0.18);
const CARD_SELECTED: Color = Color::srgb(0.91, 0.76, 0.34);
const TOKEN: Color = Color::srgb(0.12, 0.12, 0.14);
const KEY: Color = Color::srgb(0.11, 0.11, 0.13);
const BORDER: Color = Color::srgb(0.20, 0.21, 0.24);
const TEXT: Color = Color::srgb(0.93, 0.93, 0.94);
const MUTED: Color = Color::srgb(0.54, 0.57, 0.63);
const GOLD: Color = Color::srgb(0.91, 0.76, 0.34);
const GREEN: Color = Color::srgb(0.24, 0.86, 0.52);
const WIN: Color = Color::srgb(0.24, 0.86, 0.52);
const LOSE: Color = Color::srgb(0.90, 0.36, 0.34);

/// Largest hand any mode deals (Classic/Custom Easy are 5). Extra card slots
/// are spawned up front and hidden when unused.
const MAX_CARDS: usize = 5;

/// Screen flow: Title → Mode → Difficulty → (Custom target) → Play.
#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum Screen {
    #[default]
    Title,
    ModeSelect,
    DifficultySelect,
    /// Custom only: random target, or type your own.
    TargetSelect,
    Playing,
}

/// The target the player is typing on the Custom target screen. Empty digits
/// means "random".
#[derive(Resource, Default)]
struct TargetEntry {
    digits: String,
}

impl TargetEntry {
    fn push(&mut self, digit: u32) {
        if self.digits.len() < 4 {
            self.digits.push(char::from_digit(digit, 10).unwrap());
        }
    }

    fn clear(&mut self) {
        self.digits.clear();
    }

    fn value(&self) -> Option<i64> {
        self.digits.parse().ok()
    }

    fn label(&self) -> String {
        match self.value() {
            Some(value) => format!("Target: {value}"),
            None => "Target: Random".to_string(),
        }
    }
}

#[derive(Resource)]
struct Game {
    round: Round,
    rng: Rng,
    mode: Mode,
    difficulty: Difficulty,
    error: Option<String>,
    /// The original dealt cards, kept for reveal (the board mutates as you play).
    dealt: Vec<i64>,
    hints_used: u32,
    hint_level: u32,
    message: String,
    /// How many reveal solutions are currently shown, and how many exist.
    reveal_shown: usize,
    reveal_total: usize,
    started_at: f32,
    settled: bool,
    last_score: Option<i32>,
    total_score: i32,
}

impl Game {
    fn new() -> Self {
        Game {
            round: Round::new(Vec::new(), 0i64.into()),
            rng: Rng::new(initial_seed()),
            mode: Mode::Classic,
            difficulty: Difficulty::Easy,
            error: None,
            dealt: Vec::new(),
            hints_used: 0,
            hint_level: 0,
            message: String::new(),
            reveal_shown: 0,
            reveal_total: 0,
            started_at: 0.0,
            settled: false,
            last_score: None,
            total_score: 0,
        }
    }

    /// Handle a card tap. Shared by the click system and the devtools harness.
    fn play_card(&mut self, index: usize) {
        self.error = match self.round.click_card(index) {
            Ok(()) => None,
            Err(MergeError::DivideByZero) => Some("Can't divide by zero".to_string()),
            Err(MergeError::PickCardFirst) => Some("Pick a card first".to_string()),
        };
    }

    /// Handle an operator tap.
    fn play_op(&mut self, op: Op) {
        self.error = match self.round.click_op(op) {
            Ok(()) => None,
            Err(MergeError::DivideByZero) => Some("Can't divide by zero".to_string()),
            Err(MergeError::PickCardFirst) => Some("Pick a card first".to_string()),
        };
    }

    fn undo(&mut self) {
        self.round.undo();
        self.error = None;
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

/// Entity handles for the persistent board widgets, so updates can target them
/// directly without despawning anything.
#[derive(Resource)]
struct BoardUi {
    target: Entity,
    countdown: Entity,
    countdown_pill: Entity,
    status: Entity,
    score: Entity,
    message: Entity,
    undo: Entity,
    undo_label: Entity,
    more: Entity,
    cards: [Entity; MAX_CARDS],
    ranks: [Entity; MAX_CARDS],
    suits: [Entity; MAX_CARDS],
    operators: [Entity; 4],
    operator_labels: [Entity; 4],
}

fn start_puzzle(game: &mut Game, timer: &mut ViewTimer, now: f32, puzzle: Puzzle) {
    game.dealt = puzzle.cards.clone();
    game.round = Round::new(puzzle.cards, puzzle.target);
    timer.phase = ViewPhase::from_view_seconds(game.difficulty.view_seconds());
    if timer.phase == ViewPhase::Hidden {
        game.round.conceal();
    }
    game.error = None;
    game.hints_used = 0;
    game.hint_level = 0;
    game.message = String::new();
    game.reveal_shown = 0;
    game.reveal_total = 0;
    game.started_at = now;
    game.settled = false;
    game.last_score = None;
}

fn deal(game: &mut Game, timer: &mut ViewTimer, now: f32) {
    let puzzle = generate(game.mode, game.difficulty, &mut game.rng);
    start_puzzle(game, timer, now, puzzle);
}

/// Start a fresh game from the menu. Resets the running total, which otherwise
/// accumulates across rounds (including "New Puzzle") within one game.
fn begin_game(game: &mut Game, timer: &mut ViewTimer, now: f32, entry: &TargetEntry) {
    game.total_score = 0;
    if game.mode == Mode::Custom {
        deal_custom(game, timer, now, entry);
    } else {
        deal(game, timer, now);
    }
}

/// Custom start: use the typed target if there is one, otherwise random.
fn deal_custom(game: &mut Game, timer: &mut ViewTimer, now: f32, entry: &TargetEntry) {
    let puzzle = match entry.value() {
        Some(target) => generate_targeted(
            Mode::Custom,
            game.difficulty,
            Rational::from(target),
            &mut game.rng,
        ),
        None => generate(Mode::Custom, game.difficulty, &mut game.rng),
    };
    start_puzzle(game, timer, now, puzzle);
}

fn initial_seed() -> u64 {
    #[cfg(feature = "devtools")]
    if let Some(seed) = std::env::var("SNAP24_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
    {
        return seed;
    }
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
struct DifficultySlider;

#[derive(Component)]
struct DifficultyThumb;

#[derive(Component)]
struct DifficultyLabel;

#[derive(Component)]
struct DifficultyNextButton;

#[derive(Component)]
struct BackButton;

#[derive(Component)]
struct CardButton(usize);

#[derive(Component)]
struct OperatorButton(Op);

#[derive(Component)]
struct UndoButton;

#[derive(Component)]
struct HintButton;

#[derive(Component)]
struct RevealButton;

#[derive(Component)]
struct NewPuzzleButton;

#[derive(Component)]
struct DigitButton(u32);

#[derive(Component)]
struct RandomTargetButton;

#[derive(Component)]
struct StartButton;

#[derive(Component)]
struct ShowMoreButton;

#[derive(Component)]
struct TargetLabel;

/// Buttons get a hover/press tint from their base colour.
#[derive(Component)]
struct Hoverable {
    base: Color,
}

/// A one-shot scale pop (`from` → 1.0), optionally delayed (deal stagger).
#[derive(Component)]
struct Pop {
    delay: f32,
    elapsed: f32,
    duration: f32,
    from: f32,
}

/// A quick horizontal squish used when the cards flip face-down.
#[derive(Component)]
struct Flip {
    elapsed: f32,
}

/// Previous-frame snapshot, used to trigger one-shot effects on change.
#[derive(Resource, Default)]
struct FxPrev {
    dealt: Vec<i64>,
    cards: Vec<Rational>,
    phase: Option<Phase>,
    hidden: bool,
}

fn main() {
    let mut app = App::new();
    app.add_plugins(TabNavigationPlugin);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Snap 24".to_string(),
            ..default()
        }),
        ..default()
    }))
        .init_state::<Screen>()
        .insert_resource(Game::new())
        .init_resource::<ViewTimer>()
        .init_resource::<TargetEntry>()
        .init_resource::<FxPrev>()
        .add_systems(Startup, (load_fonts, setup).chain())
        .add_systems(OnEnter(Screen::Title), spawn_title)
        .add_systems(OnEnter(Screen::ModeSelect), spawn_mode_select)
        .add_systems(OnEnter(Screen::DifficultySelect), spawn_difficulty_select)
        .add_systems(OnEnter(Screen::TargetSelect), spawn_target_select)
        .add_systems(OnEnter(Screen::Playing), (spawn_board, update_board).chain())
        .add_systems(OnExit(Screen::Title), cleanup_screen)
        .add_systems(OnExit(Screen::ModeSelect), cleanup_screen)
        .add_systems(OnExit(Screen::DifficultySelect), cleanup_screen)
        .add_systems(OnExit(Screen::TargetSelect), cleanup_screen)
        .add_systems(OnExit(Screen::Playing), cleanup_screen)
        .add_systems(
            Update,
            (
                play_button,
                mode_buttons,
                difficulty_slider_changed,
                move_difficulty_thumb,
                difficulty_next,
                digit_buttons,
                random_target_button,
                start_button,
                update_target_display,
                back_buttons,
                hover_buttons,
                animate_pops,
                animate_flips,
            ),
        )
    .add_systems(
        Update,
        (
                tick_view,
                card_click,
                operator_click,
                undo_click,
                hint_button,
                reveal_button,
                show_more_button,
                new_puzzle,
                fx_system,
                settle_round,
            update_board.run_if(resource_changed::<Game>),
            update_countdown,
        )
            .chain()
            .run_if(in_state(Screen::Playing)),
    );

    #[cfg(feature = "devtools")]
    app.add_plugins(devtools::DevtoolsPlugin);

    app.run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Replaces Bevy's built-in default font with DejaVu Sans, which includes the
/// card-suit glyphs (♠ ♥ ♦ ♣) the default font lacks.
fn load_fonts(mut fonts: ResMut<Assets<Font>>) {
    const FONT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/DejaVuSans.ttf");
    match std::fs::read(FONT) {
        Ok(bytes) => {
            if fonts
                .insert(bevy::asset::AssetId::default(), Font::from_bytes(bytes))
                .is_err()
            {
                warn!("could not override the default font");
            }
        }
        Err(_) => warn!("bundled font not found at {FONT}; card suits may render as boxes"),
    }
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
    button_width(parent, label, marker, 180.0);
}

fn button_width<M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    marker: M,
    min_width: f32,
) -> Entity {
    parent
        .spawn((
            Button,
            marker,
            Hoverable { base: KEY },
            Node {
                padding: UiRect::axes(px(24), px(16)),
                border: UiRect::ZERO,
                border_radius: BorderRadius::MAX,
                min_width: px(min_width),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(KEY),
            BorderColor::all(KEY),
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
        })
        .id()
}

/// A framed panel that menu content sits inside.
fn panel(parent: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(16),
                padding: UiRect::all(px(40)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(28)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(BORDER),
        ))
        .with_children(build);
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
        panel(ui, |p| {
            heading(p, "SNAP 24", 72.0);
            heading(p, "Make the target from every card.", 24.0);
            button(p, "Play", PlayButton);
        });
    });
}

fn spawn_mode_select(mut commands: Commands) {
    let root = root(&mut commands);
    commands.entity(root).with_children(|ui| {
        panel(ui, |p| {
            heading(p, "Choose mode", 40.0);
            heading(p, "Classic: five cards, target 24.", 20.0);
            heading(p, "Custom: tiers change the card count and target.", 20.0);
            button(p, "Classic", ModeButton(Mode::Classic));
            button(p, "Custom", ModeButton(Mode::Custom));
            button(p, "Back", BackButton);
        });
    });
}

fn spawn_difficulty_select(mut commands: Commands, game: Res<Game>) {
    let root = root(&mut commands);
    let start = Difficulty::ALL
        .iter()
        .position(|tier| *tier == game.difficulty)
        .unwrap_or(0) as f32;
    let label = game.difficulty.label();
    commands.entity(root).with_children(|ui| {
        panel(ui, |p| {
            heading(p, "Choose difficulty", 40.0);
            p.spawn((
                DifficultyLabel,
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(28.0),
                    ..default()
                },
                TextColor(GOLD),
            ));
            p.spawn(Node {
                width: percent(100),
                height: px(36),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    DifficultySlider,
                    Hovered::default(),
                    Node {
                        width: px(420),
                        height: px(24),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Stretch,
                        ..default()
                    },
                    Slider {
                        track_click: TrackClick::Snap,
                        orientation: SliderOrientation::Horizontal,
                    },
                    SliderValue(start),
                    SliderRange::new(0.0, (Difficulty::ALL.len() - 1) as f32),
                    SliderStep(1.0),
                    TabIndex(0),
                    observe(slider_self_update),
                    Children::spawn((
                        Spawn((
                            Node {
                                height: px(8),
                                border_radius: BorderRadius::all(px(4)),
                                ..default()
                            },
                            BackgroundColor(KEY),
                        )),
                        Spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(0),
                                right: px(18),
                                top: px(0),
                                bottom: px(0),
                                ..default()
                            },
                            children![(
                                DifficultyThumb,
                                SliderThumb,
                                Node {
                                    width: px(22),
                                    height: px(22),
                                    position_type: PositionType::Absolute,
                                    left: percent(0),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(GOLD),
                            )],
                        )),
                    )),
                ));
            });
            button(p, "Next", DifficultyNextButton);
            button(p, "Back", BackButton);
        });
    });
}

fn spawn_target_select(mut commands: Commands) {
    let root = root(&mut commands);
    commands.entity(root).with_children(|ui| {
        panel(ui, |p| {
            heading(p, "Custom target", 40.0);
            p.spawn((
                TargetLabel,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(30.0),
                    ..default()
                },
                TextColor(GOLD),
            ));
            heading(p, "Type a number, or Random", 20.0);
            for row in [[7, 8, 9], [4, 5, 6], [1, 2, 3]] {
                p.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(12),
                    ..default()
                })
                .with_children(|row_ui| {
                    for digit in row {
                        button_width(row_ui, &digit.to_string(), DigitButton(digit), 72.0);
                    }
                });
            }
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(12),
                ..default()
            })
            .with_children(|row_ui| {
                button_width(row_ui, "0", DigitButton(0), 72.0);
                button(row_ui, "Random", RandomTargetButton);
            });
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(12),
                ..default()
            })
            .with_children(|row_ui| {
                button(row_ui, "Start", StartButton);
                button(row_ui, "Back", BackButton);
            });
        });
    });
}

/// Spawns the persistent board, once per entry to `Playing`. Widgets are empty
/// on spawn; [`update_board`] fills them from `Game`.
fn spawn_board(mut commands: Commands) {
    let mut target = Entity::PLACEHOLDER;
    let mut countdown = Entity::PLACEHOLDER;
    let mut countdown_pill = Entity::PLACEHOLDER;
    let mut status = Entity::PLACEHOLDER;
    let mut score = Entity::PLACEHOLDER;
    let mut message = Entity::PLACEHOLDER;
    let mut undo = Entity::PLACEHOLDER;
    let mut undo_label = Entity::PLACEHOLDER;
    let mut more = Entity::PLACEHOLDER;
    let mut cards = [Entity::PLACEHOLDER; MAX_CARDS];
    let mut ranks = [Entity::PLACEHOLDER; MAX_CARDS];
    let mut suits = [Entity::PLACEHOLDER; MAX_CARDS];
    let mut operators = [Entity::PLACEHOLDER; 4];
    let mut operator_labels = [Entity::PLACEHOLDER; 4];

    commands
        .spawn((
            ScreenRoot,
            Board,
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::all(px(24)),
                ..default()
            },
            BackgroundColor(BG),
        ))
        .with_children(|root| {
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(18),
                ..default()
            })
            .with_children(|ui| {
                target = text_entity(ui, 48.0, TEXT);
                // Timer reads as a thin outlined pill, like a game HUD chip.
                // Hidden entirely when there is no countdown (Easy, Blind, or
                // after the window has expired).
                countdown_pill = ui
                    .spawn((
                        Node {
                            border: UiRect::all(px(2)),
                            border_radius: BorderRadius::MAX,
                            padding: UiRect::axes(px(20), px(6)),
                            min_width: px(140),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BorderColor::all(GREEN),
                    ))
                    .with_children(|pill| {
                        countdown = text_entity(pill, 22.0, GREEN);
                    })
                    .id();

                ui.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(14),
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_children(|row| {
                    for index in 0..MAX_CARDS {
                        let slot = row
                            .spawn((
                                Button,
                                CardButton(index),
                                Node {
                                    width: px(108),
                                    height: px(150),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::FlexStart,
                                    align_items: AlignItems::FlexStart,
                                    padding: UiRect::axes(px(12), px(10)),
                                    row_gap: px(2),
                                    border: UiRect::all(px(2)),
                                    border_radius: BorderRadius::all(px(18)),
                                    ..default()
                                },
                                BackgroundColor(CARD),
                                BorderColor::all(BORDER),
                                UiTransform::default(),
                                BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), px(0), px(8), px(0), px(18)),
                            ))
                            .with_children(|card| {
                                ranks[index] = text_entity(card, 44.0, CARD_INK);
                                suits[index] = text_entity(card, 32.0, CARD_INK);
                            })
                            .id();
                        cards[index] = slot;
                    }
                });

                ui.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(14),
                    ..default()
                })
                .with_children(|row| {
                    for (index, operator) in Op::ALL.into_iter().enumerate() {
                        operators[index] = row
                            .spawn((
                                Button,
                                OperatorButton(operator),
                                Node {
                                    width: px(84),
                                    height: px(64),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border: UiRect::ZERO,
                                    border_radius: BorderRadius::all(px(20)),
                                    ..default()
                                },
                                BackgroundColor(KEY),
                                BorderColor::all(KEY),
                            ))
                            .with_children(|key| {
                                operator_labels[index] = text_entity(key, 30.0, TEXT);
                            })
                            .id();
                    }
                });

                status = text_entity(ui, 24.0, MUTED);
                score = text_entity(ui, 22.0, MUTED);
                message = ui
                    .spawn((
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(20.0),
                            ..default()
                        },
                        TextColor(GOLD),
                        Node {
                            max_width: percent(90),
                            ..default()
                        },
                    ))
                    .id();

                ui.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(14),
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_children(|row| {
                    button_width(row, "Hint", HintButton, 120.0);
                    button_width(row, "Reveal", RevealButton, 120.0);
                    more = button_width(row, "More", ShowMoreButton, 120.0);
                    undo = row
                        .spawn((
                            Button,
                            UndoButton,
                            Node {
                                padding: UiRect::axes(px(24), px(16)),
                                border: UiRect::ZERO,
                                border_radius: BorderRadius::MAX,
                                min_width: px(120),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(KEY),
                            BorderColor::all(KEY),
                        ))
                        .with_children(|button| {
                            undo_label = text_entity(button, 24.0, TEXT);
                        })
                        .id();
                    button(row, "New Puzzle", NewPuzzleButton);
                    button(row, "Menu", BackButton);
                });
            });
        });

    commands.insert_resource(BoardUi {
        target,
        countdown,
        countdown_pill,
        status,
        score,
        message,
        undo,
        undo_label,
        more,
        cards,
        ranks,
        suits,
        operators,
        operator_labels,
    });
}

/// Spawns an empty text node with the given size/colour and returns its entity
/// so [`update_board`] can fill and recolor it in place.
fn text_entity(parent: &mut ChildSpawnerCommands, size: f32, color: Color) -> Entity {
    parent
        .spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
            UiTransform::default(),
        ))
        .id()
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

fn difficulty_slider_changed(
    sliders: Query<&SliderValue, (Changed<SliderValue>, With<DifficultySlider>)>,
    mut game: ResMut<Game>,
    mut labels: Query<&mut Text, With<DifficultyLabel>>,
) {
    for value in &sliders {
        let index = value.0.round().clamp(0.0, (Difficulty::ALL.len() - 1) as f32) as usize;
        game.difficulty = Difficulty::ALL[index];
        let name = game.difficulty.label();
        for mut label in &mut labels {
            **label = name.to_string();
        }
    }
}

fn move_difficulty_thumb(
    sliders: Query<(&SliderValue, &SliderRange), With<DifficultySlider>>,
    mut thumbs: Query<&mut Node, With<DifficultyThumb>>,
) {
    for (value, range) in &sliders {
        let percent = range.thumb_position(value.0) * 100.0;
        for mut thumb in &mut thumbs {
            thumb.left = bevy::ui::Val::Percent(percent);
        }
    }
}

fn difficulty_next(
    interactions: Query<&Interaction, (Changed<Interaction>, With<DifficultyNextButton>)>,
    mut game: ResMut<Game>,
    mut timer: ResMut<ViewTimer>,
    entry: Res<TargetEntry>,
    time: Res<Time>,
    mut next: ResMut<NextState<Screen>>,
) {
    if !pressed(&interactions) {
        return;
    }
    if game.mode == Mode::Custom {
        next.set(Screen::TargetSelect);
    } else {
        begin_game(&mut game, &mut timer, time.elapsed_secs(), &entry);
        next.set(Screen::Playing);
    }
}

fn digit_buttons(
    interactions: Query<(&Interaction, &DigitButton), Changed<Interaction>>,
    mut entry: ResMut<TargetEntry>,
) {
    for (interaction, digit) in &interactions {
        if *interaction == Interaction::Pressed {
            entry.push(digit.0);
        }
    }
}

fn random_target_button(
    interactions: Query<&Interaction, (Changed<Interaction>, With<RandomTargetButton>)>,
    mut entry: ResMut<TargetEntry>,
) {
    if pressed(&interactions) {
        entry.clear();
    }
}

fn start_button(
    interactions: Query<&Interaction, (Changed<Interaction>, With<StartButton>)>,
    mut game: ResMut<Game>,
    mut timer: ResMut<ViewTimer>,
    entry: Res<TargetEntry>,
    time: Res<Time>,
    mut next: ResMut<NextState<Screen>>,
) {
    if pressed(&interactions) {
        begin_game(&mut game, &mut timer, time.elapsed_secs(), &entry);
        next.set(Screen::Playing);
    }
}

fn update_target_display(entry: Res<TargetEntry>, mut labels: Query<&mut Text, With<TargetLabel>>) {
    for mut label in &mut labels {
        **label = entry.label();
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
        Screen::TargetSelect => Screen::DifficultySelect,
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
// feel: hover feedback and one-shot animations                                 //
// --------------------------------------------------------------------------- //

fn lighten(color: Color, amount: f32) -> Color {
    color.mix(&Color::WHITE, amount)
}

fn darken(color: Color, amount: f32) -> Color {
    color.mix(&Color::BLACK, amount)
}

fn hover_buttons(
    mut query: Query<(&Interaction, &Hoverable, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (interaction, hoverable, mut color) in &mut query {
        *color = BackgroundColor(match interaction {
            Interaction::Hovered => lighten(hoverable.base, 0.12),
            Interaction::Pressed => darken(hoverable.base, 0.14),
            Interaction::None => hoverable.base,
        });
    }
}

fn animate_pops(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut UiTransform, &mut Pop)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut pop) in &mut query {
        pop.elapsed += dt;
        if pop.elapsed < pop.delay {
            transform.scale = Vec2::splat(pop.from);
            continue;
        }
        let t = ((pop.elapsed - pop.delay) / pop.duration).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        transform.scale = Vec2::splat(pop.from + (1.0 - pop.from) * eased);
        if t >= 1.0 {
            transform.scale = Vec2::ONE;
            commands.entity(entity).remove::<Pop>();
        }
    }
}

fn animate_flips(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut UiTransform, &mut Flip)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut flip) in &mut query {
        flip.elapsed += dt;
        let t = (flip.elapsed / 0.35).clamp(0.0, 1.0);
        // Narrow to a sliver at the midpoint, then back: reads as a card flip.
        let x = (t * std::f32::consts::PI).cos().abs().max(0.05);
        transform.scale = Vec2::new(x, 1.0);
        if t >= 1.0 {
            transform.scale = Vec2::ONE;
            commands.entity(entity).remove::<Flip>();
        }
    }
}

/// Watches `Game` for deal / merge / flip / result and fires the matching
/// one-shot animation.
fn fx_system(
    game: Res<Game>,
    ui: Option<Res<BoardUi>>,
    mut commands: Commands,
    mut prev: ResMut<FxPrev>,
) {
    let Some(ui) = ui else {
        return;
    };
    // `prev` starts empty, so the first dealt hand also animates in.
    if prev.dealt != game.dealt {
        for (index, &slot) in ui.cards.iter().enumerate() {
            commands.entity(slot).insert(Pop {
                delay: index as f32 * 0.06,
                elapsed: 0.0,
                duration: 0.4,
                from: 0.5,
            });
        }
        prev.dealt = game.dealt.clone();
    } else if game.round.cards.len() < prev.cards.len() {
        // The merged result sits where the first change appears.
        let changed = (0..game.round.cards.len())
            .find(|&i| game.round.cards.get(i) != prev.cards.get(i))
            .and_then(|index| ui.cards.get(index).copied());
        if let Some(slot) = changed {
            commands.entity(slot).insert(Pop {
                delay: 0.0,
                elapsed: 0.0,
                duration: 0.22,
                from: 1.25,
            });
        }
    }
    prev.cards = game.round.cards.clone();

    let hidden = !game.round.revealed.is_empty() && game.round.revealed.iter().all(|r| !*r);
    if hidden && !prev.hidden {
        for &slot in ui.cards.iter() {
            commands.entity(slot).insert(Flip { elapsed: 0.0 });
        }
    }
    prev.hidden = hidden;

    if prev.phase != Some(game.round.phase) {
        if matches!(game.round.phase, Phase::Won | Phase::Lost) {
            commands.entity(ui.status).insert(Pop {
                delay: 0.0,
                elapsed: 0.0,
                duration: 0.35,
                from: 1.5,
            });
        }
        prev.phase = Some(game.round.phase);
    }
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
            game.play_card(card.0);
        }
    }
}

fn operator_click(
    mut game: ResMut<Game>,
    interactions: Query<(&Interaction, &OperatorButton), Changed<Interaction>>,
) {
    for (interaction, operator) in &interactions {
        if *interaction == Interaction::Pressed {
            game.play_op(operator.0);
        }
    }
}

fn undo_click(
    mut game: ResMut<Game>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<UndoButton>)>,
) {
    if pressed(&interactions) {
        game.undo();
    }
}

/// Progressive hint text for the current board and hint level.
fn hint_text(game: &Game) -> String {
    let board = &game.round.cards;
    let target = game.round.target;
    let Some(step) = first_move(board, target) else {
        return "No solution from here.".to_string();
    };
    match game.hint_level {
        1 => format!("Combine {} and {}.", step.left, step.right),
        2 => format!("Use '{}' on {} and {}.", step.op, step.left, step.right),
        3 => format!("{} {} {} = {}", step.left, step.op, step.right, step.result),
        _ => {
            let steps: Vec<String> = move_sequence(board, target)
                .iter()
                .map(|m| format!("{} {} {} = {}", m.left, m.op, m.right, m.result))
                .collect();
            format!("Solution: {}", steps.join(" ; "))
        }
    }
}

/// All solutions for the original puzzle as step sequences, after checking each
/// one back through the evaluator.
fn reveal_solutions(game: &Game) -> Vec<Vec<Move>> {
    let cards: Vec<Rational> = game.dealt.iter().copied().map(Rational::from).collect();
    let target = game.round.target;
    let infix = solutions_infix(&cards, target);
    let steps = solutions_steps(&cards, target);
    steps
        .into_iter()
        .zip(infix)
        .filter(|(_, infix)| evaluate(&game.dealt, infix).map(|value| value == target) == Ok(true))
        .map(|(steps, _)| steps)
        .collect()
}

fn format_solution(solutions: &[Vec<Move>], shown: usize) -> String {
    if solutions.is_empty() {
        return "No solutions to reveal.".to_string();
    }
    let index = shown.clamp(1, solutions.len());
    let body = solutions[index - 1]
        .iter()
        .map(|step| format!("{} {} {} = {}", step.left, step.op, step.right, step.result))
        .collect::<Vec<_>>()
        .join("   ");
    format!("Solution {index}/{}:   {body}", solutions.len())
}

fn hint_button(
    mut game: ResMut<Game>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<HintButton>)>,
) {
    if !pressed(&interactions) || game.round.phase != Phase::Playing {
        return;
    }
    game.hint_level = (game.hint_level + 1).min(4);
    game.hints_used += 1;
    game.message = hint_text(&game);
}

fn reveal_button(
    mut game: ResMut<Game>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<RevealButton>)>,
) {
    if !pressed(&interactions) {
        return;
    }
    let solutions = reveal_solutions(&game);
    game.reveal_total = solutions.len();
    game.reveal_shown = usize::from(!solutions.is_empty());
    game.message = format_solution(&solutions, game.reveal_shown);
}

fn show_more_button(
    mut game: ResMut<Game>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<ShowMoreButton>)>,
) {
    if !pressed(&interactions) || game.reveal_shown >= game.reveal_total {
        return;
    }
    game.reveal_shown += 1;
    let solutions = reveal_solutions(&game);
    game.message = format_solution(&solutions, game.reveal_shown);
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

fn update_board(
    game: Res<Game>,
    ui: Option<Res<BoardUi>>,
    mut texts: Query<&mut Text>,
    mut text_colors: Query<&mut TextColor>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut borders: Query<&mut BorderColor>,
    mut nodes: Query<&mut Node>,
) {
    let Some(ui) = ui else {
        return;
    };
    set_text(
        &mut texts,
        ui.target,
        format!(
            "Target: {}   ({} · {})",
            game.round.target,
            game.mode.label(),
            game.difficulty.label()
        ),
    );
    set_text(&mut texts, ui.status, game.status());
    if let Ok(mut color) = text_colors.get_mut(ui.status) {
        *color = TextColor(match game.round.phase {
            Phase::Won => WIN,
            Phase::Lost => LOSE,
            Phase::Playing => MUTED,
        });
    }
    set_text(&mut texts, ui.message, game.message.clone());
    set_text(
        &mut texts,
        ui.score,
        match game.last_score {
            Some(value) => format!("Round score: +{value}    Total: {}", game.total_score),
            None => format!("Score: {}", game.total_score),
        },
    );

    let labels = game.round.card_labels();
    for (index, &slot) in ui.cards.iter().enumerate() {
        if index < labels.len() {
            // `Display::None` removes the slot from layout, so the remaining
            // cards stay centred as the hand shrinks.
            set_display(&mut nodes, slot, Display::Flex);
            let revealed = game.round.is_revealed(index);
            let selected = game.round.is_first(index);
            let suit = game.round.suit(index);

            // Face: a poker card (rank + suit), or a gold value token when the
            // entry is a merged result rather than a dealt card.
            let (rank, suit_glyph, ink, background) = if !revealed {
                ("?".to_string(), String::new(), MUTED, CARD_BACK)
            } else if selected {
                (labels[index].clone(), suit.map(Suit::glyph).unwrap_or("●").to_string(), CARD_INK, CARD_SELECTED)
            } else {
                match suit {
                    Some(suit) => (
                        labels[index].clone(),
                        suit.glyph().to_string(),
                        if suit.is_red() { CARD_RED } else { CARD_INK },
                        CARD,
                    ),
                    None => (labels[index].clone(), "●".to_string(), GOLD, TOKEN),
                }
            };

            set_text(&mut texts, ui.ranks[index], rank);
            set_text(&mut texts, ui.suits[index], suit_glyph);
            if let Ok(mut color) = text_colors.get_mut(ui.ranks[index]) {
                *color = TextColor(ink);
            }
            if let Ok(mut color) = text_colors.get_mut(ui.suits[index]) {
                *color = TextColor(ink);
            }
            if let Ok(mut color) = backgrounds.get_mut(slot) {
                *color = BackgroundColor(background);
            }
            if let Ok(mut border) = borders.get_mut(slot) {
                *border = BorderColor::all(if selected { GOLD } else { BORDER });
            }
        } else {
            set_display(&mut nodes, slot, Display::None);
        }
    }

    for (operator, (&slot, &label)) in Op::ALL
        .iter()
        .zip(ui.operators.iter().zip(ui.operator_labels.iter()))
    {
        let pending = game.round.op == Some(*operator);
        if let Ok(mut color) = backgrounds.get_mut(slot) {
            *color = BackgroundColor(if pending { GOLD } else { KEY });
        }
        set_text(&mut texts, label, operator.symbol().to_string());
        if let Ok(mut color) = text_colors.get_mut(label) {
            *color = TextColor(if pending { CARD_INK } else { TEXT });
        }
    }

    let undo_enabled = game.round.can_undo();
    if let Ok(mut color) = backgrounds.get_mut(ui.undo) {
        *color = BackgroundColor(if undo_enabled { KEY } else { BG });
    }
    set_text(&mut texts, ui.undo_label, "Undo".to_string());
    if let Ok(mut color) = text_colors.get_mut(ui.undo_label) {
        *color = TextColor(if undo_enabled { TEXT } else { MUTED });
    }

    // "More" appears only when a reveal has additional solutions waiting.
    let more_display = if game.reveal_shown > 0 && game.reveal_shown < game.reveal_total {
        Display::Flex
    } else {
        Display::None
    };
    if let Ok(mut node) = nodes.get_mut(ui.more)
        && node.display != more_display
    {
        node.display = more_display;
    }
}

fn update_countdown(
    timer: Res<ViewTimer>,
    ui: Res<BoardUi>,
    mut texts: Query<&mut Text>,
    mut nodes: Query<&mut Node>,
) {
    let text = match timer.phase.seconds_left() {
        Some(seconds) => format!("Hiding in {seconds}s"),
        None => String::new(),
    };
    set_text(&mut texts, ui.countdown, text);

    // Only show the pill while a countdown is actually running.
    let display = if timer.phase.seconds_left().is_some() {
        Display::Flex
    } else {
        Display::None
    };
    if let Ok(mut node) = nodes.get_mut(ui.countdown_pill)
        && node.display != display
    {
        node.display = display;
    }
}

fn set_text(texts: &mut Query<&mut Text>, entity: Entity, value: String) {
    if let Ok(mut text) = texts.get_mut(entity) {
        **text = value;
    }
}

fn set_display(nodes: &mut Query<&mut Node>, entity: Entity, value: Display) {
    if let Ok(mut node) = nodes.get_mut(entity) {
        node.display = value;
    }
}

#[cfg(test)]
mod game_tests {
    use super::*;

    #[test]
    fn starting_a_new_game_resets_the_running_total() {
        let mut game = Game::new();
        let mut timer = ViewTimer::default();
        game.total_score = 123;
        begin_game(&mut game, &mut timer, 0.0, &TargetEntry::default());
        assert_eq!(game.total_score, 0);
    }

    #[test]
    fn new_puzzle_keeps_the_running_total() {
        let mut game = Game::new();
        let mut timer = ViewTimer::default();
        game.total_score = 123;
        deal(&mut game, &mut timer, 0.0);
        assert_eq!(game.total_score, 123);
    }
}
