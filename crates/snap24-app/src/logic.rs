//! Pure round rules for the desktop app: selection, merging, win/lose.
//!
//! Kept free of Bevy so the rules can be unit-tested without a window. The Bevy
//! layer reads [`Round`] out of a resource and rebuilds the board from it.

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
    /// Fewer or more than two cards were selected.
    NeedTwoCards,
    /// The right-hand card was zero, so the division is undefined.
    DivideByZero,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    cards: Vec<Rational>,
    selected: Vec<usize>,
    phase: Phase,
}

/// A single round: the cards currently on the board, the target, and the
/// player's selection. The two selected cards are the operands in click order,
/// so the player chooses which side of `-` or `/` each card lands on.
///
/// Every successful merge is pushed onto an undo stack (selection included), so
/// a wrong combination can be taken back and the same two cards retried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Round {
    pub target: Rational,
    pub cards: Vec<Rational>,
    pub selected: Vec<usize>,
    pub phase: Phase,
    history: Vec<Snapshot>,
}

impl Round {
    pub fn new(cards: Vec<i64>, target: Rational) -> Self {
        Round {
            target,
            cards: cards.into_iter().map(Rational::from).collect(),
            selected: Vec::new(),
            phase: Phase::Playing,
            history: Vec::new(),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Take back the last merge, restoring the cards, selection and phase it
    /// had just before. Returns `false` if there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        match self.history.pop() {
            Some(snapshot) => {
                self.cards = snapshot.cards;
                self.selected = snapshot.selected;
                self.phase = snapshot.phase;
                true
            }
            None => false,
        }
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    /// Select or deselect a card. Ignored once the round is over, and never
    /// keeps more than two cards selected.
    pub fn toggle(&mut self, index: usize) {
        if self.phase != Phase::Playing || index >= self.cards.len() {
            return;
        }
        if let Some(pos) = self.selected.iter().position(|&i| i == index) {
            self.selected.remove(pos);
        } else if self.selected.len() < 2 {
            self.selected.push(index);
        }
    }

    pub fn merge(&mut self, op: Op) -> Result<(), MergeError> {
        if self.selected.len() != 2 {
            return Err(MergeError::NeedTwoCards);
        }
        let left = self.cards[self.selected[0]];
        let right = self.cards[self.selected[1]];
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
            selected: self.selected.clone(),
            phase: self.phase,
        });

        let mut indices = self.selected.clone();
        indices.sort_unstable();
        let (low, high) = (indices[0], indices[1]);
        self.cards.remove(high);
        self.cards.remove(low);
        self.cards.insert(low, value);
        self.selected.clear();

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
    fn selection_toggles_and_caps_at_two() {
        let mut r = round(&[1, 2, 3], 6);
        r.toggle(0);
        assert!(r.is_selected(0));
        r.toggle(1);
        r.toggle(2); // third selection ignored
        assert_eq!(r.selected, vec![0, 1]);
        r.toggle(0); // deselect
        assert_eq!(r.selected, vec![1]);
    }

    #[test]
    fn merge_needs_two_cards() {
        let mut r = round(&[1, 2, 3], 6);
        assert_eq!(r.merge(Op::Add), Err(MergeError::NeedTwoCards));
        r.toggle(0);
        assert_eq!(r.merge(Op::Add), Err(MergeError::NeedTwoCards));
    }

    #[test]
    fn merge_replaces_two_cards_with_one() {
        let mut r = round(&[1, 2, 3], 6);
        r.toggle(0);
        r.toggle(1);
        r.merge(Op::Add).unwrap();
        assert_eq!(r.cards, vec![Rational::from(3), Rational::from(3)]);
        assert!(r.selected.is_empty());
        assert_eq!(r.phase, Phase::Playing);
    }

    #[test]
    fn subtraction_respects_click_order() {
        let mut r = round(&[9, 4], 5);
        r.toggle(0); // 9
        r.toggle(1); // 4
        r.merge(Op::Sub).unwrap();
        assert_eq!(r.cards, vec![Rational::from(5)]);
        assert_eq!(r.phase, Phase::Won);

        let mut r = round(&[9, 4], 5);
        r.toggle(1); // 4 first
        r.toggle(0); // then 9
        r.merge(Op::Sub).unwrap();
        assert_eq!(r.cards, vec![Rational::from(-5)]);
        assert_eq!(r.phase, Phase::Lost);
    }

    #[test]
    fn division_by_zero_is_blocked() {
        let mut r = round(&[6, 0], 6);
        r.toggle(0);
        r.toggle(1);
        assert_eq!(r.merge(Op::Div), Err(MergeError::DivideByZero));
        // Nothing changed, so the player can try another operator.
        assert_eq!(r.cards.len(), 2);
        assert_eq!(r.selected, vec![0, 1]);
    }

    #[test]
    fn exact_fractions_are_kept() {
        let mut r = round(&[1, 2], 2);
        r.toggle(0);
        r.toggle(1); // 1 / 2
        r.merge(Op::Div).unwrap();
        assert_eq!(r.cards, vec![Rational::new(1, 2)]);
        assert_eq!(Round::label(&r.cards[0]), "1/2");
        assert_eq!(r.phase, Phase::Lost);
    }

    #[test]
    fn win_and_lose_on_final_value() {
        let mut win = round(&[9, 1, 3], 24);
        win.toggle(0);
        win.toggle(1);
        win.merge(Op::Sub).unwrap(); // (9-1) = 8 ; cards [8, 3]
        win.toggle(0);
        win.toggle(1);
        win.merge(Op::Mul).unwrap(); // 24
        assert_eq!(win.phase, Phase::Won);

        let mut lose = round(&[9, 1, 3], 24);
        lose.toggle(0);
        lose.toggle(1);
        lose.merge(Op::Add).unwrap(); // 10 ; cards [10, 3]
        lose.toggle(0);
        lose.toggle(1);
        lose.merge(Op::Mul).unwrap(); // 30
        assert_eq!(lose.cards, vec![Rational::from(30)]);
        assert_eq!(lose.phase, Phase::Lost);
    }

    #[test]
    fn undo_takes_back_a_wrong_merge() {
        let mut r = round(&[9, 1, 3], 24);
        assert!(!r.can_undo());
        r.toggle(0);
        r.toggle(1);
        r.merge(Op::Add).unwrap(); // wrong: 9 + 1 = 10
        assert!(r.can_undo());
        assert_eq!(r.cards, vec![Rational::from(10), Rational::from(3)]);
        assert!(r.undo());
        assert_eq!(r.cards, vec![Rational::from(9), Rational::from(1), Rational::from(3)]);
        // The two cards are re-selected, so the operator can be changed directly.
        assert_eq!(r.selected, vec![0, 1]);
        assert!(!r.can_undo());
    }

    #[test]
    fn undo_retries_wrong_combinations_to_a_win() {
        let mut r = round(&[9, 1, 3], 24);
        r.toggle(0);
        r.toggle(1);
        r.merge(Op::Add).unwrap(); // wrong start: 10
        r.toggle(0);
        r.toggle(1);
        r.merge(Op::Mul).unwrap(); // 30 -> Lost
        assert_eq!(r.phase, Phase::Lost);

        assert!(r.undo()); // back to 10, 3 with both re-selected
        assert_eq!(r.phase, Phase::Playing);
        assert!(r.undo()); // back to 9, 1, 3 with both re-selected
        assert_eq!(r.cards, vec![Rational::from(9), Rational::from(1), Rational::from(3)]);

        r.merge(Op::Sub).unwrap(); // (9 - 1) = 8
        r.toggle(0);
        r.toggle(1);
        r.merge(Op::Mul).unwrap(); // 8 * 3 = 24 -> Won
        assert_eq!(r.phase, Phase::Won);
    }

    #[test]
    fn undo_unwinds_multiple_merges() {
        let mut r = round(&[9, 1, 3], 24);
        r.toggle(0);
        r.toggle(1);
        r.merge(Op::Sub).unwrap(); // 8
        r.toggle(0);
        r.toggle(1);
        r.merge(Op::Mul).unwrap(); // 24 -> Won
        assert_eq!(r.phase, Phase::Won);
        assert!(r.undo());
        assert_eq!(r.cards, vec![Rational::from(8), Rational::from(3)]);
        assert_eq!(r.phase, Phase::Playing);
        assert!(r.undo());
        assert_eq!(r.cards, vec![Rational::from(9), Rational::from(1), Rational::from(3)]);
        assert!(!r.can_undo());
        assert!(!r.undo());
    }

    #[test]
    fn selection_changes_do_not_create_undo_history() {
        let mut r = round(&[1, 2, 3], 6);
        r.toggle(0);
        r.toggle(1);
        r.toggle(0);
        assert!(!r.can_undo());
        // A blocked merge also must not become undoable.
        let mut r = round(&[6, 0], 6);
        r.toggle(0);
        r.toggle(1);
        assert_eq!(r.merge(Op::Div), Err(MergeError::DivideByZero));
        assert!(!r.can_undo());
    }

    #[test]
    fn input_is_ignored_after_the_round_ends() {
        let mut r = round(&[9, 1], 8);
        r.toggle(0);
        r.toggle(1);
        r.merge(Op::Sub).unwrap();
        assert_eq!(r.phase, Phase::Won);
        r.toggle(0);
        assert!(r.selected.is_empty());
        assert_eq!(r.merge(Op::Add), Err(MergeError::NeedTwoCards));
    }
}
