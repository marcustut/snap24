//! Snap 24 core: exact-rational solver over a dealt hand of cards.
//!
//! Arithmetic is exact ([`Rational`]), so there is no float tolerance anywhere.
//! Solutions are canonicalized per `docs/canonical-form.md` (version `v1`) and
//! returned as canonical serializations; see [`solve`].

mod generator;
mod submission;

pub use generator::{generate, generate_targeted, Difficulty, Mode, Puzzle, Rng};
pub use submission::{evaluate, EvalError};

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
use std::str::FromStr;

// --------------------------------------------------------------------------- //
// exact rational arithmetic                                                    //
// --------------------------------------------------------------------------- //

/// An exact rational number in lowest terms with a positive denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rational {
    num: i64,
    den: i64,
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

// Inherent arithmetic is intentional: `div` returns `Option` rather than
// panicking, so it can't be the `std::ops::Div` trait method.
#[allow(clippy::should_implement_trait)]
impl Rational {
    pub fn new(num: i64, den: i64) -> Self {
        assert!(den != 0, "zero denominator");
        let g = gcd(num, den);
        let g = if g == 0 { 1 } else { g };
        let sign = if den < 0 { -1 } else { 1 };
        let result = Rational {
            num: sign * (num / g),
            den: (den / g).abs(),
        };
        debug_assert!(gcd(result.num, result.den) == 1 || result.num == 0);
        debug_assert!(result.den > 0);
        result
    }

    pub fn add(self, other: Self) -> Self {
        Rational::new(
            (self.num as i128 * other.den as i128 + other.num as i128 * self.den as i128) as i64,
            (self.den as i128 * other.den as i128) as i64,
        )
    }

    pub fn sub(self, other: Self) -> Self {
        Rational::new(
            (self.num as i128 * other.den as i128 - other.num as i128 * self.den as i128) as i64,
            (self.den as i128 * other.den as i128) as i64,
        )
    }

    pub fn mul(self, other: Self) -> Self {
        Rational::new(
            (self.num as i128 * other.num as i128) as i64,
            (self.den as i128 * other.den as i128) as i64,
        )
    }

    /// Returns `None` when `other` is zero.
    pub fn div(self, other: Self) -> Option<Self> {
        if other.num == 0 {
            return None;
        }
        Some(Rational::new(
            (self.num as i128 * other.den as i128) as i64,
            (self.den as i128 * other.num as i128) as i64,
        ))
    }

    pub fn is_zero(self) -> bool {
        self.num == 0
    }

    pub fn is_integer(self) -> bool {
        self.den == 1
    }

    pub fn is_positive(self) -> bool {
        self.num > 0
    }

    /// The value as an `i64`, if it is a whole number.
    pub fn as_i64(self) -> Option<i64> {
        (self.den == 1).then_some(self.num)
    }
}

impl From<i64> for Rational {
    fn from(value: i64) -> Self {
        Rational { num: value, den: 1 }
    }
}

impl FromStr for Rational {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('/') {
            Some((n, d)) => {
                let num = n.parse::<i64>().map_err(|e| e.to_string())?;
                let den = d.parse::<i64>().map_err(|e| e.to_string())?;
                if den == 0 {
                    return Err("zero denominator".into());
                }
                Ok(Rational::new(num, den))
            }
            None => Ok(Rational::from(
                s.parse::<i64>().map_err(|e| e.to_string())?,
            )),
        }
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.num as i128 * other.den as i128).cmp(&(other.num as i128 * self.den as i128))
    }
}

// --------------------------------------------------------------------------- //
// canonical expressions                                                        //
// --------------------------------------------------------------------------- //

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

impl Op {
    fn symbol(self) -> char {
        match self {
            Op::Add => '+',
            Op::Sub => '-',
            Op::Mul => '*',
            Op::Div => '/',
        }
    }

    fn is_commutative(self) -> bool {
        matches!(self, Op::Add | Op::Mul)
    }
}

/// A canonical expression tree. `+`/`*` nodes are n-ary, flattened and sorted
/// by serialization; `-`/`/` nodes stay binary and ordered. Children are `Rc`
/// so a tree can be reused by many parent nodes without deep cloning.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Expr {
    Leaf(Rational),
    Nary { op: Op, children: Vec<ExprRef> },
    Bin { op: Op, left: ExprRef, right: ExprRef },
}

type ExprRef = Rc<Expr>;

impl Expr {
    fn serialize(&self) -> String {
        match self {
            Expr::Leaf(v) => v.to_string(),
            Expr::Nary { op, children } => {
                let mut keys: Vec<String> = children.iter().map(|c| c.serialize()).collect();
                keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
                format!("({},{})", op.symbol(), keys.join(","))
            }
            Expr::Bin { op, left, right } => format!(
                "({},{},{})",
                op.symbol(),
                left.serialize(),
                right.serialize()
            ),
        }
    }

    /// Standard infix rendering (`+ - * /` and parentheses), suitable for
    /// [`evaluate`]. Unlike the canonical serialization this is what a player
    /// would type, so a solution can be re-validated through the evaluator.
    fn to_infix(&self) -> String {
        match self {
            Expr::Leaf(v) => v.to_string(),
            Expr::Nary { op, children } => {
                let parts: Vec<String> = children.iter().map(|c| c.to_infix()).collect();
                format!("({})", parts.join(&format!(" {} ", op.symbol())))
            }
            Expr::Bin { op, left, right } => {
                format!("({} {} {})", left.to_infix(), op.symbol(), right.to_infix())
            }
        }
    }

    /// The step-by-step merges that evaluate this tree, in an order a player
    /// could follow (`a op b = result`, repeatedly).
    fn steps(&self) -> Vec<Move> {
        match self {
            Expr::Leaf(_) => Vec::new(),
            Expr::Nary { op, children } => {
                let mut out = Vec::new();
                let mut iter = children.iter();
                let Some(first) = iter.next() else {
                    return out;
                };
                out.extend(first.steps());
                let mut acc = first.value();
                for child in iter {
                    out.extend(child.steps());
                    let right = child.value();
                    let result = match op {
                        Op::Add => acc.add(right),
                        Op::Mul => acc.mul(right),
                        _ => unreachable!("n-ary nodes are + or *"),
                    };
                    out.push(Move {
                        left: acc,
                        right,
                        op: op.symbol(),
                        result,
                    });
                    acc = result;
                }
                out
            }
            Expr::Bin { op, left, right } => {
                let mut out = left.steps();
                out.extend(right.steps());
                let l = left.value();
                let r = right.value();
                let result = match op {
                    Op::Sub => l.sub(r),
                    Op::Div => l.div(r).expect("division by zero"),
                    _ => unreachable!("binary nodes are - or /"),
                };
                out.push(Move {
                    left: l,
                    right: r,
                    op: op.symbol(),
                    result,
                });
                out
            }
        }
    }

    fn value(&self) -> Rational {
        match self {
            Expr::Leaf(v) => *v,
            Expr::Nary { op, children } => children.iter().map(|c| c.value()).fold(
                match op {
                    Op::Add => Rational::from(0),
                    Op::Mul => Rational::from(1),
                    _ => unreachable!("n-ary nodes are + or *"),
                },
                |acc, v| match op {
                    Op::Add => acc.add(v),
                    Op::Mul => acc.mul(v),
                    _ => unreachable!("n-ary nodes are + or *"),
                },
            ),
            Expr::Bin { op, left, right } => match op {
                Op::Sub => left.value().sub(right.value()),
                Op::Div => left.value().div(right.value()).expect("division by zero"),
                _ => unreachable!("binary nodes are - or /"),
            },
        }
    }

    #[cfg(test)]
    fn leaves(&self, out: &mut Vec<i64>) {
        match self {
            Expr::Leaf(v) => out.push(v.as_i64().expect("card leaves are integers")),
            Expr::Nary { children, .. } => children.iter().for_each(|c| c.leaves(out)),
            Expr::Bin { left, right, .. } => {
                left.leaves(out);
                right.leaves(out);
            }
        }
    }
}

fn combine(op: Op, a: &ExprRef, b: &ExprRef) -> ExprRef {
    if op.is_commutative() {
        let mut children: Vec<ExprRef> = Vec::new();
        for node in [a, b] {
            match node.as_ref() {
                Expr::Nary { op: inner, children: inner_children } if *inner == op => {
                    children.extend(inner_children.iter().cloned())
                }
                _ => children.push(Rc::clone(node)),
            }
        }
        children.sort_by_cached_key(|c| c.serialize());
        Rc::new(Expr::Nary { op, children })
    } else {
        Rc::new(Expr::Bin {
            op,
            left: Rc::clone(a),
            right: Rc::clone(b),
        })
    }
}

/// Combine two already-canonical `(value, expression)` operands by `op`,
/// returning `(value, expression)` results. For `-` and `/` both operand
/// directions are produced because the order decides the value. Values are
/// passed in so no tree is re-evaluated here.
fn combine_both(
    op: Op,
    a: (Rational, &ExprRef),
    b: (Rational, &ExprRef),
) -> Vec<(Rational, ExprRef)> {
    let (va, ea) = a;
    let (vb, eb) = b;
    match op {
        Op::Add => vec![(va.add(vb), combine(Op::Add, ea, eb))],
        Op::Mul => vec![(va.mul(vb), combine(Op::Mul, ea, eb))],
        Op::Sub => vec![
            (va.sub(vb), combine(Op::Sub, ea, eb)),
            (vb.sub(va), combine(Op::Sub, eb, ea)),
        ],
        Op::Div => {
            let mut out = Vec::new();
            if !vb.is_zero() {
                out.push((va.div(vb).unwrap(), combine(Op::Div, ea, eb)));
            }
            if !va.is_zero() {
                out.push((vb.div(va).unwrap(), combine(Op::Div, eb, ea)));
            }
            out
        }
    }
}

const OPS: [Op; 4] = [Op::Add, Op::Sub, Op::Mul, Op::Div];

// --------------------------------------------------------------------------- //
// solver                                                                       //
// --------------------------------------------------------------------------- //

/// All values exactly reachable using every dealt card exactly once.
pub fn reachable(cards: &[i64]) -> BTreeSet<Rational> {
    reachable_values(&cards.iter().copied().map(Rational::from).collect::<Vec<_>>())
}

/// [`reachable`] over arbitrary (possibly already-merged) board values.
pub fn reachable_values(cards: &[Rational]) -> BTreeSet<Rational> {
    let n = cards.len();
    if n == 0 {
        return BTreeSet::new();
    }
    let mut dp: Vec<HashSet<Rational>> = vec![HashSet::new(); 1 << n];
    for (i, card) in cards.iter().enumerate() {
        dp[1 << i].insert(*card);
    }
    for mask in 1usize..(1 << n) {
        if mask.count_ones() < 2 {
            continue;
        }
        let low = mask.isolate_lowest_one();
        let mut values = HashSet::new();
        let mut sub = (mask - 1) & mask;
        while sub > 0 {
            let other = mask ^ sub;
            if other != 0 && sub & low != 0 {
                for a in &dp[sub] {
                    for b in &dp[other] {
                        let va = *a;
                        let vb = *b;
                        values.insert(va.add(vb));
                        values.insert(va.sub(vb));
                        values.insert(vb.sub(va));
                        values.insert(va.mul(vb));
                        if !vb.is_zero() {
                            values.insert(va.div(vb).unwrap());
                        }
                        if !va.is_zero() {
                            values.insert(vb.div(va).unwrap());
                        }
                    }
                }
            }
            sub = (sub - 1) & mask;
        }
        dp[mask] = values;
    }
    dp[(1 << n) - 1].iter().copied().collect()
}

/// Whether `target` is exactly reachable using every dealt card once.
///
/// Unlike [`solve`] this never builds expression trees, so it is the cheap
/// "can this be won?" check.
pub fn is_solvable(cards: &[i64], target: impl Into<Rational>) -> bool {
    is_solvable_values(
        &cards.iter().copied().map(Rational::from).collect::<Vec<_>>(),
        target.into(),
    )
}

/// [`is_solvable`] over arbitrary (possibly already-merged) board values.
pub fn is_solvable_values(cards: &[Rational], target: Rational) -> bool {
    reachable_values(cards).contains(&target)
}

fn solve_exprs(cards: &[Rational], target: Rational) -> Vec<ExprRef> {
    let n = cards.len();
    if n == 0 {
        return Vec::new();
    }
    // dp[mask]: value -> set of canonical expressions. Dedup is structural
    // (the canonical form is unique), so no serialization happens in the hot
    // loop.
    let mut dp: Vec<HashMap<Rational, HashSet<ExprRef>>> = vec![HashMap::new(); 1 << n];
    for (i, card) in cards.iter().enumerate() {
        dp[1 << i]
            .entry(*card)
            .or_default()
            .insert(Rc::new(Expr::Leaf(*card)));
    }
    for mask in 1usize..(1 << n) {
        if mask.count_ones() < 2 {
            continue;
        }
        let low = mask.isolate_lowest_one();
        let mut acc: HashMap<Rational, HashSet<ExprRef>> = HashMap::new();
        let mut sub = (mask - 1) & mask;
        while sub > 0 {
            let other = mask ^ sub;
            if other != 0 && sub & low != 0 && !dp[sub].is_empty() && !dp[other].is_empty() {
                let left_values: Vec<(Rational, ExprRef)> = dp[sub]
                    .iter()
                    .flat_map(|(v, m)| m.iter().map(|e| (*v, Rc::clone(e))))
                    .collect();
                let right_values: Vec<(Rational, ExprRef)> = dp[other]
                    .iter()
                    .flat_map(|(v, m)| m.iter().map(|e| (*v, Rc::clone(e))))
                    .collect();
                for (va, a) in &left_values {
                    for (vb, b) in &right_values {
                        for op in OPS {
                            for (value, expr) in combine_both(op, (*va, a), (*vb, b)) {
                                acc.entry(value).or_default().insert(expr);
                            }
                        }
                    }
                }
            }
            sub = (sub - 1) & mask;
        }
        dp[mask] = acc;
    }
    let mut exprs: Vec<ExprRef> = dp[(1 << n) - 1]
        .get(&target)
        .map(|m| m.iter().cloned().collect())
        .unwrap_or_default();
    exprs.sort_by_cached_key(|e| e.serialize());
    exprs
}

/// Every distinct canonical solution to `cards == target`, as canonical
/// serializations, sorted byte-wise. Duplicate trees that differ only by
/// commutativity/associativity are returned once (see `docs/canonical-form.md`).
pub fn solve(cards: &[i64], target: impl Into<Rational>) -> Vec<String> {
    solve_values(
        &cards.iter().copied().map(Rational::from).collect::<Vec<_>>(),
        target.into(),
    )
}

/// [`solve`] over arbitrary (possibly already-merged) board values.
pub fn solve_values(cards: &[Rational], target: Rational) -> Vec<String> {
    solve_exprs(cards, target)
        .iter()
        .map(|e| e.serialize())
        .collect()
}

/// Distinct solutions rendered as standard infix expressions, so each can be
/// checked by feeding it back through [`evaluate`].
pub fn solutions_infix(cards: &[Rational], target: Rational) -> Vec<String> {
    solve_exprs(cards, target)
        .iter()
        .map(|e| e.to_infix())
        .collect()
}

/// Distinct solutions as step-by-step moves, the way a player would read them.
pub fn solutions_steps(cards: &[Rational], target: Rational) -> Vec<Vec<Move>> {
    solve_exprs(cards, target)
        .iter()
        .map(|e| e.steps())
        .collect()
}

/// One move: combine `left` and `right` with `op` to get `result`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub left: Rational,
    pub right: Rational,
    pub op: char,
    pub result: Rational,
}

/// A first move that keeps the board on a path to `target`, if one exists.
///
/// Searches card pairs and operators and returns the first move whose resulting
/// board is still solvable, so a hint built from it can never send the player
/// somewhere unwinnable.
pub fn first_move(cards: &[Rational], target: Rational) -> Option<Move> {
    for i in 0..cards.len() {
        for j in (i + 1)..cards.len() {
            for (left, right, op, result) in candidate_moves(cards[i], cards[j]) {
                let mut next: Vec<Rational> = cards
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| *k != i && *k != j)
                    .map(|(_, value)| *value)
                    .collect();
                next.push(result);
                if is_solvable_values(&next, target) {
                    return Some(Move {
                        left,
                        right,
                        op,
                        result,
                    });
                }
            }
        }
    }
    None
}

/// The full sequence of moves from `cards` to `target`, each step chosen by
/// [`first_move`]. Empty if the board cannot reach the target.
pub fn move_sequence(cards: &[Rational], target: Rational) -> Vec<Move> {
    let mut current: Vec<Rational> = cards.to_vec();
    let mut moves = Vec::new();
    while current.len() > 1 {
        let Some(next) = first_move(&current, target) else {
            break;
        };
        remove_one(&mut current, next.left);
        remove_one(&mut current, next.right);
        current.push(next.result);
        moves.push(next);
    }
    moves
}

/// `(left, right, operator, result)` candidates from combining `a` and `b`,
/// including both operand orders for `-` and `/`. The operand order matches the
/// result, so a move renders as `left <op> right = result`. Division by zero is
/// skipped.
fn candidate_moves(a: Rational, b: Rational) -> Vec<(Rational, Rational, char, Rational)> {
    let mut out = vec![
        (a, b, '+', a.add(b)),
        (a, b, '-', a.sub(b)),
        (b, a, '-', b.sub(a)),
        (a, b, '*', a.mul(b)),
    ];
    if !b.is_zero() {
        out.push((a, b, '/', a.div(b).unwrap()));
    }
    if !a.is_zero() {
        out.push((b, a, '/', b.div(a).unwrap()));
    }
    out
}

fn remove_one(values: &mut Vec<Rational>, value: Rational) {
    if let Some(pos) = values.iter().position(|v| *v == value) {
        values.remove(pos);
    }
}

// --------------------------------------------------------------------------- //
// tests                                                                        //
// --------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny deterministic LCG so random-hand properties are reproducible
    /// without pulling in a rand dependency.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }

        fn hand(&mut self, n: usize, max: u64) -> Vec<i64> {
            (0..n).map(|_| (self.next() % max + 1) as i64).collect()
        }
    }

    fn values(cards: &[i64]) -> Vec<Rational> {
        cards.iter().copied().map(Rational::from).collect()
    }

    fn apply(left: Rational, right: Rational, op: char) -> Option<Rational> {
        match op {
            '+' => Some(left.add(right)),
            '-' => Some(left.sub(right)),
            '*' => Some(left.mul(right)),
            '/' => left.div(right),
            _ => None,
        }
    }

    fn assert_sound_and_unique(cards: &[i64], target: Rational) {
        let exprs = solve_exprs(&values(cards), target);
        let mut keys = HashSet::new();
        let mut sorted_cards = cards.to_vec();
        sorted_cards.sort();
        for expr in &exprs {
            assert_eq!(expr.value(), target, "unsound solution {expr:?}");
            let mut leaves = Vec::new();
            expr.leaves(&mut leaves);
            leaves.sort();
            assert_eq!(leaves, sorted_cards, "wrong card multiset in {expr:?}");
            assert!(keys.insert(expr.serialize()), "duplicate solution {expr:?}");
        }
    }

    #[test]
    fn spec_worked_example() {
        assert_eq!(solve(&[1, 1, 1, 1, 8], 24).len(), 10);
    }

    #[test]
    fn unsolvable_hand() {
        assert!(solve(&[9, 10, 9, 9, 1], 24).is_empty());
        assert!(!is_solvable(&[9, 10, 9, 9, 1], 24));
    }

    #[test]
    fn solve_is_sorted_and_deduped() {
        let solutions = solve(&[1, 1, 1, 1, 8], 24);
        let mut sorted = solutions.clone();
        sorted.sort();
        assert_eq!(solutions, sorted);
        assert_eq!(solutions.iter().collect::<HashSet<_>>().len(), solutions.len());
    }

    #[test]
    fn rendered_solutions_revalidate_through_the_evaluator() {
        let cases: &[(&[i64], Rational)] = &[
            (&[1, 1, 1, 1, 8], Rational::from(24)),
            (&[3, 3, 8, 8], Rational::from(24)),
            (&[1, 1, 3, 4], Rational::new(7, 2)),
        ];
        for (cards, target) in cases {
            let solutions = solutions_infix(&values(cards), *target);
            assert_eq!(solutions.len(), solve(cards, *target).len());
            assert!(!solutions.is_empty());
            for infix in &solutions {
                assert_eq!(evaluate(cards, infix), Ok(*target), "revalidate {infix:?}");
            }
        }
    }

    #[test]
    fn solution_steps_are_valid_step_by_step() {
        let cards = values(&[1, 1, 1, 1, 8]);
        let target = Rational::from(24);
        let solutions = solutions_steps(&cards, target);
        assert_eq!(solutions.len(), solve(&[1, 1, 1, 1, 8], 24).len());
        for steps in &solutions {
            assert_eq!(steps.len(), 4);
            let mut current = cards.clone();
            for step in steps {
                assert_eq!(apply(step.left, step.right, step.op), Some(step.result));
                remove_one(&mut current, step.left);
                remove_one(&mut current, step.right);
                current.push(step.result);
            }
            assert_eq!(current, vec![target]);
        }
    }

    #[test]
    fn move_sequence_reaches_the_target() {
        let cards = values(&[1, 1, 1, 1, 8]);
        let target = Rational::from(24);
        let moves = move_sequence(&cards, target);
        assert_eq!(moves.len(), 4, "one merge per extra card");
        let mut current = cards.clone();
        for step in &moves {
            assert!(
                candidate_moves(step.left, step.right).iter().any(
                    |(left, right, op, result)| *left == step.left
                        && *right == step.right
                        && *op == step.op
                        && *result == step.result
                ),
                "invalid step {step:?}"
            );
            assert_eq!(
                apply(step.left, step.right, step.op),
                Some(step.result),
                "equation does not hold {step:?}"
            );
            remove_one(&mut current, step.left);
            remove_one(&mut current, step.right);
            current.push(step.result);
        }
        assert_eq!(current, vec![target]);
    }

    #[test]
    fn first_move_keeps_the_board_solvable_and_none_when_hopeless() {
        let cards = values(&[1, 1, 1, 1, 8]);
        let target = Rational::from(24);
        let step = first_move(&cards, target).expect("solvable hand has a first move");
        let mut next = cards.clone();
        remove_one(&mut next, step.left);
        remove_one(&mut next, step.right);
        next.push(step.result);
        assert!(is_solvable_values(&next, target));
        assert!(first_move(&values(&[9, 10, 9, 9, 1]), target).is_none());
    }

    #[test]
    fn hints_work_from_a_mid_game_board() {
        let cards = values(&[1, 1, 1, 1, 8]);
        let target = Rational::from(24);
        let first = first_move(&cards, target).expect("solvable");
        let mut board = cards.clone();
        remove_one(&mut board, first.left);
        remove_one(&mut board, first.right);
        board.push(first.result); // a real 4-card mid-game board

        let step = first_move(&board, target).expect("mid-game board is solvable");
        let mut next = board.clone();
        remove_one(&mut next, step.left);
        remove_one(&mut next, step.right);
        next.push(step.result);
        assert!(is_solvable_values(&next, target));
        assert_eq!(move_sequence(&board, target).last().unwrap().result, target);
    }

    #[test]
    fn exact_rational_target() {
        assert_eq!(solve(&[1, 1, 3, 4], Rational::new(7, 2)).len(), 4);
        assert!(is_solvable(&[1, 1, 3, 4], Rational::new(7, 3)));
        assert!(!is_solvable(&[1, 1, 3, 4], Rational::new(5, 7)));
    }

    #[test]
    fn reachable_matches_solve_membership() {
        let cards = [3, 3, 8, 8];
        let values = reachable(&cards);
        assert!(values.contains(&Rational::from(24)));
        assert_eq!(solve(&cards, 24).len(), 1);
    }

    #[test]
    fn property_random_hands() {
        let mut rng = Lcg(0x5eed_1234_abcd_0001);
        for n in 3..=5 {
            let iterations = if n == 5 { 8 } else { 20 };
            for _ in 0..iterations {
                let cards = rng.hand(n, 6);
                let values = reachable(&cards);
                // Unsolvable target: result must be empty.
                assert_sound_and_unique(&cards, Rational::from(97));
                // A reachable target: result must be non-empty, sound and unique.
                if let Some(&target) = values.iter().next() {
                    assert_sound_and_unique(&cards, target);
                }
            }
        }
    }

    #[test]
    fn reachable_agrees_with_solve() {
        let mut rng = Lcg(0x0bad_c0de_0000_0001);
        for n in 3..=5 {
            for _ in 0..10 {
                let cards = rng.hand(n, 6);
                let values = reachable(&cards);
                let target = Rational::from(24);
                assert_eq!(is_solvable(&cards, target), values.contains(&target));
                assert_eq!(solve(&cards, target).is_empty(), !values.contains(&target));
            }
        }
    }
}
