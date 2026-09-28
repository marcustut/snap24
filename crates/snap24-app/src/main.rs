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

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
use bevy::ecs::system::NonSendMarker;
use bevy::color::Mix;
use bevy::input_focus::tab_navigation::{TabIndex, TabNavigationPlugin};
use bevy::ui::{
    BackgroundGradient, ColorStop, Gradient, RadialGradient, RadialGradientShape, UiPosition,
};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{
    observe, slider_self_update, Slider, SliderOrientation, SliderPrecision, SliderRange,
    SliderStep, SliderThumb, SliderValue, TrackClick,
};
use logic::{round_score, MergeError, Op, Phase, Round, ViewPhase};
use snap24_core::{
    evaluate, first_move, generate, generate_targeted, move_sequence, solutions_infix,
    solutions_steps, Difficulty, Mode, Move, Puzzle, Rational, Rng,
};
use std::time::{SystemTime, UNIX_EPOCH};

// Direction A · Parlour — warm near-black room, ivory cards, brass accent.
const BG: Color = Color::srgb(0.035, 0.027, 0.023);
const PANEL: Color = Color::srgb(0.078, 0.061, 0.047);
const CARD: Color = Color::srgb(0.953, 0.925, 0.878);
const CARD_INK: Color = Color::srgb(0.102, 0.078, 0.059);
const CARD_RED: Color = Color::srgb(0.698, 0.227, 0.180);
const CARD_BACK: Color = Color::srgb(0.086, 0.067, 0.051);
const TOKEN: Color = Color::srgb(0.078, 0.063, 0.047);
const KEY: Color = Color::srgb(0.086, 0.067, 0.051);
const BORDER: Color = Color::srgb(0.24, 0.20, 0.16);
const TEXT: Color = Color::srgb(0.937, 0.906, 0.855);
const MUTED: Color = Color::srgb(0.55, 0.50, 0.44);
const GOLD: Color = Color::srgb(0.788, 0.635, 0.290);
const WIN: Color = Color::srgb(0.788, 0.635, 0.290);
const LOSE: Color = Color::srgb(0.698, 0.227, 0.180);

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

    /// Value shown next to the static "TARGET" label.
    fn display(&self) -> String {
        match self.value() {
            Some(value) => value.to_string(),
            None => "Random".to_string(),
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
    /// Session-wide sound mute.
    muted: bool,
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
            muted: false,
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
    mode_chip: Entity,
    mute_label: Entity,
    countdown: Entity,
    countdown_pill: Entity,
    status: Entity,
    score: Entity,
    message: Entity,
    undo: Entity,
    undo_label: Entity,
    more: Entity,
    give_up: Entity,
    next: Entity,
    cards: [Entity; MAX_CARDS],
    ranks: [Entity; MAX_CARDS],
    suits: [Entity; MAX_CARDS],
    fracs: [Entity; MAX_CARDS],
    nums: [Entity; MAX_CARDS],
    dens: [Entity; MAX_CARDS],
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
struct DifficultyMeta;

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

/// Ends the current round and deals a new puzzle (shown mid-round as "Give up").
#[derive(Component)]
struct NewPuzzleButton;

/// Deals the next puzzle once a round is over.
#[derive(Component)]
struct NextPuzzleButton;

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

#[derive(Component)]
struct MuteButton;

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
            // On iOS the default desktop window size (1280×720 points) is wider
            // than the screen, so the UI lands off-screen; go fullscreen.
            #[cfg(target_os = "ios")]
            mode: bevy::window::WindowMode::BorderlessFullscreen(
                bevy::window::MonitorSelection::Primary,
            ),
            #[cfg(target_os = "ios")]
            resizable: false,
            ..default()
        }),
        ..default()
    }))
        .init_state::<Screen>()
        .insert_resource(Game::new())
        .init_resource::<ViewTimer>()
        .init_resource::<TargetEntry>()
        .init_resource::<FxPrev>()
        .init_resource::<Fonts>()
        .add_systems(Startup, setup)
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
                mute_click,
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

    // On iOS/Android winit should drive the loop event-driven rather than
    // free-running; without this the app renders one frame and stalls.
    #[cfg(any(target_os = "ios", target_os = "android"))]
    app.insert_resource(bevy::winit::WinitSettings::mobile());

    // Phones need the desktop-sized UI scaled down to fit.
    #[cfg(target_os = "ios")]
    app.add_systems(Update, fit_ui);

    #[cfg(feature = "devtools")]
    app.add_plugins(devtools::DevtoolsPlugin);

    install_fonts(&mut app);
    install_audio(&mut app);

    app.run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Type for direction A: Fraunces (display), Space Grotesk (body, the global
/// default), DejaVu Sans (only for the ♠♥♦♣ suit glyphs, which the other two
/// lack).
#[derive(Resource, Default)]
struct Fonts {
    display: Handle<Font>,
    suit: Handle<Font>,
}

/// Haptics, no-op off iOS. Fired on the main thread only.
mod haptics {
    #[cfg(target_os = "ios")]
    mod imp {
        use objc2::MainThreadMarker;
        use objc2_ui_kit::{UIImpactFeedbackGenerator, UINotificationFeedbackGenerator, UINotificationFeedbackType};

        pub fn impact() {
            if let Some(mtm) = MainThreadMarker::new() {
                let generator = unsafe { UIImpactFeedbackGenerator::new(mtm) };
                unsafe { generator.impactOccurred() };
            }
        }

        pub fn success() {
            if let Some(mtm) = MainThreadMarker::new() {
                let generator = unsafe { UINotificationFeedbackGenerator::new(mtm) };
                unsafe { generator.notificationOccurred(UINotificationFeedbackType::Success) };
            }
        }
    }

    #[cfg(not(target_os = "ios"))]
    mod imp {
        pub fn impact() {}
        pub fn success() {}
    }

    pub use imp::{impact, success};
}

/// The four generated UI sounds, embedded in the binary.
#[derive(Resource, Default)]
struct Sfx {
    deal: Handle<AudioSource>,
    merge: Handle<AudioSource>,
    win: Handle<AudioSource>,
    lose: Handle<AudioSource>,
}

fn install_audio(app: &mut App) {
    let (deal, merge, win, lose) = {
        let mut assets = app.world_mut().resource_mut::<Assets<AudioSource>>();
        let mut add = |bytes: &[u8]| {
            assets.add(AudioSource {
                bytes: bytes.to_vec().into(),
            })
        };
        (
            add(include_bytes!("../assets/sfx/deal.wav")),
            add(include_bytes!("../assets/sfx/merge.wav")),
            add(include_bytes!("../assets/sfx/win.wav")),
            add(include_bytes!("../assets/sfx/lose.wav")),
        )
    };
    app.world_mut().insert_resource(Sfx {
        deal,
        merge,
        win,
        lose,
    });
}

/// Fire-and-forget: the entity despawns when playback finishes.
fn play(commands: &mut Commands, handle: &Handle<AudioSource>) {
    commands.spawn((AudioPlayer::new(handle.clone()), PlaybackSettings::DESPAWN));
}

/// Installs the three fonts at build time, before any system runs, so the very
/// first `OnEnter` already has real handles.
fn install_fonts(app: &mut App) {
    let (display, suit) = {
        let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
        let display = fonts.add(Font::from_bytes(
            include_bytes!("../assets/fonts/Fraunces-Black.ttf").to_vec(),
        ));
        let suit = fonts.add(Font::from_bytes(
            include_bytes!("../assets/fonts/DejaVuSans.ttf").to_vec(),
        ));
        // Space Grotesk becomes the default body font.
        let _ = fonts.insert(
            bevy::asset::AssetId::default(),
            Font::from_bytes(include_bytes!("../assets/fonts/SpaceGrotesk-Regular.ttf").to_vec()),
        );
        (display, suit)
    };
    app.world_mut().insert_resource(Fonts { display, suit });
}

// --------------------------------------------------------------------------- //
// menu screens                                                                 //
// --------------------------------------------------------------------------- //

fn cleanup_screen(mut commands: Commands, roots: Query<Entity, With<ScreenRoot>>) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}

/// Shared chrome for the menu screens: the vignette background, the two-tone
/// wordmark top-left, a tracked context label top-right, and centred content.
fn menu_screen(
    commands: &mut Commands,
    display: &Handle<Font>,
    context: &str,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    commands
        .spawn((
            ScreenRoot,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: screen_padding(),
                ..default()
            },
            BackgroundColor(BG),
            vignette(),
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: percent(100),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(6), px(0)),
                ..default()
            })
            .with_children(|bar| {
                bar.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                    ..default()
                })
                .with_children(|mark| {
                    text_static(mark, "SNAP", 26.0, TEXT, display);
                    text_static(mark, "24", 26.0, GOLD, display);
                });
                text_body(bar, &tracked(context), 13.0, MUTED);
            });

            root.spawn(Node {
                width: percent(100),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(22),
                ..default()
            })
            .with_children(build);
        });
}

/// The brass primary action (Play / Next / Start).
fn primary_button(parent: &mut ChildSpawnerCommands, label: &str, marker: impl Bundle) -> Entity {
    parent
        .spawn((
            Button,
            marker,
            Hoverable { base: GOLD },
            Node {
                padding: UiRect::axes(px(36), px(16)),
                border_radius: BorderRadius::MAX,
                min_width: px(220),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(GOLD),
            BorderColor::all(GOLD),
        ))
        .with_children(|button| {
            text_body(button, label, 20.0, CARD_INK);
        })
        .id()
}

/// A selectable mode option: display title + body description in a hairline card.
fn mode_card(
    parent: &mut ChildSpawnerCommands,
    mode: Mode,
    title: &str,
    description: &str,
    display: &Handle<Font>,
) {
    parent
        .spawn((
            Button,
            ModeButton(mode),
            Hoverable { base: PANEL },
            Node {
                width: px(440),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: px(8),
                padding: UiRect::all(px(24)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(18)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(BORDER),
        ))
        .with_children(|card| {
            text_static(card, title, 30.0, TEXT, display);
            text_body(card, description, 16.0, MUTED);
        });
}

/// A square hairline key (target keypad digits).
fn key_button(parent: &mut ChildSpawnerCommands, label: &str, marker: impl Bundle) {
    parent
        .spawn((
            Button,
            marker,
            Hoverable { base: KEY },
            Node {
                width: px(78),
                height: px(64),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(14)),
                ..default()
            },
            BackgroundColor(BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|button| {
            text_body(button, label, 24.0, TEXT);
        });
}

fn heading(parent: &mut ChildSpawnerCommands, text: &str, size: f32, font: &Handle<Font>) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(TEXT),
    ));
}

/// Screen padding. On iOS the extra top/bottom keeps content clear of the notch
/// and home indicator (Bevy exposes no safe-area insets), sized in design units
/// so it survives the UI scale applied by `fit_ui`.
fn screen_padding() -> UiRect {
    #[cfg(target_os = "ios")]
    {
        UiRect {
            left: px(24),
            right: px(24),
            top: px(110),
            bottom: px(70),
        }
    }
    #[cfg(not(target_os = "ios"))]
    {
        UiRect::all(px(28))
    }
}

/// On phones the desktop fixed pixel sizes are far too large, so scale the
/// whole UI down against a phone-appropriate design width.
#[cfg(target_os = "ios")]
fn fit_ui(windows: Query<&Window>, mut scale: ResMut<UiScale>) {
    const DESIGN_WIDTH: f32 = 720.0;
    const DESIGN_HEIGHT: f32 = 1100.0;
    let Ok(window) = windows.single() else {
        return;
    };
    // Fit both axes so landscape (short height) doesn't overflow either.
    let target = (window.width() / DESIGN_WIDTH)
        .min(window.height() / DESIGN_HEIGHT)
        .clamp(0.35, 1.2);
    if (scale.0 - target).abs() > 0.001 {
        scale.0 = target;
    }
}

/// A subtle warm radial glow from the top, fading to the base colour.
fn vignette() -> BackgroundGradient {
    BackgroundGradient::from(Gradient::Radial(RadialGradient::new(
        UiPosition::TOP,
        RadialGradientShape::FarthestCorner,
        vec![
            ColorStop::new(Color::srgb(0.105, 0.078, 0.058), percent(0.0)),
            ColorStop::new(BG, percent(62.0)),
        ],
    )))
}

/// Wide-tracked uppercase, approximating the mockup's letterspacing (Bevy text
/// has no letter-spacing property).
fn tracked(text: &str) -> String {
    text.to_uppercase()
        .chars()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A static body-font text node (the default font is Space Grotesk).
fn text_body(parent: &mut ChildSpawnerCommands, text: &str, size: f32, color: Color) -> Entity {
    parent
        .spawn((
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
        ))
        .id()
}

/// A borderless text control with a thin underline (the play-screen style).
fn ghost_button(parent: &mut ChildSpawnerCommands, label: &str, marker: impl Bundle) -> Entity {
    let mut label_entity = Entity::PLACEHOLDER;
    ghost_button_id(parent, label, marker, &mut label_entity)
}

fn ghost_button_id(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    marker: impl Bundle,
    label_out: &mut Entity,
) -> Entity {
    parent
        .spawn((
            Button,
            marker,
            Node {
                padding: UiRect::axes(px(8), px(4)),
                border: UiRect {
                    bottom: px(1),
                    ..default()
                },
                ..default()
            },
            BackgroundColor(BG),
            BorderColor::all(MUTED),
        ))
        .with_children(|button| {
            *label_out = text_body(button, label, 16.0, TEXT);
        })
        .id()
}

/// A static text node in a specific font.
fn text_static(
    parent: &mut ChildSpawnerCommands,
    text: &str,
    size: f32,
    color: Color,
    font: &Handle<Font>,
) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    ));
}

/// A text node in a specific font (used for Fraunces display and DejaVu suits).
fn text_with(
    parent: &mut ChildSpawnerCommands,
    size: f32,
    color: Color,
    font: &Handle<Font>,
) -> Entity {
    parent
        .spawn((
            Text::new(""),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
            UiTransform::default(),
        ))
        .id()
}

fn spawn_title(mut commands: Commands, fonts: Res<Fonts>) {
    let display = fonts.display.clone();
    menu_screen(&mut commands, &display, "A card puzzle", |ui| {
        ui.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(14),
            ..default()
        })
        .with_children(|hero| {
            text_static(hero, "SNAP", 96.0, TEXT, &display);
            text_static(hero, "24", 96.0, GOLD, &display);
        });
        text_body(ui, "Make the target from every card.", 20.0, MUTED);
        primary_button(ui, "Play", PlayButton);
    });
}

fn spawn_mode_select(mut commands: Commands, fonts: Res<Fonts>) {
    let display = fonts.display.clone();
    menu_screen(&mut commands, &display, "Mode", |ui| {
        heading(ui, "Choose mode", 40.0, &display);
        mode_card(
            ui,
            Mode::Classic,
            "Classic",
            "Five cards. Make 24.",
            &display,
        );
        mode_card(
            ui,
            Mode::Custom,
            "Custom",
            "Tiers change the card count and the target.",
            &display,
        );
        ghost_button(ui, "Back", BackButton);
    });
}

fn spawn_difficulty_select(mut commands: Commands, game: Res<Game>, fonts: Res<Fonts>) {
    let display = fonts.display.clone();
    let start = Difficulty::ALL
        .iter()
        .position(|tier| *tier == game.difficulty)
        .unwrap_or(0) as f32;
    menu_screen(&mut commands, &display, "Difficulty", |ui| {
        heading(ui, "Choose difficulty", 40.0, &display);
        text_body(ui, tier_note(game.mode), 16.0, MUTED);

        // The tier reads big; its real parameters sit under it as tracked caps.
        ui.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            ..default()
        })
        .with_children(|tier| {
            tier.spawn((
                DifficultyLabel,
                Text::new(game.difficulty.label()),
                TextFont {
                    font: display.clone().into(),
                    font_size: FontSize::Px(46.0),
                    ..default()
                },
                TextColor(TEXT),
            ));
            tier.spawn((
                DifficultyMeta,
                Text::new(tracked(&tier_meta(game.difficulty))),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(MUTED),
            ));
        });

        ui.spawn(Node {
            width: px(460),
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
                    width: px(440),
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
                // Round to whole tiers while dragging, so the thumb snaps in.
                SliderPrecision(0),
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

        primary_button(ui, "Next", DifficultyNextButton);
        ghost_button(ui, "Back", BackButton);
    });
}

fn spawn_target_select(mut commands: Commands, fonts: Res<Fonts>) {
    let display = fonts.display.clone();
    menu_screen(&mut commands, &display, "Target", |ui| {
        heading(ui, "Custom target", 40.0, &display);

        ui.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Baseline,
            column_gap: px(16),
            ..default()
        })
        .with_children(|row| {
            text_body(row, "TARGET", 15.0, MUTED);
            row.spawn((
                TargetLabel,
                Text::new(""),
                TextFont {
                    font: display.clone().into(),
                    font_size: FontSize::Px(56.0),
                    ..default()
                },
                TextColor(TEXT),
            ));
        });

        for row in [[7, 8, 9], [4, 5, 6], [1, 2, 3]] {
            ui.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(12),
                ..default()
            })
            .with_children(|digits| {
                for digit in row {
                    key_button(digits, &digit.to_string(), DigitButton(digit));
                }
            });
        }
        ui.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(12),
            ..default()
        })
        .with_children(|row_ui| {
            key_button(row_ui, "0", DigitButton(0));
            ghost_button(row_ui, "Random", RandomTargetButton);
        });

        primary_button(ui, "Start", StartButton);
        ghost_button(ui, "Back", BackButton);
    });
}

/// Spawns the persistent board, once per entry to `Playing`. Widgets are empty
/// on spawn; [`update_board`] fills them from `Game`.
fn spawn_board(mut commands: Commands, fonts: Res<Fonts>) {
    let display = fonts.display.clone();
    let suit_font = fonts.suit.clone();
    let mut target = Entity::PLACEHOLDER;
    let mut mode_chip = Entity::PLACEHOLDER;
    let mut mute_label = Entity::PLACEHOLDER;
    let mut countdown = Entity::PLACEHOLDER;
    let mut countdown_pill = Entity::PLACEHOLDER;
    let mut status = Entity::PLACEHOLDER;
    let mut score = Entity::PLACEHOLDER;
    let mut message = Entity::PLACEHOLDER;
    let mut undo = Entity::PLACEHOLDER;
    let mut undo_label = Entity::PLACEHOLDER;
    let mut more = Entity::PLACEHOLDER;
    let mut give_up = Entity::PLACEHOLDER;
    let mut next = Entity::PLACEHOLDER;
    let mut cards = [Entity::PLACEHOLDER; MAX_CARDS];
    let mut ranks = [Entity::PLACEHOLDER; MAX_CARDS];
    let mut suits = [Entity::PLACEHOLDER; MAX_CARDS];
    let mut fracs = [Entity::PLACEHOLDER; MAX_CARDS];
    let mut nums = [Entity::PLACEHOLDER; MAX_CARDS];
    let mut dens = [Entity::PLACEHOLDER; MAX_CARDS];
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
                padding: screen_padding(),
                ..default()
            },
            BackgroundColor(BG),
            vignette(),
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(18),
                ..default()
            })
            .with_children(|ui| {
                // Top bar: two-tone wordmark left, tracked mode · tier right.
                ui.spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(6), px(0)),
                    ..default()
                })
                .with_children(|bar| {
                    bar.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6),
                        ..default()
                    })
                    .with_children(|mark| {
                        text_static(mark, "SNAP", 28.0, TEXT, &display);
                        text_static(mark, "24", 28.0, GOLD, &display);
                    });
                    bar.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(22),
                        ..default()
                    })
                    .with_children(|right| {
                        mode_chip = text_entity(right, 13.0, MUTED);
                        let mut mute_text = Entity::PLACEHOLDER;
                        ghost_button_id(right, "Sound", MuteButton, &mut mute_text);
                        mute_label = mute_text;
                    });
                });

                // Timer: tracked caps over a thin brass rule; hidden when idle.
                countdown_pill = ui
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(8),
                        display: Display::None,
                        ..default()
                    })
                    .with_children(|pill| {
                        countdown = text_entity(pill, 15.0, GOLD);
                        pill.spawn((
                            Node {
                                height: px(1),
                                width: px(150),
                                ..default()
                            },
                            BackgroundColor(GOLD),
                        ));
                    })
                    .id();

                // Target: tracked label on the left, big numeral on the right.
                // The label is padded up so it sits on the numeral's baseline.
                ui.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::FlexEnd,
                    column_gap: px(16),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new("TARGET"),
                        TextFont {
                            font_size: FontSize::Px(15.0),
                            ..default()
                        },
                        TextColor(MUTED),
                        Node {
                            padding: UiRect {
                                bottom: px(18),
                                ..default()
                            },
                            ..default()
                        },
                    ));
                    target = text_with(row, 96.0, TEXT, &display);
                });

                ui.spawn(Node {
                    width: percent(100),
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
                                    width: px(116),
                                    height: px(160),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::FlexStart,
                                    align_items: AlignItems::FlexStart,
                                    padding: UiRect::axes(px(14), px(12)),
                                    row_gap: px(2),
                                    border: UiRect::all(px(2)),
                                    border_radius: BorderRadius::all(px(22)),
                                    ..default()
                                },
                                BackgroundColor(CARD),
                                BorderColor::all(BORDER),
                                UiTransform::default(),
                                BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), px(0), px(8), px(0), px(18)),
                            ))
                            .with_children(|card| {
                                ranks[index] = text_with(card, 46.0, CARD_INK, &display);
                                suits[index] = text_with(card, 34.0, CARD_INK, &suit_font);
                                // Stacked fraction (numerator / bar / denominator),
                                // shown for merged fractional values.
                                fracs[index] = card
                                    .spawn(Node {
                                        width: percent(100),
                                        flex_direction: FlexDirection::Column,
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        row_gap: px(2),
                                        display: Display::None,
                                        ..default()
                                    })
                                    .with_children(|frac| {
                                        nums[index] = text_with(frac, 30.0, GOLD, &display);
                                        frac.spawn((
                                            Node {
                                                height: px(3),
                                                width: px(48),
                                                border_radius: BorderRadius::all(px(2)),
                                                ..default()
                                            },
                                            BackgroundColor(GOLD),
                                        ));
                                        dens[index] = text_with(frac, 30.0, GOLD, &display);
                                    })
                                    .id();
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
                                    width: px(76),
                                    height: px(76),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(BG),
                                BorderColor::all(BORDER),
                            ))
                            .with_children(|key| {
                                operator_labels[index] = text_entity(key, 30.0, TEXT);
                            })
                            .id();
                    }
                });

                status = text_entity(ui, 21.0, TEXT);
                score = text_entity(ui, 13.0, MUTED);
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

                // The primary action after a round: a solid brass "Next puzzle".
                next = ui
                    .spawn((
                        Button,
                        NextPuzzleButton,
                        Hoverable { base: GOLD },
                        Node {
                            padding: UiRect::axes(px(28), px(14)),
                            border_radius: BorderRadius::MAX,
                            justify_content: JustifyContent::Center,
                            display: Display::None,
                            ..default()
                        },
                        BackgroundColor(GOLD),
                        BorderColor::all(GOLD),
                    ))
                    .with_children(|button| {
                        text_body(button, "Next puzzle", 18.0, CARD_INK);
                    })
                    .id();

                // In-play actions, then a gap, then puzzle management.
                ui.spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Row,
                    column_gap: px(14),
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    ghost_button(row, "Hint", HintButton);
                    ghost_button(row, "Reveal", RevealButton);
                    more = ghost_button(row, "More", ShowMoreButton);
                    undo = ghost_button_id(row, "Undo", UndoButton, &mut undo_label);
                    // Visual separation: giving up is not a nav action.
                    row.spawn(Node {
                        width: px(44),
                        ..default()
                    });
                    give_up = ghost_button(row, "Give up", NewPuzzleButton);
                    ghost_button(row, "Menu", BackButton);
                });
            });
        });

    commands.insert_resource(BoardUi {
        target,
        mode_chip,
        mute_label,
        countdown,
        countdown_pill,
        status,
        score,
        message,
        undo,
        undo_label,
        more,
        give_up,
        next,
        cards,
        ranks,
        suits,
        fracs,
        nums,
        dens,
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

/// Real tier parameters, so the difficulty screen says something concrete.
/// Both modes follow the tier card ladder.
fn tier_meta(difficulty: Difficulty) -> String {
    let cards = difficulty.card_count();
    match difficulty.view_seconds() {
        None => format!("{cards} cards · unlimited view"),
        Some(0) => format!("{cards} cards · never shown"),
        Some(seconds) => format!("{cards} cards · {seconds}s view"),
    }
}

/// What the tier actually changes, per mode.
fn tier_note(mode: Mode) -> &'static str {
    match mode {
        Mode::Classic => "Tiers set the card count. Target stays 24.",
        Mode::Custom => "Tiers set the card count and the target.",
    }
}

fn difficulty_slider_changed(
    sliders: Query<&SliderValue, (Changed<SliderValue>, With<DifficultySlider>)>,
    mut game: ResMut<Game>,
    mut labels: Query<&mut Text, (With<DifficultyLabel>, Without<DifficultyMeta>)>,
    mut metas: Query<&mut Text, (With<DifficultyMeta>, Without<DifficultyLabel>)>,
) {
    for value in &sliders {
        let index = value.0.round().clamp(0.0, (Difficulty::ALL.len() - 1) as f32) as usize;
        game.difficulty = Difficulty::ALL[index];
        for mut label in &mut labels {
            **label = game.difficulty.label().to_string();
        }
        for mut meta in &mut metas {
            **meta = tracked(&tier_meta(game.difficulty));
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
        **label = entry.display();
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

/// The glyph shown on an operator key (serif-ish × and ÷ rather than * and /).
fn key_symbol(op: Op) -> &'static str {
    match op {
        Op::Add => "+",
        Op::Sub => "-",
        Op::Mul => "×",
        Op::Div => "÷",
    }
}

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
#[allow(clippy::too_many_arguments)]
fn fx_system(
    game: Res<Game>,
    sfx: Res<Sfx>,
    _marker: NonSendMarker,
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
        if !game.muted {
            play(&mut commands, &sfx.deal);
        }
        prev.dealt = game.dealt.clone();
    } else if game.round.cards.len() < prev.cards.len() {
        if !game.muted {
            play(&mut commands, &sfx.merge);
            haptics::impact();
        }
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
            if !game.muted {
                let sound = if game.round.phase == Phase::Won {
                    &sfx.win
                } else {
                    &sfx.lose
                };
                play(&mut commands, sound);
                if game.round.phase == Phase::Won {
                    haptics::success();
                }
            }
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

fn mute_click(
    mut game: ResMut<Game>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<MuteButton>)>,
) {
    if pressed(&interactions) {
        game.muted = !game.muted;
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

#[allow(clippy::type_complexity)] // Bevy query filters get verbose
fn new_puzzle(
    mut game: ResMut<Game>,
    mut timer: ResMut<ViewTimer>,
    time: Res<Time>,
    interactions: Query<
        &Interaction,
        (
            Changed<Interaction>,
            Or<(With<NewPuzzleButton>, With<NextPuzzleButton>)>,
        ),
    >,
) {
    if pressed(&interactions) {
        deal(&mut game, &mut timer, time.elapsed_secs());
    }
}

#[allow(clippy::too_many_arguments)] // Bevy systems routinely take many params
fn update_board(
    game: Res<Game>,
    ui: Option<Res<BoardUi>>,
    mut texts: Query<&mut Text>,
    mut text_colors: Query<&mut TextColor>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut borders: Query<&mut BorderColor>,
    mut nodes: Query<&mut Node>,
    mut transforms: Query<&mut UiTransform>,
) {
    let Some(ui) = ui else {
        return;
    };
    set_text(&mut texts, ui.target, game.round.target.to_string());
    set_text(
        &mut texts,
        ui.mode_chip,
        tracked(&format!("{} · {}", game.mode.label(), game.difficulty.label())),
    );
    set_text(
        &mut texts,
        ui.mute_label,
        if game.muted { "Muted" } else { "Sound" }.to_string(),
    );
    set_text(&mut texts, ui.status, game.status());
    if let Ok(mut color) = text_colors.get_mut(ui.status) {
        *color = TextColor(match game.round.phase {
            Phase::Won => WIN,
            Phase::Lost => LOSE,
            Phase::Playing => TEXT,
        });
    }
    set_text(&mut texts, ui.message, game.message.clone());
    set_text(
        &mut texts,
        ui.score,
        match game.last_score {
            Some(value) => tracked(&format!("Score {}  ·  +{value}", game.total_score)),
            None => tracked(&format!("Score {}", game.total_score)),
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

            let background;
            let ink;

            if !revealed {
                set_display(&mut nodes, ui.ranks[index], Display::Flex);
                set_display(&mut nodes, ui.suits[index], Display::None);
                set_display(&mut nodes, ui.fracs[index], Display::None);
                set_text(&mut texts, ui.ranks[index], "?".to_string());
                ink = MUTED;
                background = CARD_BACK;
            } else if let Some(suit) = suit {
                set_display(&mut nodes, ui.ranks[index], Display::Flex);
                set_display(&mut nodes, ui.suits[index], Display::Flex);
                set_display(&mut nodes, ui.fracs[index], Display::None);
                set_text(&mut texts, ui.ranks[index], labels[index].clone());
                set_text(&mut texts, ui.suits[index], suit.glyph().to_string());
                ink = if suit.is_red() { CARD_RED } else { CARD_INK };
                background = CARD;
            } else {
                // Merged value: an integer token, or a stacked fraction.
                let (numer, denom) = game.round.cards[index].parts();
                if denom == 1 {
                    set_display(&mut nodes, ui.ranks[index], Display::Flex);
                    set_display(&mut nodes, ui.suits[index], Display::None);
                    set_display(&mut nodes, ui.fracs[index], Display::None);
                    set_text(&mut texts, ui.ranks[index], numer.to_string());
                } else {
                    set_display(&mut nodes, ui.ranks[index], Display::None);
                    set_display(&mut nodes, ui.suits[index], Display::None);
                    set_display(&mut nodes, ui.fracs[index], Display::Flex);
                    set_text(&mut texts, ui.nums[index], numer.to_string());
                    set_text(&mut texts, ui.dens[index], denom.to_string());
                }
                ink = GOLD;
                background = TOKEN;
            }

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
                // Only the selected card gets a ring; otherwise the border is
                // invisible so ivory cards read as clean paper.
                *border = BorderColor::all(if selected { GOLD } else { background });
            }
            // Lift the selected card so selection reads physically.
            if let Ok(mut transform) = transforms.get_mut(slot) {
                transform.translation =
                    Val2::px(0.0, if selected { -14.0 } else { 0.0 });
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
            *color = BackgroundColor(if pending { GOLD } else { BG });
        }
        if let Ok(mut border) = borders.get_mut(slot) {
            *border = BorderColor::all(if pending { GOLD } else { BORDER });
        }
        set_text(&mut texts, label, key_symbol(*operator).to_string());
        if let Ok(mut color) = text_colors.get_mut(label) {
            *color = TextColor(if pending { CARD_INK } else { TEXT });
        }
    }

    let undo_enabled = game.round.can_undo();
    // Ghost control: underline + text dim when there is nothing to undo.
    if let Ok(mut border) = borders.get_mut(ui.undo) {
        *border = BorderColor::all(if undo_enabled { MUTED } else { BG });
    }
    set_text(&mut texts, ui.undo_label, "Undo".to_string());
    if let Ok(mut color) = text_colors.get_mut(ui.undo_label) {
        *color = TextColor(if undo_enabled { TEXT } else { MUTED });
    }

    // Mid-round: "Give up" is available; after the round: "Next puzzle".
    let playing = game.round.phase == Phase::Playing;
    set_display(
        &mut nodes,
        ui.give_up,
        if playing { Display::Flex } else { Display::None },
    );
    set_display(
        &mut nodes,
        ui.next,
        if playing { Display::None } else { Display::Flex },
    );

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
    game: Res<Game>,
    ui: Res<BoardUi>,
    mut texts: Query<&mut Text>,
    mut nodes: Query<&mut Node>,
) {
    let text = match timer.phase.seconds_left() {
        Some(seconds) => tracked(&format!("Hiding in {seconds}s")),
        None => String::new(),
    };
    set_text(&mut texts, ui.countdown, text);

    // Only show the timer while a countdown is actually running mid-round.
    let display = if timer.phase.seconds_left().is_some() && game.round.phase == Phase::Playing {
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
    fn tier_meta_reports_the_dealt_card_count() {
        for difficulty in Difficulty::ALL {
            assert!(
                tier_meta(difficulty).starts_with(&format!("{} cards", difficulty.card_count())),
                "advertised meta must match the dealt count: {difficulty:?}"
            );
        }
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
