//! Puzzle generation and the difficulty ladder.
//!
//! `generate` deals a puzzle that is guaranteed solvable by construction:
//! Classic draws a 5-card hand until 24 is reachable; Custom draws a hand and
//! then picks the target out of that hand's own reachable set. Generation is
//! deterministic for a given [`Rng`] seed.

use crate::{reachable, Rational};

/// A complete standard deck: ranks `A=1 … K=13`, four suits each.
fn deck() -> Vec<i64> {
    (1..=13).flat_map(|rank| std::iter::repeat_n(rank, 4)).collect()
}

/// Small deterministic RNG (xorshift64*). Good enough for dealing cards and
/// keeps `snap24-core` dependency-free; not for anything security-sensitive.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// Draw `count` distinct physical cards from a standard deck.
    fn deal(&mut self, count: usize) -> Vec<i64> {
        let mut cards = deck();
        for i in 0..count {
            let j = i + self.below(cards.len() - i);
            cards.swap(i, j);
        }
        cards.truncate(count);
        cards
    }
}

/// Which puzzle recipe to deal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Five cards, always targeting 24.
    Classic,
    /// Card count from the difficulty ladder, target drawn from the hand.
    Custom,
}

/// The difficulty ladder. Fewer cards is harder; once the card count bottoms
/// out, a shorter view window carries the rest of the difficulty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
    Expert,
    Insane,
    Blind,
}

impl Difficulty {
    pub const ALL: [Difficulty; 6] = [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
        Difficulty::Insane,
        Difficulty::Blind,
    ];

    /// How many cards a Custom puzzle deals at this tier.
    pub fn card_count(self) -> usize {
        match self {
            Difficulty::Easy | Difficulty::Medium => 5,
            Difficulty::Hard | Difficulty::Expert => 4,
            Difficulty::Insane | Difficulty::Blind => 3,
        }
    }

    /// Seconds the cards stay visible before flipping face-down. `None` means
    /// visible indefinitely (Easy); `Some(0)` means never shown (Blind).
    pub fn view_seconds(self) -> Option<u32> {
        match self {
            Difficulty::Easy => None,
            Difficulty::Medium => Some(10),
            Difficulty::Hard => Some(8),
            Difficulty::Expert => Some(6),
            Difficulty::Insane => Some(4),
            Difficulty::Blind => Some(0),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
            Difficulty::Expert => "Expert",
            Difficulty::Insane => "Insane",
            Difficulty::Blind => "Blind",
        }
    }

    /// Score multiplier for clearing a round at this tier. Ordered so a harder
    /// tier never scores less.
    pub fn score_multiplier(self) -> u32 {
        match self {
            Difficulty::Easy => 1,
            Difficulty::Medium => 2,
            Difficulty::Hard => 3,
            Difficulty::Expert => 4,
            Difficulty::Insane => 5,
            Difficulty::Blind => 8,
        }
    }
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Classic => "Classic",
            Mode::Custom => "Custom",
        }
    }
}

/// A dealt puzzle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Puzzle {
    pub mode: Mode,
    pub difficulty: Difficulty,
    pub cards: Vec<i64>,
    pub target: Rational,
}

fn sum_of(cards: &[i64]) -> Rational {
    cards.iter().fold(Rational::from(0), |acc, c| acc.add(Rational::from(*c)))
}

fn product_of(cards: &[i64]) -> Rational {
    cards.iter().fold(Rational::from(1), |acc, c| acc.mul(Rational::from(*c)))
}

/// A target is "trivial" if it needs no real puzzle-solving: `0` or `1`, a card
/// that was already dealt, or simply all the cards added/multiplied together.
fn is_trivial(target: Rational, cards: &[i64]) -> bool {
    target == Rational::from(0)
        || target == Rational::from(1)
        || cards.iter().any(|c| target == Rational::from(*c))
        || target == sum_of(cards)
        || target == product_of(cards)
}

/// Deal a solvable puzzle for `mode` and `difficulty`, consuming `rng`.
///
/// Classic retries until the drawn 5-card hand can make 24. Custom retries
/// until a non-trivial target can be drawn from the hand's reachable set.
pub fn generate(mode: Mode, difficulty: Difficulty, rng: &mut Rng) -> Puzzle {
    match mode {
        Mode::Classic => loop {
            let cards = rng.deal(5);
            if reachable(&cards).contains(&Rational::from(24)) {
                return Puzzle {
                    mode,
                    difficulty,
                    cards,
                    target: Rational::from(24),
                };
            }
        },
        Mode::Custom => loop {
            let cards = rng.deal(difficulty.card_count());
            let values = reachable(&cards);
            let candidates: Vec<Rational> =
                values.into_iter().filter(|v| !is_trivial(*v, &cards)).collect();
            if !candidates.is_empty() {
                let target = candidates[rng.below(candidates.len())];
                return Puzzle {
                    mode,
                    difficulty,
                    cards,
                    target,
                };
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipliers_are_ordered_by_difficulty() {
        let multipliers: Vec<u32> = Difficulty::ALL.iter().map(|d| d.score_multiplier()).collect();
        assert!(multipliers.windows(2).all(|w| w[0] <= w[1]), "{multipliers:?}");
        assert_eq!(Difficulty::Easy.score_multiplier(), 1);
        assert_eq!(Difficulty::Blind.score_multiplier(), 8);
    }

    #[test]
    fn ladder_is_five_down_to_three() {
        assert_eq!(Difficulty::Easy.card_count(), 5);
        assert_eq!(Difficulty::Medium.card_count(), 5);
        assert_eq!(Difficulty::Hard.card_count(), 4);
        assert_eq!(Difficulty::Expert.card_count(), 4);
        assert_eq!(Difficulty::Insane.card_count(), 3);
        assert_eq!(Difficulty::Blind.card_count(), 3);
        assert_eq!(Difficulty::Easy.view_seconds(), None);
        assert_eq!(Difficulty::Blind.view_seconds(), Some(0));
        // View window never increases as difficulty rises.
        let windows: Vec<u32> = Difficulty::ALL
            .iter()
            .filter_map(|d| d.view_seconds())
            .collect();
        assert!(windows.windows(2).all(|w| w[0] >= w[1]), "{windows:?}");
    }

    #[test]
    fn classic_is_five_cards_and_solvable_24() {
        for seed in 0..80 {
            let mut rng = Rng::new(seed);
            let puzzle = generate(Mode::Classic, Difficulty::Easy, &mut rng);
            assert_eq!(puzzle.cards.len(), 5);
            assert_eq!(puzzle.target, Rational::from(24));
            assert!(puzzle.cards.iter().all(|c| (1..=13).contains(c)));
            assert!(crate::is_solvable(&puzzle.cards, puzzle.target));
        }
    }

    #[test]
    fn custom_matches_tier_and_is_solvable() {
        for difficulty in Difficulty::ALL {
            for seed in 0..25 {
                let mut rng = Rng::new(seed * 31 + 7);
                let puzzle = generate(Mode::Custom, difficulty, &mut rng);
                assert_eq!(puzzle.cards.len(), difficulty.card_count());
                assert!(puzzle.cards.iter().all(|c| (1..=13).contains(c)));
                assert!(
                    reachable(&puzzle.cards).contains(&puzzle.target),
                    "target {:?} not reachable for {:?}",
                    puzzle.target,
                    puzzle.cards
                );
                assert!(!is_trivial(puzzle.target, &puzzle.cards));
            }
        }
    }

    #[test]
    fn generation_is_deterministic_per_seed() {
        for difficulty in Difficulty::ALL {
            let mut a = Rng::new(12345);
            let mut b = Rng::new(12345);
            assert_eq!(
                generate(Mode::Custom, difficulty, &mut a),
                generate(Mode::Custom, difficulty, &mut b)
            );
        }
        let mut a = Rng::new(999);
        let mut b = Rng::new(999);
        assert_eq!(
            generate(Mode::Classic, Difficulty::Medium, &mut a),
            generate(Mode::Classic, Difficulty::Medium, &mut b)
        );
    }

    #[test]
    fn different_seeds_can_differ() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        let hand_a = generate(Mode::Custom, Difficulty::Insane, &mut a).cards;
        let hand_b = generate(Mode::Custom, Difficulty::Insane, &mut b).cards;
        assert_ne!(hand_a, hand_b);
    }
}
