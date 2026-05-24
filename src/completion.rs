//! Completion records: the spec's abrupt-completion mechanism, lifted into
//! a sum type the interpreter can return from statement evaluation.

use crate::value::Value;

/// The result of evaluating a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// Normal completion with an optional value (the result of an
    /// `Expression` statement, the body of a block, etc.).
    Normal(Value),
    /// `return value;`
    Return(Value),
    /// `throw value;`
    Throw(Value),
    /// `break;` (labeled break deferred to v0.2).
    Break,
    /// `continue;` (labeled continue deferred to v0.2).
    Continue,
}

impl Completion {
    /// Whether this completion is abrupt (anything other than `Normal`).
    #[must_use]
    pub fn is_abrupt(&self) -> bool {
        !matches!(self, Self::Normal(_))
    }
}
