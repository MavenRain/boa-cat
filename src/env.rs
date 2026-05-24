//! Persistent lexical environments.
//!
//! The environment is a cons list of `(name, CellId)` bindings.  Cells
//! live in the [`Heap`] so assignment can mutate variables without
//! rebuilding the surrounding environment.
//!
//! [`Heap`]: crate::heap::Heap

use crate::value::{CellId, Value};

/// A binding's reference: either a heap-allocated cell (for variables and
/// parameters) or a direct value (used for the implicit `__this__`
/// binding which is value-typed and never assigned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// A mutable or immutable variable cell.
    Cell(CellId),
    /// A direct value binding (e.g. `__this__`).
    Direct(Value),
}

/// A persistent lexical environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Env {
    /// The empty environment.
    #[default]
    Empty,
    /// A binding atop a smaller environment.
    Cons {
        /// The bound name.
        name: String,
        /// The binding -- a cell reference or direct value.
        binding: Binding,
        /// The environment underneath this binding.
        rest: Box<Env>,
    },
}

impl Env {
    /// The empty environment.
    #[must_use]
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Extend `self` with a cell-backed binding.
    #[must_use]
    pub fn extend_cell(&self, name: impl Into<String>, cell: CellId) -> Self {
        Self::Cons {
            name: name.into(),
            binding: Binding::Cell(cell),
            rest: Box::new(self.clone()),
        }
    }

    /// Extend `self` with a direct value binding.  Used for the implicit
    /// `__this__` binding which is value-typed.
    #[must_use]
    pub fn extend_direct(&self, name: impl Into<String>, value: Value) -> Self {
        Self::Cons {
            name: name.into(),
            binding: Binding::Direct(value),
            rest: Box::new(self.clone()),
        }
    }

    /// Look up `name`, returning the most recently bound binding.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&Binding> {
        match self {
            Self::Empty => None,
            Self::Cons {
                name: n,
                binding,
                rest,
            } => {
                if n == name {
                    Some(binding)
                } else {
                    rest.lookup(name)
                }
            }
        }
    }
}
