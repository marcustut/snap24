//! Pure round rules for the desktop app: infix selection, merging, win/lose.
//!
//! Kept free of Bevy so the rules can be unit-tested without a window. The Bevy
//! layer reads [`Round`] out of a resource and rebuilds the board from it.
//!
//! The player builds an expression in infix order: tap a card, tap an operator,
//! then tap a second card, which performs the merge. Every successful merge is
//! pushed onto an undo stack.

use snap24_core::Rational;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

impl Op {
    pub const ALL: [Op; 4] = [Op::Add, Op::Sub, Op::Mul, Op::Div];

    pub fn symbol(self) -> char {
        match self {
            Op::Add => '+',
            Op::Sub => '-',
            Op::Mul => '*',
            Op::Div => '/',
        }
    }
}

/// A card suit. Cosmetic only — it never affects the maths; it exists so dealt
/// cards look like real poker cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Suit {
    Spade,
    Heart,
    Diamond,
    Club,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Spade, Suit::Heart, Suit::Diamond, Suit::Club];

    pub fn glyph(self) -> &'static str {
        match self {
            Suit::Spade => "♠",
            Suit::Heart => "♥",
            Suit::Diamond => "♦",
            Suit::Club => "♣",
        }
    }

    pub fn is_red(self) -> bool {
        matches!(self, Suit::Heart | Suit::Diamond)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Playing,
    Won,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeError {
    /// An operator was chosen before a first card.
    PickCardFirst,
    /// The second card was zero, so the division is undefined.
    DivideByZero,
}

/// The view window: how long the dealt cards stay face-up. `Easy` is
/// unlimited, `Blind` starts hidden, everything else counts down then conceals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewPhase {
    Unlimited,
    Visible { remaining: f32 },
    Hidden,
}

impl ViewPhase {
    pub fn from_view_seconds(seconds: Option<u32>) -> Self {
        match seconds {
            None => ViewPhase::Unlimited,
            Some(0) => ViewPhase::Hidden,
            Some(n) => ViewPhase::Visible { remaining: n as f32 },
        }
    }

    /// Whole seconds left, for the on-screen countdown.
    pub fn seconds_left(self) -> Option<u32> {
        match self {
            ViewPhase::Visible { remaining } => Some(remaining.max(0.0).ceil() as u32),
            _ => None,
        }
    }

    /// Advance by `dt` seconds. Returns `true` only on the tick that just
    /// ended the view window, so the caller knows to flip the cards.
    pub fn tick(&mut self, dt: f32) -> bool {
        if let ViewPhase::Visible { remaining } = self {
            *remaining -= dt;
            if *remaining <= 0.0 {
                *self = ViewPhase::Hidden;
                return true;
            }
        }
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    cards: Vec<Rational>,
    suits: Vec<Option<Suit>>,
    revealed: Vec<bool>,
    first: Option<usize>,
    op: Option<Op>,
    phase: Phase,
}

/// A single round: the cards on the board, the target, and the in-progress
/// selection (`first` card and pending `op`). The second card is chosen last,
/// so `A - B` and `B - A` are both reachable by tap order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Round {
    pub target: Rational,
    pub cards: Vec<Rational>,
    /// Cosmetic suit per card; `None` for merged values (which are tokens, not
    /// cards). Parallel to `cards`.
    pub suits: Vec<Option<Suit>>,
    /// Whether each card is currently face-up. Parallel to `cards`.
    pub revealed: Vec<bool>,
    /// Index of the first operand, if one has been tapped.
    pub first: Option<usize>,
    /// The operator chosen after the first card, if any.
    pub op: Option<Op>,
    pub phase: Phase,
    history: Vec<Snapshot>,
}

impl Round {
    pub fn new(cards: Vec<i64>, target: Rational) -> Self {
        let cards: Vec<Rational> = cards.into_iter().map(Rational::from).collect();
        // Spread suits across the hand (a real deck never has two identical
        // cards). Mixing the rank into the choice keeps a hand of distinct
        // ranks from coming out all one suit; the per-rank counter keeps
        // duplicates of a rank on different suits.
        let mut seen: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        let suits = cards
            .iter()
            .map(|card| {
                let rank = card.as_i64().unwrap_or(0);
                let count = seen.entry(rank).or_insert(0);
                let suit = Suit::ALL[(rank as usize + *count) % Suit::ALL.len()];
                *count += 1;
                Some(suit)
            })
            .collect();
        let revealed = vec![true; cards.len()];
        Round {
            target,
            cards,
            suits,
            revealed,
            first: None,
            op: None,
            phase: Phase::Playing,
            history: Vec::new(),
        }
    }

    /// The suit shown on card `index`, or `None` if it is a merged value.
    pub fn suit(&self, index: usize) -> Option<Suit> {
        self.suits.get(index).copied().flatten()
    }

    /// Flip every card face-down (timer expiry, or the Blind tier from the
    /// start). Slot positions do not move.
    pub fn conceal(&mut self) {
        self.revealed.iter_mut().for_each(|shown| *shown = false);
    }

    pub fn is_revealed(&self, index: usize) -> bool {
        self.revealed.get(index).copied().unwrap_or(false)
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Take back the last merge, restoring the cards and the pending
    /// `first`/`op` selection it had just before. Returns `false` if there is
    /// nothing to undo.
    pub fn undo(&mut self) -> bool {
        match self.history.pop() {
            Some(snapshot) => {
                self.cards = snapshot.cards;
                self.suits = snapshot.suits;
                self.revealed = snapshot.revealed;
                self.first = snapshot.first;
                self.op = snapshot.op;
                self.phase = snapshot.phase;
                true
            }
            None => false,
        }
    }

    pub fn is_first(&self, index: usize) -> bool {
        self.first == Some(index)
    }

    fn push_history(&mut self) {
        self.history.push(Snapshot {
            cards: self.cards.clone(),
            suits: self.suits.clone(),
            revealed: self.revealed.clone(),
            first: self.first,
            op: self.op,
            phase: self.phase,
        });
    }

    /// Tap a card. Without a first card this selects it; without an operator it
    /// replaces/clears the first card; with both set this is the second card and
    /// performs the merge. Every change is undoable.
    pub fn click_card(&mut self, index: usize) -> Result<(), MergeError> {
        if self.phase != Phase::Playing || index >= self.cards.len() {
            return Ok(());
        }
        match (self.first, self.op) {
            (None, _) => {
                self.push_history();
                self.first = Some(index);
                Ok(())
            }
            (Some(first), None) => {
                self.push_history();
                if first == index {
                    self.first = None;
                } else {
                    self.first = Some(index);
                }
                Ok(())
            }
            (Some(first), Some(_)) => {
                // Tapping the first card again is not a valid second operand.
                if first == index {
                    return Ok(());
                }
                self.merge(index)
            }
        }
    }

    /// Tap an operator. Requires a first card already chosen.
    pub fn click_op(&mut self, op: Op) -> Result<(), MergeError> {
        if self.phase != Phase::Playing {
            return Ok(());
        }
        if self.first.is_none() {
            return Err(MergeError::PickCardFirst);
        }
        if self.op != Some(op) {
            self.push_history();
            self.op = Some(op);
        }
        Ok(())
    }

    /// Combine the first card and `index` with the pending operator.
    fn merge(&mut self, index: usize) -> Result<(), MergeError> {
        let first = self.first.expect("merge called without a first card");
        let op = self.op.expect("merge called without an operator");
        let left = self.cards[first];
        let right = self.cards[index];
        let value = match op {
            Op::Add => left.add(right),
            Op::Sub => left.sub(right),
            Op::Mul => left.mul(right),
            Op::Div => match left.div(right) {
                Some(value) => value,
                None => return Err(MergeError::DivideByZero),
            },
        };

        self.push_history();

        let (low, high) = if first < index { (first, index) } else { (index, first) };
        self.cards.remove(high);
        self.cards.remove(low);
        self.cards.insert(low, value);
        // The result is a computed value, not a card, so it has no suit.
        self.suits.remove(high);
        self.suits.remove(low);
        self.suits.insert(low, None);
        // Agreed reveal rule: the computed merge result is always shown, even
        // when the operands were face-down. The other cards keep their state.
        self.revealed.remove(high);
        self.revealed.remove(low);
        self.revealed.insert(low, true);
        self.first = None;
        self.op = None;

        if self.cards.len() == 1 {
            self.phase = if self.cards[0] == self.target {
                Phase::Won
            } else {
                Phase::Lost
            };
        }
        Ok(())
    }

    /// A/J/Q/K for the card ranks, plain text otherwise (merged values may be
    /// larger integers or fractions).
    pub fn label(value: &Rational) -> String {
        match value {
            v if *v == Rational::from(1) => "A".to_string(),
            v if *v == Rational::from(11) => "J".to_string(),
            v if *v == Rational::from(12) => "Q".to_string(),
            v if *v == Rational::from(13) => "K".to_string(),
            v => v.to_string(),
        }
    }

    pub fn card_labels(&self) -> Vec<String> {
        self.cards.iter().map(Round::label).collect()
    }
}

/// Base points for winning a round, before the tier multiplier.
pub const BASE_SCORE: i32 = 100;
/// Solving within this many seconds earns the full time bonus; slower solves
/// earn proportionally less, never a negative bonus.
pub const PAR_SECONDS: f32 = 90.0;
pub const TIME_BONUS_PER_SECOND: i32 = 2;
/// Points removed per hint used.
pub const HINT_PENALTY: i32 = 30;

/// Score for one finished round. Losing scores nothing; winning pays
/// `tier_multiplier * BASE_SCORE`, plus a time bonus for solving under par,
/// minus the hint penalty. Never negative.
pub fn round_score(multiplier: u32, elapsed_secs: f32, hints_used: u32, won: bool) -> i32 {
    if !won {
        return 0;
    }
    let base = multiplier as i32 * BASE_SCORE;
    let bonus = ((PAR_SECONDS - elapsed_secs).max(0.0) * TIME_BONUS_PER_SECOND as f32) as i32;
    let penalty = hints_used as i32 * HINT_PENALTY;
    (base + bonus - penalty).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(cards: &[i64], target: i64) -> Round {
        Round::new(cards.to_vec(), Rational::from(target))
    }

    #[test]
    fn rank_mapping() {
        assert_eq!(Round::label(&Rational::from(1)), "A");
        assert_eq!(Round::label(&Rational::from(10)), "10");
        assert_eq!(Round::label(&Rational::from(11)), "J");
        assert_eq!(Round::label(&Rational::from(12)), "Q");
        assert_eq!(Round::label(&Rational::from(13)), "K");
        assert_eq!(Round::label(&Rational::from(24)), "24");
        assert_eq!(Round::label(&Rational::new(3, 2)), "3/2");
    }

    #[test]
    fn copies_of_a_rank_get_four_distinct_suits() {
        let r = round(&[5, 5, 5, 5, 3], 24);
        let suits: Vec<Suit> = r.suits.iter().map(|s| s.expect("dealt cards have suits")).collect();
        let four_fives: std::collections::HashSet<Suit> = suits[..4].iter().copied().collect();
        assert_eq!(four_fives.len(), 4, "duplicate ranks must be different suits");
        assert_eq!(r.suit(4), Some(Suit::ALL[3]));
    }

    #[test]
    fn a_hand_of_distinct_ranks_is_not_all_one_suit() {
        let r = round(&[10, 12, 13, 8, 11], 24);
        let suits: std::collections::HashSet<Suit> =
            r.suits.iter().map(|s| s.unwrap()).collect();
        assert!(suits.len() > 1, "suits should vary across a hand");
    }

    #[test]
    fn merged_values_have_no_suit_and_undo_restores_them() {
        let mut r = round(&[9, 4], 5);
        r.click_card(0).unwrap();
        r.click_op(Op::Sub).unwrap();
        r.click_card(1).unwrap();
        assert_eq!(r.suits, vec![None]);
        assert!(r.undo());
        assert_eq!(r.suits.len(), 2);
        assert!(r.suits.iter().all(Option::is_some));
    }

    #[test]
    fn infix_flow_is_card_then_op_then_card() {
        let mut r = round(&[1, 2, 3], 6);
        r.click_card(0).unwrap();
        assert_eq!(r.first, Some(0));
        assert_eq!(r.op, None);

        r.click_op(Op::Add).unwrap();
        assert_eq!(r.op, Some(Op::Add));
        assert_eq!(r.cards.len(), 3); // nothing merged yet

        r.click_card(1).unwrap();
        assert_eq!(r.cards, vec![Rational::from(3), Rational::from(3)]);
        assert_eq!(r.first, None);
        assert_eq!(r.op, None);
    }

    #[test]
    fn operator_before_a_card_is_rejected() {
        let mut r = round(&[1, 2, 3], 6);
        assert_eq!(r.click_op(Op::Add), Err(MergeError::PickCardFirst));
        assert_eq!(r.op, None);
    }

    #[test]
    fn tapping_another_card_replaces_the_first() {
        let mut r = round(&[1, 2, 3], 6);
        r.click_card(0).unwrap();
        r.click_card(1).unwrap();
        assert_eq!(r.first, Some(1));
        r.click_card(1).unwrap(); // tapping it again clears
        assert_eq!(r.first, None);
    }

    #[test]
    fn the_operator_can_be_changed_before_the_second_card() {
        let mut r = round(&[9, 4], 5);
        r.click_card(0).unwrap();
        r.click_op(Op::Add).unwrap();
        r.click_op(Op::Sub).unwrap();
        assert_eq!(r.op, Some(Op::Sub));
        r.click_card(1).unwrap();
        assert_eq!(r.cards, vec![Rational::from(5)]);
        assert_eq!(r.phase, Phase::Won);
    }

    #[test]
    fn subtraction_respects_infix_order() {
        let mut r = round(&[9, 4], 5);
        r.click_card(0).unwrap(); // 9 first
        r.click_op(Op::Sub).unwrap();
        r.click_card(1).unwrap(); // 9 - 4
        assert_eq!(r.cards, vec![Rational::from(5)]);
        assert_eq!(r.phase, Phase::Won);

        let mut r = round(&[9, 4], 5);
        r.click_card(1).unwrap(); // 4 first
        r.click_op(Op::Sub).unwrap();
        r.click_card(0).unwrap(); // 4 - 9
        assert_eq!(r.cards, vec![Rational::from(-5)]);
        assert_eq!(r.phase, Phase::Lost);
    }

    #[test]
    fn tapping_the_first_card_as_second_operand_is_ignored() {
        let mut r = round(&[6, 0], 6);
        r.click_card(0).unwrap();
        r.click_op(Op::Mul).unwrap();
        r.click_card(0).unwrap(); // same card; no merge
        assert_eq!(r.cards.len(), 2);
        assert_eq!(r.first, Some(0));
    }

    #[test]
    fn division_by_zero_is_blocked_and_state_is_kept() {
        let mut r = round(&[6, 0], 6);
        r.click_card(0).unwrap();
        r.click_op(Op::Div).unwrap();
        assert_eq!(r.click_card(1), Err(MergeError::DivideByZero));
        // Nothing changed, so the player can pick a different second card/op.
        assert_eq!(r.cards.len(), 2);
        assert_eq!(r.first, Some(0));
        assert_eq!(r.op, Some(Op::Div));
    }

    #[test]
    fn exact_fractions_are_kept() {
        let mut r = round(&[1, 2], 2);
        r.click_card(0).unwrap();
        r.click_op(Op::Div).unwrap();
        r.click_card(1).unwrap(); // 1 / 2
        assert_eq!(r.cards, vec![Rational::new(1, 2)]);
        assert_eq!(Round::label(&r.cards[0]), "1/2");
        assert_eq!(r.phase, Phase::Lost);
    }

    #[test]
    fn win_and_lose_on_final_value() {
        let mut win = round(&[9, 1, 3], 24);
        win.click_card(0).unwrap();
        win.click_op(Op::Sub).unwrap();
        win.click_card(1).unwrap(); // 9 - 1 = 8
        win.click_card(0).unwrap();
        win.click_op(Op::Mul).unwrap();
        win.click_card(1).unwrap(); // 8 * 3 = 24
        assert_eq!(win.phase, Phase::Won);

        let mut lose = round(&[9, 1, 3], 24);
        lose.click_card(0).unwrap();
        lose.click_op(Op::Add).unwrap();
        lose.click_card(1).unwrap(); // 10
        lose.click_card(0).unwrap();
        lose.click_op(Op::Mul).unwrap();
        lose.click_card(1).unwrap(); // 30
        assert_eq!(lose.cards, vec![Rational::from(30)]);
        assert_eq!(lose.phase, Phase::Lost);
    }

    #[test]
    fn undo_takes_back_a_wrong_merge_and_keeps_the_operator() {
        let mut r = round(&[9, 1, 3], 24);
        assert!(!r.can_undo());
        r.click_card(0).unwrap();
        r.click_op(Op::Add).unwrap();
        r.click_card(1).unwrap(); // 9 + 1 = 10 (wrong)
        assert!(r.can_undo());
        assert_eq!(r.cards, vec![Rational::from(10), Rational::from(3)]);

        assert!(r.undo());
        assert_eq!(r.cards, vec![Rational::from(9), Rational::from(1), Rational::from(3)]);
        // The first card is re-selected so a different second card can be tried.
        assert_eq!(r.first, Some(0));
        assert_eq!(r.op, Some(Op::Add));
        // The earlier selections are still on the stack, so undo keeps working.
        assert!(r.can_undo());
    }

    #[test]
    fn undo_clears_the_first_card_selection() {
        let mut r = round(&[1, 2, 3], 6);
        assert!(!r.can_undo());
        r.click_card(0).unwrap();
        assert_eq!(r.first, Some(0));
        assert!(r.can_undo());
        assert!(r.undo());
        assert_eq!(r.first, None);
        assert!(!r.can_undo());
    }

    #[test]
    fn undo_steps_back_through_selections_then_merges() {
        let mut r = round(&[9, 1, 3], 24);
        r.click_card(0).unwrap(); // pick 9
        r.click_op(Op::Sub).unwrap(); // pick -
        assert!(r.undo()); // un-pick the operator
        assert_eq!((r.first, r.op), (Some(0), None));
        assert!(r.undo()); // un-pick the first card
        assert_eq!((r.first, r.op), (None, None));
        assert!(!r.can_undo());

        r.click_card(0).unwrap();
        r.click_op(Op::Sub).unwrap();
        r.click_card(1).unwrap(); // 9 - 1 = 8 (merge)
        assert!(r.undo()); // back to "8" undo: restores 9,1,3 with 9 and - pending
        assert_eq!(r.cards, vec![Rational::from(9), Rational::from(1), Rational::from(3)]);
        assert_eq!((r.first, r.op), (Some(0), Some(Op::Sub)));
    }

    #[test]
    fn undo_retries_wrong_combinations_to_a_win() {
        let mut r = round(&[9, 1, 3], 24);
        r.click_card(0).unwrap();
        r.click_op(Op::Add).unwrap();
        r.click_card(1).unwrap(); // 10
        r.click_card(0).unwrap();
        r.click_op(Op::Mul).unwrap();
        r.click_card(1).unwrap(); // 30 -> Lost
        assert_eq!(r.phase, Phase::Lost);

        // Undo repeatedly until the full three-card hand is back.
        while r.cards.len() < 3 {
            assert!(r.undo());
        }
        assert_eq!(r.cards, vec![Rational::from(9), Rational::from(1), Rational::from(3)]);

        r.click_card(0).unwrap();
        r.click_op(Op::Sub).unwrap();
        r.click_card(1).unwrap(); // 8
        r.click_card(0).unwrap();
        r.click_op(Op::Mul).unwrap();
        r.click_card(1).unwrap(); // 24 -> Won
        assert_eq!(r.phase, Phase::Won);
    }

    #[test]
    fn no_op_actions_do_not_create_undo_history() {
        // An operator with no card selected changes nothing.
        let mut r = round(&[1, 2, 3], 6);
        assert_eq!(r.click_op(Op::Add), Err(MergeError::PickCardFirst));
        assert!(!r.can_undo());

        // A blocked merge changes nothing, so unwinding the two selections
        // returns straight to the start.
        let mut r = round(&[6, 0], 6);
        r.click_card(0).unwrap();
        r.click_op(Op::Div).unwrap();
        assert_eq!(r.click_card(1), Err(MergeError::DivideByZero));
        assert!(r.undo());
        assert_eq!((r.first, r.op), (Some(0), None));
        assert!(r.undo());
        assert_eq!((r.first, r.op), (None, None));
        assert!(!r.can_undo());

        // Re-tapping the first card as the second operand is ignored.
        let mut r = round(&[6, 0], 6);
        r.click_card(0).unwrap();
        r.click_op(Op::Mul).unwrap();
        r.click_card(0).unwrap();
        assert!(r.undo());
        assert!(r.undo());
        assert!(!r.can_undo());
    }

    #[test]
    fn input_is_ignored_after_the_round_ends() {
        let mut r = round(&[9, 1], 8);
        r.click_card(0).unwrap();
        r.click_op(Op::Sub).unwrap();
        r.click_card(1).unwrap(); // 9 - 1 = 8 -> Won
        assert_eq!(r.phase, Phase::Won);
        r.click_card(0).unwrap();
        assert_eq!(r.first, None);
        assert_eq!(r.click_op(Op::Add), Ok(()));
        assert_eq!(r.op, None);
    }

    #[test]
    fn cards_start_revealed() {
        let r = round(&[1, 2, 3], 6);
        assert_eq!(r.revealed, vec![true, true, true]);
        assert!(r.is_revealed(0));
        assert!(!r.is_revealed(99));
    }

    #[test]
    fn conceal_hides_every_card_without_moving_slots() {
        let mut r = round(&[9, 1, 3], 24);
        let before = r.cards.clone();
        r.conceal();
        assert_eq!(r.revealed, vec![false, false, false]);
        assert_eq!(r.cards, before);
    }

    #[test]
    fn merge_reveals_only_the_result_when_operands_were_hidden() {
        let mut r = round(&[9, 1, 3], 24);
        r.conceal();
        r.click_card(0).unwrap();
        r.click_op(Op::Sub).unwrap();
        r.click_card(1).unwrap(); // 9 - 1 = 8
        assert_eq!(r.cards, vec![Rational::from(8), Rational::from(3)]);
        // The result is shown; the untouched card stays face-down.
        assert_eq!(r.revealed, vec![true, false]);
    }

    #[test]
    fn undo_restores_reveal_state() {
        let mut r = round(&[9, 1, 3], 24);
        r.conceal();
        r.click_card(0).unwrap();
        r.click_op(Op::Sub).unwrap();
        r.click_card(1).unwrap();
        assert_eq!(r.revealed, vec![true, false]);
        assert!(r.undo());
        assert_eq!(r.revealed, vec![false, false, false]);
        assert_eq!(r.cards, vec![Rational::from(9), Rational::from(1), Rational::from(3)]);
    }

    #[test]
    fn view_phase_maps_from_tier_seconds() {
        assert_eq!(ViewPhase::from_view_seconds(None), ViewPhase::Unlimited);
        assert_eq!(ViewPhase::from_view_seconds(Some(0)), ViewPhase::Hidden);
        assert_eq!(
            ViewPhase::from_view_seconds(Some(10)),
            ViewPhase::Visible { remaining: 10.0 }
        );
        assert_eq!(ViewPhase::from_view_seconds(Some(10)).seconds_left(), Some(10));
        assert_eq!(ViewPhase::Unlimited.seconds_left(), None);
        assert_eq!(ViewPhase::Hidden.seconds_left(), None);
    }

    #[test]
    fn view_phase_ticks_down_and_expires_exactly_once() {
        let mut view = ViewPhase::from_view_seconds(Some(2));
        assert!(!view.tick(0.5));
        assert_eq!(view.seconds_left(), Some(2)); // ceil(1.5)
        assert!(!view.tick(1.0));
        assert_eq!(view.seconds_left(), Some(1)); // ceil(0.5)
        assert!(view.tick(1.0)); // crosses zero -> just expired
        assert_eq!(view, ViewPhase::Hidden);
        assert!(!view.tick(1.0)); // hidden stays hidden, no repeat expiry

        let mut unlimited = ViewPhase::Unlimited;
        assert!(!unlimited.tick(99.0));
        assert_eq!(unlimited, ViewPhase::Unlimited);
    }

    #[test]
    fn losing_scores_nothing() {
        assert_eq!(round_score(8, 0.0, 0, false), 0);
    }

    #[test]
    fn faster_wins_score_more_than_slower_ones() {
        let fast = round_score(3, 10.0, 0, true);
        let slow = round_score(3, 80.0, 0, true);
        let over_par = round_score(3, 120.0, 0, true);
        assert!(fast > slow);
        assert!(slow > over_par);
        assert_eq!(over_par, 3 * BASE_SCORE); // no bonus once past par
    }

    #[test]
    fn hints_reduce_the_score_and_it_never_goes_negative() {
        let clean = round_score(2, 0.0, 0, true);
        let hinted = round_score(2, 0.0, 1, true);
        assert_eq!(hinted, clean - HINT_PENALTY);
        assert_eq!(round_score(1, 200.0, 100, true), 0);
    }

    #[test]
    fn higher_tiers_multiply_the_base() {
        assert_eq!(round_score(1, 200.0, 0, true), BASE_SCORE);
        assert_eq!(round_score(4, 200.0, 0, true), 4 * BASE_SCORE);
    }
}
