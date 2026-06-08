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

/// A heap-allocated promise's identifier (v0.4 async track).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PromiseId(u64);

impl PromiseId {
    /// Build a `PromiseId` from a raw integer.  Internal use; the
    /// heap is the only legitimate source of `PromiseId`s.
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

impl std::fmt::Display for PromiseId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "promise#{}", self.0)
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
    /// A native (Rust-implemented) callable.  Native callables receive
    /// their arguments, a `this` binding, the current heap, and the fuel
    /// budget, and return the standard expression-evaluation result.
    Native(NativeFn),
    /// A handle to a heap-allocated promise (v0.4).
    Promise(PromiseId),
}

/// Signature of a native (Rust-implemented) callable.
///
/// `args` are the call's positional arguments after evaluation; `this`
/// is the binding the call site selected (the method receiver for
/// `obj.method(...)`, or `Undefined` for plain calls); `heap` and `fuel`
/// thread the persistent state.
pub type NativeFn = fn(
    args: Vec<Value>,
    this: Value,
    heap: crate::heap::Heap,
    fuel: crate::fuel::Fuel,
) -> crate::outcome::EvalResult;

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
            Self::Native(_) => f.write_str("function [native code]"),
            Self::Promise(_) => f.write_str("[object Promise]"),
        }
    }
}

/// A heap-allocated object: a string-keyed map of property values.  Arrays
/// are objects with numeric string keys plus a `length` property.
///
/// v0.3 adds a parallel accessor map: any key present in `accessors`
/// is a getter/setter property (the engine invokes the getter on
/// `obj.key` reads and the setter on `obj.key = value` writes).
/// Data and accessor maps are mutually exclusive by key -- writing a
/// data value via [`Self::with`] evicts any existing accessor on the
/// same key, and installing an accessor via [`Self::with_accessor`]
/// evicts any existing data value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Object {
    properties: BTreeMap<String, Value>,
    accessors: BTreeMap<String, AccessorPair>,
}

/// The getter / setter pair backing an accessor property.  Either
/// half may be absent: a getter-only accessor returns `undefined`
/// on assignment in non-strict mode; a setter-only accessor returns
/// `undefined` on read.  Each function value is whatever
/// [`Value::Function`] or [`Value::Native`] the call site supplied.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccessorPair {
    get: Option<Value>,
    set: Option<Value>,
}

impl AccessorPair {
    /// Build an accessor with the given getter / setter halves.
    #[must_use]
    pub fn new(get: Option<Value>, set: Option<Value>) -> Self {
        Self { get, set }
    }

    /// The getter, if any.
    #[must_use]
    pub fn get_fn(&self) -> Option<&Value> {
        self.get.as_ref()
    }

    /// The setter, if any.
    #[must_use]
    pub fn set_fn(&self) -> Option<&Value> {
        self.set.as_ref()
    }

    /// Return a new pair with the getter replaced.
    #[must_use]
    pub fn with_get(&self, get: Value) -> Self {
        Self {
            get: Some(get),
            set: self.set.clone(),
        }
    }

    /// Return a new pair with the setter replaced.
    #[must_use]
    pub fn with_set(&self, set: Value) -> Self {
        Self {
            get: self.get.clone(),
            set: Some(set),
        }
    }
}

impl Object {
    /// An empty object.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build an object from the given data-property map.  Accessor
    /// properties are not installed here; use
    /// [`Self::with_accessor`] for those.
    #[must_use]
    pub fn from_properties(properties: BTreeMap<String, Value>) -> Self {
        Self {
            properties,
            accessors: BTreeMap::new(),
        }
    }

    /// Look up a data property by name.  Returns `None` for absent
    /// keys and for accessor properties (use [`Self::accessor`] to
    /// reach those).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.properties.get(key)
    }

    /// All data properties in name order.  Accessor properties are
    /// not surfaced here; this preserves the v0.2 caller contract
    /// (e.g. `JSON.stringify` enumerates data properties only).
    #[must_use]
    pub fn properties(&self) -> &BTreeMap<String, Value> {
        &self.properties
    }

    /// Look up an accessor property by name.
    #[must_use]
    pub fn accessor(&self, key: &str) -> Option<&AccessorPair> {
        self.accessors.get(key)
    }

    /// All accessor properties in name order.
    #[must_use]
    pub fn accessors(&self) -> &BTreeMap<String, AccessorPair> {
        &self.accessors
    }

    /// Return a copy of the object with `key` set to a data property
    /// holding `value`.  Any existing accessor on `key` is evicted.
    #[must_use]
    pub fn with(&self, key: String, value: Value) -> Self {
        let mut next_props = self.properties.clone();
        let mut next_accs = self.accessors.clone();
        let _ = next_accs.remove(&key);
        let _ = next_props.insert(key, value);
        Self {
            properties: next_props,
            accessors: next_accs,
        }
    }

    /// Return a copy of the object with `key` set to the given
    /// accessor pair.  Any existing data value on `key` is evicted.
    #[must_use]
    pub fn with_accessor(&self, key: String, pair: AccessorPair) -> Self {
        let mut next_props = self.properties.clone();
        let mut next_accs = self.accessors.clone();
        let _ = next_props.remove(&key);
        let _ = next_accs.insert(key, pair);
        Self {
            properties: next_props,
            accessors: next_accs,
        }
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
