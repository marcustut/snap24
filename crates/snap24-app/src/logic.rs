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

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    cards: Vec<Rational>,
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
    /// Index of the first operand, if one has been tapped.
    pub first: Option<usize>,
    /// The operator chosen after the first card, if any.
    pub op: Option<Op>,
    pub phase: Phase,
    history: Vec<Snapshot>,
}

impl Round {
    pub fn new(cards: Vec<i64>, target: Rational) -> Self {
        Round {
            target,
            cards: cards.into_iter().map(Rational::from).collect(),
            first: None,
            op: None,
            phase: Phase::Playing,
            history: Vec::new(),
        }
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

    /// Tap a card. Without a first card this selects it; without an operator it
    /// replaces/clears the first card; with both set this is the second card and
    /// performs the merge.
    pub fn click_card(&mut self, index: usize) -> Result<(), MergeError> {
        if self.phase != Phase::Playing || index >= self.cards.len() {
            return Ok(());
        }
        match (self.first, self.op) {
            (None, _) => {
                self.first = Some(index);
                Ok(())
            }
            (Some(first), None) => {
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
        self.op = Some(op);
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

        self.history.push(Snapshot {
            cards: self.cards.clone(),
            first: self.first,
            op: self.op,
            phase: self.phase,
        });

        let (low, high) = if first < index { (first, index) } else { (index, first) };
        self.cards.remove(high);
        self.cards.remove(low);
        self.cards.insert(low, value);
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
        assert!(!r.can_undo());
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

        assert!(r.undo()); // back to 10, 3
        assert!(r.undo()); // back to 9, 1, 3
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
    fn selection_changes_do_not_create_undo_history() {
        let mut r = round(&[1, 2, 3], 6);
        r.click_card(0).unwrap();
        r.click_card(1).unwrap(); // replaces the first card
        r.click_op(Op::Add).unwrap();
        assert!(!r.can_undo());

        // A blocked merge must not become undoable either.
        let mut r = round(&[6, 0], 6);
        r.click_card(0).unwrap();
        r.click_op(Op::Div).unwrap();
        assert_eq!(r.click_card(1), Err(MergeError::DivideByZero));
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
}
