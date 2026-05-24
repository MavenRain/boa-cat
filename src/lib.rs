//! Tree-walking ECMAScript engine.
//!
//! Consumes an [`ecma_syntax_cat::program::Program`] (or a source string
//! via [`run`]) and evaluates it to a [`Value`].  All state is threaded
//! immutably; the only mutable container is `comp-cat-rs`'s `Io`, which
//! brackets the engine's `run` catamorphism.
//!
//! # Examples
//!
//! ```
//! # fn main() -> Result<(), boa_cat::Error> {
//! use boa_cat::run;
//!
//! let value = run("let x = 1 + 2; x * 10").run()?;
//! assert_eq!(format!("{value}"), "30");
//! # Ok(())
//! # }
//! ```

#![cfg_attr(docsrs, feature(doc_auto_cfg))]
// State-threaded interpreter: persistent Heap and Fuel are passed by
// value so the caller hands ownership forward each step.  Pedantic lints
// that assume in-place mutation don't fit the model.
#![allow(clippy::similar_names)]
#![allow(clippy::needless_pass_by_value)]
// IEEE-754 equality matches ECMA-262 `===` semantics (NaN != NaN, +0 === -0).
#![allow(clippy::float_cmp)]
// Numeric conversions in operator semantics follow ToInt32/ToUint32/length
// rules; clippy's truncation/precision warnings flag them spuriously.
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
// Native callables are compared by fn-pointer address; this is good
// enough for equality (you get true iff you stored the same registry
// entry twice) and avoids ad-hoc PartialEq for the whole Value enum.
#![allow(unpredictable_function_pointer_comparisons)]

pub mod coercion;
pub mod completion;
pub mod env;
pub mod error;
pub mod expression;
pub mod fuel;
pub mod heap;
pub mod operator;
pub mod outcome;
pub mod statement;
pub mod value;

use comp_cat_rs::effect::io::Io;
use ecma_lex_cat::lex;
use ecma_parse_cat::parse_script;
use ecma_syntax_cat::program::{Program, ProgramKind};

pub use env::Binding;
pub use error::Error;
pub use value::{Cell, NativeFn, Value};

use crate::completion::Completion;
use crate::env::Env;
use crate::fuel::Fuel;
use crate::heap::Heap;
use crate::statement::eval_block;

/// Default step budget used by [`run`].
pub const DEFAULT_FUEL: u64 = 100_000;

/// Lex, parse, and evaluate `source` with the default fuel budget.
///
/// # Errors
///
/// See [`Error`].  An uncaught `throw` surfaces as
/// [`Error::UncaughtException`].
#[must_use]
pub fn run(source: &str) -> Io<Error, Value> {
    run_with_fuel(source, Fuel::new(DEFAULT_FUEL))
}

/// Lex, parse, and evaluate `source` with a caller-supplied fuel budget.
///
/// # Errors
///
/// See [`Error`].
#[must_use]
pub fn run_with_fuel(source: &str, fuel: Fuel) -> Io<Error, Value> {
    let owned = source.to_owned();
    Io::suspend(move || pipeline(&owned, fuel).map(|(value, _heap)| value))
}

/// Evaluate `source` and return the final value, heap, and remaining fuel.
/// Useful for tests that need to inspect heap state.
///
/// # Errors
///
/// See [`Error`].
#[must_use]
pub fn run_inspecting(source: &str, fuel: Fuel) -> Io<Error, (Value, Heap)> {
    let owned = source.to_owned();
    Io::suspend(move || pipeline(&owned, fuel))
}

fn pipeline(source: &str, fuel: Fuel) -> Result<(Value, Heap), Error> {
    let tokens = lex(source)?;
    let program = parse_script(&tokens)?;
    evaluate_program(&program, fuel)
}

/// Evaluate an already-parsed program with `fuel`, starting from the
/// engine's default initial environment (`undefined`, `NaN`, `Infinity`).
///
/// # Errors
///
/// See [`Error`].
pub fn evaluate_program(program: &Program, fuel: Fuel) -> Result<(Value, Heap), Error> {
    evaluate_program_with(program, initial_env(), Heap::new(), fuel)
}

/// Evaluate an already-parsed program in a caller-supplied environment,
/// heap, and fuel.  Used by downstream crates (e.g. `ecma-runtime-cat`)
/// to pre-populate global bindings with native callables and host objects
/// before evaluation.
///
/// # Errors
///
/// See [`Error`].
pub fn evaluate_program_with(
    program: &Program,
    env: Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(Value, Heap), Error> {
    match program.value() {
        ProgramKind::Script { body } => {
            eval_block(body, &env, heap, fuel).and_then(|(completion, heap, _env, _fuel)| {
                let value = match completion {
                    Completion::Normal(v) | Completion::Return(v) => Ok(v),
                    Completion::Throw(v) => Err(Error::UncaughtException {
                        rendered: format!("{v}"),
                    }),
                    Completion::Break | Completion::Continue => Ok(Value::Undefined),
                };
                value.map(|v| (v, heap))
            })
        }
        ProgramKind::Module { .. } => Err(Error::Unsupported {
            feature: "module evaluation (v0 supports Script only)",
        }),
    }
}

fn initial_env() -> Env {
    Env::empty()
        .extend_direct("undefined", Value::Undefined)
        .extend_direct("NaN", Value::Number(f64::NAN))
        .extend_direct("Infinity", Value::Number(f64::INFINITY))
}
