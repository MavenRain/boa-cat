//! Runtime values and identifiers.

use std::collections::BTreeMap;

use ecma_syntax_cat::function::ArrowBody;
use ecma_syntax_cat::identifier::Identifier;
use ecma_syntax_cat::pattern::Pattern;
use ecma_syntax_cat::statement::Statement;

use crate::env::Env;

/// A heap-allocated object's identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(u64);

impl ObjectId {
    /// Build an `ObjectId` from a raw integer.  Internal use; the heap is
    /// the only legitimate source of `ObjectId`s.
    #[must_use]
    pub(crate) fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw underlying id.
    #[must_use]
    pub fn raw(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "obj#{}", self.0)
    }
}

/// A heap-allocated function's identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionId(u64);

impl FunctionId {
    /// Build a `FunctionId` from a raw integer.
    #[must_use]
    pub(crate) fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw underlying id.
    #[must_use]
    pub fn raw(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for FunctionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fn#{}", self.0)
    }
}

/// A heap-allocated variable cell's identifier.  Variables live in cells
/// so that assignment can update the value without rebuilding the
/// surrounding environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellId(u64);

impl CellId {
    /// Build a `CellId` from a raw integer.
    #[must_use]
    pub(crate) fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw underlying id.
    #[must_use]
    pub fn raw(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for CellId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cell#{}", self.0)
    }
}

/// A variable cell: the current value plus a mutability flag.  `let`/`var`
/// declarations allocate mutable cells; `const` allocates immutable ones,
/// and the engine rejects assignment to immutable cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    value: Value,
    mutable: bool,
}

impl Cell {
    /// Build a cell with the given value and mutability.
    #[must_use]
    pub fn new(value: Value, mutable: bool) -> Self {
        Self { value, mutable }
    }

    /// The current value.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Whether the cell accepts assignment.
    #[must_use]
    pub fn is_mutable(&self) -> bool {
        self.mutable
    }

    /// Replace the value, preserving mutability.
    #[must_use]
    pub fn with_value(self, value: Value) -> Self {
        Self {
            value,
            mutable: self.mutable,
        }
    }
}

/// A runtime value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// The `undefined` singleton.
    Undefined,
    /// The `null` singleton.
    Null,
    /// A boolean.
    Boolean(bool),
    /// An IEEE-754 double.  Equality follows IEEE-754 (NaN != NaN) which
    /// matches `===` semantics.
    Number(f64),
    /// A string.
    String(String),
    /// A handle to a heap-allocated object.
    Object(ObjectId),
    /// A handle to a heap-allocated function.
    Function(FunctionId),
}

impl Eq for Value {}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Undefined => f.write_str("undefined"),
            Self::Null => f.write_str("null"),
            Self::Boolean(b) => write!(f, "{b}"),
            Self::Number(n) => write!(f, "{n}"),
            Self::String(s) => write!(f, "{s:?}"),
            Self::Object(id) => write!(f, "{id}"),
            Self::Function(id) => write!(f, "{id}"),
        }
    }
}

/// A heap-allocated object: a string-keyed map of property values.  Arrays
/// are objects with numeric string keys plus a `length` property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    properties: BTreeMap<String, Value>,
}

impl Object {
    /// An empty object.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            properties: BTreeMap::new(),
        }
    }

    /// Build an object from the given property map.
    #[must_use]
    pub fn from_properties(properties: BTreeMap<String, Value>) -> Self {
        Self { properties }
    }

    /// Look up a property by name.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.properties.get(key)
    }

    /// All properties in name order.
    #[must_use]
    pub fn properties(&self) -> &BTreeMap<String, Value> {
        &self.properties
    }

    /// Return a copy of the object with `key` set to `value`.
    #[must_use]
    pub fn with(&self, key: String, value: Value) -> Self {
        let mut next = self.properties.clone();
        let _ = next.insert(key, value);
        Self { properties: next }
    }
}

/// A function's static definition.  Captures the parameters, body, the
/// environment at the definition site (for closure semantics), and whether
/// the function is an arrow function (no own `this`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    name: Option<Identifier>,
    params: Vec<Pattern>,
    body: FunctionBody,
    captured_env: Env,
    is_arrow: bool,
}

impl FunctionDef {
    /// Build a function definition.
    #[must_use]
    pub fn new(
        name: Option<Identifier>,
        params: Vec<Pattern>,
        body: FunctionBody,
        captured_env: Env,
        is_arrow: bool,
    ) -> Self {
        Self {
            name,
            params,
            body,
            captured_env,
            is_arrow,
        }
    }

    /// The function's name, if any.
    #[must_use]
    pub fn name(&self) -> Option<&Identifier> {
        self.name.as_ref()
    }

    /// The formal parameters.
    #[must_use]
    pub fn params(&self) -> &[Pattern] {
        &self.params
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> &FunctionBody {
        &self.body
    }

    /// The lexical environment at the definition site.
    #[must_use]
    pub fn captured_env(&self) -> &Env {
        &self.captured_env
    }

    /// Whether the function is an arrow function.
    #[must_use]
    pub fn is_arrow(&self) -> bool {
        self.is_arrow
    }
}

/// A function body: statements (for `function ...`) or an expression
/// (for concise arrow bodies, kept as a wrapping `Statement::Return` in
/// the unified `Statements` variant).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionBody {
    /// A statement-list body.
    Statements(Vec<Statement>),
    /// An arrow body, retained so the evaluator can distinguish expression
    /// vs block forms.  Currently both lower to statements at evaluation
    /// time; the variant is kept for source fidelity.
    Arrow(Box<ArrowBody>),
}

impl FunctionBody {
    /// View the body as statements; an expression body is wrapped in a
    /// synthetic `Return` statement at evaluation.
    #[must_use]
    pub fn as_statements(&self) -> ArrowOrStatements<'_> {
        match self {
            Self::Statements(stmts) => ArrowOrStatements::Statements(stmts),
            Self::Arrow(body) => ArrowOrStatements::Arrow(body),
        }
    }
}

/// View result of [`FunctionBody::as_statements`].
pub enum ArrowOrStatements<'a> {
    /// Statement list (function declarations / non-concise arrow bodies).
    Statements(&'a [Statement]),
    /// Arrow body (concise expression or block).
    Arrow(&'a ArrowBody),
}
