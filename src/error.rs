//! Engine error type.
//!
//! Engine errors are reserved for *fatal* conditions: out-of-fuel,
//! unsupported AST nodes, and parser/lexer failures.  JavaScript-level
//! exceptions (`TypeError`, `ReferenceError`, user `throw`) are not
//! `Error`s -- they are [`Outcome::Throw`] values that propagate through
//! the evaluator and can be caught by `try`/`catch`.  Only when a `Throw`
//! escapes the top of the script does it become
//! [`Error::UncaughtException`].
//!
//! [`Outcome::Throw`]: crate::outcome::Outcome::Throw

use ecma_lex_cat::error::Error as LexError;
use ecma_parse_cat::Error as ParseError;
use ecma_syntax_cat::error::Error as SyntaxError;

/// Fatal engine errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Lexing failed.
    Lex(LexError),
    /// Parsing failed.
    Parse(Box<ParseError>),
    /// AST construction failed (validation by `ecma-syntax-cat`).
    Syntax(SyntaxError),
    /// The engine ran out of evaluation steps.
    FuelExhausted {
        /// The original budget.
        limit: u64,
    },
    /// An unhandled `throw` escaped the top of the script.
    UncaughtException {
        /// The thrown value rendered for diagnostics.
        rendered: String,
    },
    /// Encountered an AST node that v0 does not yet evaluate.
    Unsupported {
        /// What feature is not yet supported.
        feature: &'static str,
    },
    /// A `BigInt` literal appeared but `BigInt` evaluation is not
    /// implemented.
    BigIntUnsupported,
}

impl From<LexError> for Error {
    fn from(value: LexError) -> Self {
        Self::Lex(value)
    }
}

impl From<ParseError> for Error {
    fn from(value: ParseError) -> Self {
        Self::Parse(Box::new(value))
    }
}

impl From<SyntaxError> for Error {
    fn from(value: SyntaxError) -> Self {
        Self::Syntax(value)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lex(e) => write!(f, "lex error: {e}"),
            Self::Parse(e) => write!(f, "parse error: {e}"),
            Self::Syntax(e) => write!(f, "syntax error: {e}"),
            Self::FuelExhausted { limit } => {
                write!(f, "fuel exhausted after {limit} evaluation steps")
            }
            Self::UncaughtException { rendered } => {
                write!(f, "uncaught exception: {rendered}")
            }
            Self::Unsupported { feature } => write!(f, "unsupported feature: {feature}"),
            Self::BigIntUnsupported => f.write_str("BigInt evaluation is not implemented in v0"),
        }
    }
}

impl std::error::Error for Error {}
