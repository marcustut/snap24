//! Snap 24 desktop app — the playable core loop.
//!
//! Bevy owns rendering and input; [`logic::Round`] owns the rules. The board is
//! rebuilt from the round whenever the [`Game`] resource changes.

mod logic;

use bevy::prelude::*;
use logic::{MergeError, Op, Phase, Round};
use snap24_core::{generate, Difficulty, Mode, Rng};
use std::time::{SystemTime, UNIX_EPOCH};

const BG: Color = Color::srgb(0.07, 0.08, 0.11);
const CARD: Color = Color::srgb(0.16, 0.18, 0.24);
const CARD_SELECTED: Color = Color::srgb(0.20, 0.55, 0.32);
const KEY: Color = Color::srgb(0.20, 0.23, 0.30);
const BORDER: Color = Color::srgb(0.30, 0.34, 0.42);
const TEXT: Color = Color::srgb(0.93, 0.95, 0.98);
const MUTED: Color = Color::srgb(0.72, 0.76, 0.84);

#[derive(Resource)]
struct Game {
    round: Round,
    rng: Rng,
    mode: Mode,
    difficulty: Difficulty,
    error: Option<String>,
}

impl Game {
    fn deal(&mut self) {
        let puzzle = generate(self.mode, self.difficulty, &mut self.rng);
        self.round = Round::new(puzzle.cards, puzzle.target);
        self.error = None;
    }

    fn new() -> Self {
        let mut game = Game {
            round: Round::new(Vec::new(), 0i64.into()),
            rng: Rng::new(seed_from_time()),
            mode: Mode::Classic,
            difficulty: Difficulty::Easy,
            error: None,
        };
        game.deal();
        game
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

fn seed_from_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED)
}

#[derive(Component)]
struct Board;

#[derive(Component)]
struct CardButton(usize);

#[derive(Component)]
struct OperatorButton(Op);

#[derive(Component)]
struct UndoButton;

#[derive(Component)]
struct NewPuzzleButton;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Snap 24".to_string(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(Game::new())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                card_click,
                operator_click,
                undo_click,
                new_puzzle,
                rebuild_board.run_if(resource_changed::<Game>),
            )
                .chain(),
        )
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
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
    for interaction in &interactions {
        if *interaction == Interaction::Pressed {
            game.round.undo();
            game.error = None;
        }
    }
}

fn new_puzzle(
    mut game: ResMut<Game>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<NewPuzzleButton>)>,
) {
    for interaction in &interactions {
        if *interaction == Interaction::Pressed {
            game.deal();
        }
    }
}

fn rebuild_board(mut commands: Commands, game: Res<Game>, existing: Query<Entity, With<Board>>) {
    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let root = commands
        .spawn((
            Board,
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
        .id();

    commands.entity(root).with_children(|ui| {
        ui.spawn((
            Text::new(format!("Target: {}", game.round.target)),
            TextFont {
                font_size: FontSize::Px(44.0),
                ..default()
            },
            TextColor(TEXT),
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
                        Text::new(label.clone()),
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

        let undo_enabled = game.round.can_undo();
        ui.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(14),
            ..default()
        })
        .with_children(|row| {
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

            row.spawn((
                Button,
                NewPuzzleButton,
                Node {
                    padding: UiRect::axes(px(24), px(12)),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::all(px(12)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.20, 0.34, 0.62)),
                BorderColor::all(TEXT),
            ))
            .with_children(|button| {
                button.spawn((
                    Text::new("New Puzzle"),
                    TextFont {
                        font_size: FontSize::Px(24.0),
                        ..default()
                    },
                    TextColor(TEXT),
                ));
            });
        });
    });
}
