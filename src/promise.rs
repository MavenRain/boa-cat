//! Promise values and synchronous `.then` / `.catch` dispatch
//! (boa-cat 0.4 -- chunk 1 of the async track).
//!
//! v0.4 ships the type infrastructure and the resolved/rejected
//! dispatch paths:
//!
//! - [`PromiseState::Resolved(v)`] -- `.then(cb)` invokes `cb(v)`
//!   immediately and returns a new `Resolved` promise wrapping
//!   `cb`'s result; `.catch(cb)` passes through unchanged.
//! - [`PromiseState::Rejected(v)`] -- `.then(cb)` passes through;
//!   `.catch(cb)` invokes `cb(v)` and returns `Resolved(cb_result)`
//!   (matching real Promise semantics where a handled rejection
//!   recovers).
//! - [`PromiseState::Pending`] -- handlers are queued for the v0.5
//!   microtask driver.  This chunk reserves the variant but the
//!   queued handlers don't run yet (a Pending `.then` returns a
//!   Pending child promise).
//!
//! Without `await` (chunk 3) or `Promise.resolve` / `reject` /
//! `all` / `race` built-ins (chunk 5), Pending promises stay
//! Pending and the .then-chain only runs when the caller built the
//! source promise as Resolved/Rejected (typically from Rust via
//! [`Heap::alloc_promise`]).  The chunk-1 surface is enough to
//! pin down the value type + chained-resolution semantics; later
//! chunks layer the rest.

use crate::fuel::Fuel;
use crate::heap::Heap;
use crate::outcome::{EvalResult, Outcome};
use crate::value::{PromiseId, Value};

/// A promise's lifecycle state.  Stored on the heap under a
/// [`PromiseId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromiseState {
    /// Awaiting resolution.  The queued handlers (`.then` /
    /// `.catch` callbacks paired with the chained promise to
    /// settle) will fire once the v0.5 microtask driver lands.
    Pending(Vec<PromiseHandler>),
    /// Resolved with a value.
    Resolved(Value),
    /// Rejected with a value (typically a `Value::String` carrying
    /// the error text in this engine's loose error model).
    Rejected(Value),
}

/// One queued `.then` / `.catch` handler attached to a Pending
/// promise.  Both halves are optional: `.then(onResolve)` queues
/// `on_reject = None`; `.catch(onReject)` queues `on_resolve =
/// None`.  `chained` is the downstream promise that will receive
/// the handler's return value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromiseHandler {
    on_resolve: Option<Value>,
    on_reject: Option<Value>,
    chained: PromiseId,
}

impl PromiseHandler {
    /// Build a handler pair targeting `chained`.
    #[must_use]
    pub fn new(on_resolve: Option<Value>, on_reject: Option<Value>, chained: PromiseId) -> Self {
        Self {
            on_resolve,
            on_reject,
            chained,
        }
    }

    /// The on-resolve callback, if any.
    #[must_use]
    pub fn on_resolve(&self) -> Option<&Value> {
        self.on_resolve.as_ref()
    }

    /// The on-reject callback, if any.
    #[must_use]
    pub fn on_reject(&self) -> Option<&Value> {
        self.on_reject.as_ref()
    }

    /// The downstream promise this handler resolves once it fires.
    #[must_use]
    pub fn chained(&self) -> PromiseId {
        self.chained
    }
}

/// `Promise.prototype.then(onResolve, onReject)` native shim.
///
/// Called by [`crate::expression::access_property`] when JS reads
/// `promise.then` -- the returned `NativeFn` carries the chunk-1
/// dispatch logic.
///
/// # Errors
///
/// Propagates errors from the invoked callback.
#[allow(clippy::needless_pass_by_value)]
pub fn then_impl(args: Vec<Value>, this: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    match promise_id_of(&this) {
        Some(promise_id) => {
            let on_resolve = args.first().cloned();
            let on_reject = args.get(1).cloned();
            settle(promise_id, on_resolve, on_reject, heap, fuel)
        }
        None => Ok((
            Outcome::Throw(type_error("Promise.prototype.then called on non-promise")),
            heap,
            fuel,
        )),
    }
}

/// `Promise.prototype.catch(onReject)` native shim -- sugar for
/// `.then(undefined, onReject)`.
///
/// # Errors
///
/// Propagates errors from the invoked callback.
#[allow(clippy::needless_pass_by_value)]
pub fn catch_impl(args: Vec<Value>, this: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    match promise_id_of(&this) {
        Some(promise_id) => {
            let on_reject = args.first().cloned();
            settle(promise_id, None, on_reject, heap, fuel)
        }
        None => Ok((
            Outcome::Throw(type_error("Promise.prototype.catch called on non-promise")),
            heap,
            fuel,
        )),
    }
}

fn promise_id_of(value: &Value) -> Option<PromiseId> {
    match value {
        Value::Promise(id) => Some(*id),
        Value::Undefined
        | Value::Null
        | Value::Boolean(_)
        | Value::Number(_)
        | Value::String(_)
        | Value::Object(_)
        | Value::Function(_)
        | Value::Native(_) => None,
    }
}

fn settle(
    promise_id: PromiseId,
    on_resolve: Option<Value>,
    on_reject: Option<Value>,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match heap.promise(promise_id).cloned() {
        Some(PromiseState::Resolved(value)) => fire_resolve_handler(value, on_resolve, heap, fuel),
        Some(PromiseState::Rejected(value)) => fire_reject_handler(value, on_reject, heap, fuel),
        Some(PromiseState::Pending(handlers)) => {
            queue_handler(promise_id, handlers, on_resolve, on_reject, heap, fuel)
        }
        None => Ok((
            Outcome::Throw(type_error("promise missing from heap")),
            heap,
            fuel,
        )),
    }
}

fn fire_resolve_handler(
    value: Value,
    on_resolve: Option<Value>,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match on_resolve {
        Some(callback) if is_callable(&callback) => {
            crate::expression::call_function(&callback, &Value::Undefined, vec![value], heap, fuel)
                .and_then(|(outcome, heap, fuel)| chain_outcome(outcome, heap, fuel))
        }
        Some(_) | None => alloc_resolved_value(value, heap, fuel),
    }
}

fn fire_reject_handler(
    value: Value,
    on_reject: Option<Value>,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match on_reject {
        Some(callback) if is_callable(&callback) => {
            crate::expression::call_function(&callback, &Value::Undefined, vec![value], heap, fuel)
                .and_then(|(outcome, heap, fuel)| chain_outcome(outcome, heap, fuel))
        }
        Some(_) | None => alloc_rejected_value(value, heap, fuel),
    }
}

/// Pending source: queue the handler onto the source promise's
/// handler list and return a fresh Pending child promise that
/// the v0.5 microtask driver will settle.
#[allow(clippy::unnecessary_wraps)] // EvalResult is the uniform signature for these helpers
fn queue_handler(
    source: PromiseId,
    existing_handlers: Vec<PromiseHandler>,
    on_resolve: Option<Value>,
    on_reject: Option<Value>,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    let (chained, heap) = heap.alloc_promise(PromiseState::Pending(Vec::new()));
    let handler = PromiseHandler::new(on_resolve, on_reject, chained);
    let extended_handlers: Vec<PromiseHandler> = existing_handlers
        .into_iter()
        .chain(std::iter::once(handler))
        .collect();
    let heap = heap
        .store_promise(source, PromiseState::Pending(extended_handlers))
        .unwrap_or_else(|h| h);
    Ok((Outcome::Normal(Value::Promise(chained)), heap, fuel))
}

fn chain_outcome(outcome: Outcome, heap: Heap, fuel: Fuel) -> EvalResult {
    match outcome {
        Outcome::Normal(value) => alloc_resolved_value(value, heap, fuel),
        Outcome::Throw(value) => alloc_rejected_value(value, heap, fuel),
    }
}

#[allow(clippy::unnecessary_wraps)] // EvalResult is the uniform signature for these helpers
fn alloc_resolved_value(value: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    let (id, heap) = heap.alloc_promise(PromiseState::Resolved(value));
    Ok((Outcome::Normal(Value::Promise(id)), heap, fuel))
}

#[allow(clippy::unnecessary_wraps)] // EvalResult is the uniform signature for these helpers
fn alloc_rejected_value(value: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    let (id, heap) = heap.alloc_promise(PromiseState::Rejected(value));
    Ok((Outcome::Normal(Value::Promise(id)), heap, fuel))
}

fn is_callable(value: &Value) -> bool {
    matches!(value, Value::Function(_) | Value::Native(_))
}

fn type_error(message: &str) -> Value {
    Value::String(format!("TypeError: {message}"))
}
