//! Parsing and validation of player-submitted expressions.
//!
//! This is the trust boundary: a client's "I solved it" is never trusted, only
//! the expression text is. [`evaluate`] parses `+ - * /`, parentheses and
//! multi-digit numbers with exact arithmetic, then checks that the expression
//! uses exactly the dealt cards once each before returning the value.

use crate::Rational;
use std::fmt;

/// Why a submitted expression was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    /// The input was empty or only whitespace.
    Empty,
    /// A character that cannot start a factor (not a digit or `(`).
    UnexpectedChar(char),
    /// A number was expected but the input ended.
    UnexpectedEnd,
    /// A number literal would not fit in `i64`.
    NumberTooLarge,
    /// Closing parenthesis without a matching open one.
    UnexpectedRParen,
    /// An open parenthesis was never closed.
    MissingRParen,
    /// Input remained after a complete expression (e.g. `1 2`).
    TrailingInput,
    /// An operation divided by an exact zero.
    DivisionByZero,
    /// The expression's card values are not exactly the dealt multiset.
    WrongCards { used: Vec<i64>, dealt: Vec<i64> },
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Empty => write!(f, "empty expression"),
            EvalError::UnexpectedChar(c) => write!(f, "unexpected character {c:?}"),
            EvalError::UnexpectedEnd => write!(f, "unexpected end of expression"),
            EvalError::NumberTooLarge => write!(f, "number is too large"),
            EvalError::UnexpectedRParen => write!(f, "unmatched ')'"),
            EvalError::MissingRParen => write!(f, "missing ')'"),
            EvalError::TrailingInput => write!(f, "unexpected input after expression"),
            EvalError::DivisionByZero => write!(f, "division by zero"),
            EvalError::WrongCards { used, dealt } => write!(
                f,
                "expression uses cards {used:?} but {dealt:?} were dealt"
            ),
        }
    }
}

impl std::error::Error for EvalError {}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    leaves: Vec<i64>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Parser {
            input,
            pos: 0,
            leaves: Vec::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.bump();
        }
    }

    fn expression(&mut self) -> Result<Rational, EvalError> {
        let mut value = self.term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.bump();
                    value = value.add(self.term()?);
                }
                Some('-') => {
                    self.bump();
                    value = value.sub(self.term()?);
                }
                _ => return Ok(value),
            }
        }
    }

    fn term(&mut self) -> Result<Rational, EvalError> {
        let mut value = self.factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.bump();
                    value = value.mul(self.factor()?);
                }
                Some('/') => {
                    self.bump();
                    let rhs = self.factor()?;
                    value = value.div(rhs).ok_or(EvalError::DivisionByZero)?;
                }
                _ => return Ok(value),
            }
        }
    }

    fn factor(&mut self) -> Result<Rational, EvalError> {
        self.skip_ws();
        match self.peek() {
            Some('(') => {
                self.bump();
                let value = self.expression()?;
                self.skip_ws();
                match self.bump() {
                    Some(')') => Ok(value),
                    _ => Err(EvalError::MissingRParen),
                }
            }
            Some(c) if c.is_ascii_digit() => {
                let start = self.pos;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.bump();
                }
                let digits = &self.input[start..self.pos];
                let value = digits.parse::<i64>().map_err(|_| EvalError::NumberTooLarge)?;
                self.leaves.push(value);
                Ok(Rational::from(value))
            }
            Some(')') => Err(EvalError::UnexpectedRParen),
            Some(c) => Err(EvalError::UnexpectedChar(c)),
            None => Err(EvalError::UnexpectedEnd),
        }
    }
}

fn sorted(mut values: Vec<i64>) -> Vec<i64> {
    values.sort_unstable();
    values
}

/// Parse and validate a submission against the dealt `cards`.
///
/// Returns the exact value of the expression, or a typed [`EvalError`] if the
/// input is malformed, divides by zero, or does not use exactly the dealt cards
/// once each. The caller accepts the submission iff the value equals the target.
pub fn evaluate(cards: &[i64], input: &str) -> Result<Rational, EvalError> {
    if input.trim().is_empty() {
        return Err(EvalError::Empty);
    }
    let mut parser = Parser::new(input);
    let value = parser.expression()?;
    parser.skip_ws();
    if parser.pos != input.len() {
        return Err(EvalError::TrailingInput);
    }
    let used = sorted(parser.leaves);
    let dealt = sorted(cards.to_vec());
    if used != dealt {
        return Err(EvalError::WrongCards { used, dealt });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_and_parentheses() {
        assert_eq!(evaluate(&[1, 2, 3], "1 + 2 * 3").unwrap(), Rational::from(7));
        assert_eq!(evaluate(&[1, 2, 3], "(1 + 2) * 3").unwrap(), Rational::from(9));
        assert_eq!(evaluate(&[8, 5, 3], "8 - 5 - 3").unwrap(), Rational::from(0));
    }

    #[test]
    fn exact_division() {
        assert_eq!(
            evaluate(&[1, 5, 3], "5 / (3 - 1)").unwrap(),
            Rational::from(5).div(Rational::from(2)).unwrap()
        );
    }

    #[test]
    fn multi_digit_and_whitespace() {
        assert_eq!(evaluate(&[13, 11], "13 - 11").unwrap(), Rational::from(2));
        assert_eq!(evaluate(&[13, 11], "  13-11  ").unwrap(), Rational::from(2));
    }

    #[test]
    fn malformed_inputs() {
        assert_eq!(evaluate(&[], ""), Err(EvalError::Empty));
        assert_eq!(evaluate(&[], "   "), Err(EvalError::Empty));
        assert_eq!(evaluate(&[1], "1 +"), Err(EvalError::UnexpectedEnd));
        assert_eq!(evaluate(&[1, 2], "1 + 2 +"), Err(EvalError::UnexpectedEnd));
        assert_eq!(evaluate(&[1, 2], "(1 + 2"), Err(EvalError::MissingRParen));
        assert_eq!(evaluate(&[1, 2], "1 + 2)"), Err(EvalError::TrailingInput));
        assert_eq!(evaluate(&[1, 2], "1 + a"), Err(EvalError::UnexpectedChar('a')));
        assert_eq!(evaluate(&[1, 2], "1 2"), Err(EvalError::TrailingInput));
        assert_eq!(evaluate(&[1], "* 1"), Err(EvalError::UnexpectedChar('*')));
        assert_eq!(evaluate(&[1], ")"), Err(EvalError::UnexpectedRParen));
    }

    #[test]
    fn division_by_zero() {
        assert_eq!(evaluate(&[1, 0], "1 / 0"), Err(EvalError::DivisionByZero));
        assert_eq!(evaluate(&[1, 2], "1 / (2 - 2)"), Err(EvalError::DivisionByZero));
    }

    #[test]
    fn wrong_multiset() {
        // Uses a value that was never dealt.
        assert_eq!(
            evaluate(&[1, 2, 3], "1 + 2 + 4"),
            Err(EvalError::WrongCards {
                used: vec![1, 2, 4],
                dealt: vec![1, 2, 3],
            })
        );
        // Uses a dealt value too many times and omits another.
        assert_eq!(
            evaluate(&[1, 2, 3], "1 + 2 + 2"),
            Err(EvalError::WrongCards {
                used: vec![1, 2, 2],
                dealt: vec![1, 2, 3],
            })
        );
        // Omits a card entirely.
        assert!(matches!(
            evaluate(&[1, 2, 3], "1 + 2"),
            Err(EvalError::WrongCards { .. })
        ));
    }

    #[test]
    fn accepts_correct_and_rejects_incorrect_values() {
        let cards = [9, 1, 3];
        let target = Rational::from(24);
        // Correct: (9 - 1) * 3 == 24.
        assert_eq!(evaluate(&cards, "(9 - 1) * 3").unwrap(), target);
        // Correct but not the target: still a valid expression.
        assert_ne!(evaluate(&cards, "9 - 1 - 3").unwrap(), target);
    }

    #[test]
    fn duplicate_ranks_must_match_exactly() {
        assert_eq!(evaluate(&[1, 1, 8], "(1 + 1) * 8").unwrap(), Rational::from(16));
        assert!(matches!(
            evaluate(&[1, 1, 8], "1 * 8"),
            Err(EvalError::WrongCards { .. })
        ));
    }
}
