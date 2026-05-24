//! Expression-evaluation outcomes.
//!
//! Every expression evaluation either returns a [`Value`] (`Normal`) or
//! propagates a JavaScript-level exception value (`Throw`).  Both forms
//! are paired with the updated [`Heap`] and [`Fuel`] state.
//!
//! Engine-fatal errors (out-of-fuel, unsupported AST) are returned via
//! [`Error`] (`Result::Err`), not via this enum.
//!
//! [`Heap`]: crate::heap::Heap
//! [`Fuel`]: crate::fuel::Fuel
//! [`Error`]: crate::error::Error

use crate::fuel::Fuel;
use crate::heap::Heap;
use crate::value::Value;

/// The outcome of evaluating an expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Normal completion carrying a value.
    Normal(Value),
    /// `throw value`.
    Throw(Value),
}

impl Outcome {
    /// Map the value under `Normal`, leaving `Throw` untouched.
    #[must_use]
    pub fn map_normal(self, f: impl FnOnce(Value) -> Value) -> Self {
        match self {
            Self::Normal(v) => Self::Normal(f(v)),
            Self::Throw(v) => Self::Throw(v),
        }
    }
}

/// Result of an expression evaluation step.
pub type EvalResult = Result<(Outcome, Heap, Fuel), crate::error::Error>;

/// Continue from a normal outcome; propagate a throw.
///
/// Performs the common monadic-bind on `EvalResult`: when `result` is a
/// fatal error, propagate it; when it's a throw, propagate the throw with
/// updated `Heap`/`Fuel`; when it's a normal value, hand it to `k`.
///
/// # Errors
///
/// Propagates any [`Error`] from `result` or from `k`.
///
/// [`Error`]: crate::error::Error
pub fn step<F>(result: EvalResult, k: F) -> EvalResult
where
    F: FnOnce(Value, Heap, Fuel) -> EvalResult,
{
    result.and_then(|(outcome, heap, fuel)| match outcome {
        Outcome::Throw(v) => Ok((Outcome::Throw(v), heap, fuel)),
        Outcome::Normal(v) => k(v, heap, fuel),
    })
}
