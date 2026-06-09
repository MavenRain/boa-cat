//! Promise values, synchronous `.then` / `.catch` dispatch (v0.4),
//! and the microtask driver (v0.5).
//!
//! Dispatch surface:
//!
//! - [`PromiseState::Resolved(v)`] -- `.then(cb)` invokes `cb(v)`
//!   immediately and returns a new `Resolved` promise wrapping
//!   `cb`'s result; the second `.then` arg (and `.catch`) passes
//!   through unchanged.
//! - [`PromiseState::Rejected(v)`] -- `.then(cb)` passes through;
//!   `.then(null, cb)` (and `.catch(cb)`) invokes `cb(v)` and
//!   returns `Resolved(cb_result)` (matching real Promise
//!   semantics where a handled rejection recovers).
//! - [`PromiseState::Pending`] -- handlers queue on `.then`; calling
//!   [`resolve`] / [`reject`] (or the JS-side `__resolve_promise` /
//!   `__reject_promise` hooks installed via [`install_test_hooks`])
//!   transitions the promise and drains the queue, settling each
//!   chained child by invoking its callback against the resolved
//!   value and recursing.  Once settled a promise is immutable;
//!   later [`resolve`] / [`reject`] calls on the same id are no-ops
//!   per Promise A+ spec.
//!
//! Known v0.5 limitation: thenable adoption.  When a `.then`
//! callback returns a Promise, real engines adopt that Promise's
//! eventual state into the chained promise.  This implementation
//! wraps the returned Promise in a new `Resolved(Value::Promise(_))`,
//! so chaining through `.then(_ => somePromise).then(cb)` gives
//! `cb` the Promise value rather than its eventual contents.  Lands
//! when ecma-runtime-cat 0.3 introduces `Promise.resolve(...)` and
//! the spec-faithful adoption path can be tested end-to-end.

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

/// Transition the promise at `promise_id` from `Pending` to
/// `Resolved(value)` and drain its queued handlers (v0.5).  Each
/// queued [`PromiseHandler`] fires its `on_resolve` callback (or
/// passes the value through unchanged when no callback is set),
/// and the result settles the handler's `chained` child -- the
/// drain is fully recursive, so chained `.then(...).then(...)`
/// graphs all settle within one [`resolve`] call.  Promises that
/// are already settled (Resolved or Rejected) are no-ops per
/// Promise A+ spec.
///
/// # Errors
///
/// Propagates errors from any handler callback.  Successful no-ops
/// (already-settled or unknown id) yield
/// `Outcome::Normal(Value::Undefined)`.
pub fn resolve(promise_id: PromiseId, value: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    settle_pending(
        promise_id, value, /* was_resolved = */ true, heap, fuel,
    )
}

/// Transition the promise at `promise_id` from `Pending` to
/// `Rejected(value)` and drain its queued handlers, firing each
/// `on_reject` callback (or passing the value through as a fresh
/// rejection when no callback is set).
///
/// # Errors
///
/// Propagates errors from any handler callback.
pub fn reject(promise_id: PromiseId, value: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    settle_pending(
        promise_id, value, /* was_resolved = */ false, heap, fuel,
    )
}

fn settle_pending(
    promise_id: PromiseId,
    value: Value,
    was_resolved: bool,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match heap.promise(promise_id).cloned() {
        Some(PromiseState::Pending(handlers)) => {
            let new_state = if was_resolved {
                PromiseState::Resolved(value.clone())
            } else {
                PromiseState::Rejected(value.clone())
            };
            let heap = heap
                .store_promise(promise_id, new_state)
                .unwrap_or_else(|h| h);
            drain_handlers(handlers, value, was_resolved, heap, fuel)
        }
        Some(PromiseState::Resolved(_) | PromiseState::Rejected(_)) | None => {
            // Already settled (or missing): no-op per Promise A+.
            Ok((Outcome::Normal(Value::Undefined), heap, fuel))
        }
    }
}

fn drain_handlers(
    handlers: Vec<PromiseHandler>,
    value: Value,
    was_resolved: bool,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    handlers.into_iter().try_fold(
        (Outcome::Normal(Value::Undefined), heap, fuel),
        |(_, heap, fuel), handler| {
            fire_one_handler(handler, value.clone(), was_resolved, heap, fuel)
        },
    )
}

fn fire_one_handler(
    handler: PromiseHandler,
    value: Value,
    was_resolved: bool,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    let callback = if was_resolved {
        handler.on_resolve().cloned()
    } else {
        handler.on_reject().cloned()
    };
    let chained = handler.chained();
    match callback {
        Some(cb) if is_callable(&cb) => {
            crate::expression::call_function(&cb, &Value::Undefined, vec![value], heap, fuel)
                .and_then(|(outcome, heap, fuel)| match outcome {
                    Outcome::Normal(result) => resolve(chained, result, heap, fuel),
                    Outcome::Throw(thrown) => reject(chained, thrown, heap, fuel),
                })
        }
        Some(_) | None => {
            // No callback (or non-callable): pass the value through
            // unchanged, preserving the resolved-vs-rejected
            // disposition.
            if was_resolved {
                resolve(chained, value, heap, fuel)
            } else {
                reject(chained, value, heap, fuel)
            }
        }
    }
}

/// `__resolve_promise(promise, value)` `NativeFn` for tests and
/// embedders that don't yet have a JS-side `Promise.resolve`.
/// Returns `undefined`.
///
/// # Errors
///
/// Propagates errors from the recursive handler drain.
#[allow(clippy::needless_pass_by_value)]
pub fn resolve_test_hook(args: Vec<Value>, _this: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    let promise_id = args.first().and_then(promise_id_of);
    let value = args.get(1).cloned().unwrap_or(Value::Undefined);
    match promise_id {
        Some(id) => resolve(id, value, heap, fuel),
        None => Ok((Outcome::Normal(Value::Undefined), heap, fuel)),
    }
}

/// `__reject_promise(promise, value)` `NativeFn` for tests and
/// embedders.
///
/// # Errors
///
/// Propagates errors from the recursive handler drain.
#[allow(clippy::needless_pass_by_value)]
pub fn reject_test_hook(args: Vec<Value>, _this: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    let promise_id = args.first().and_then(promise_id_of);
    let value = args.get(1).cloned().unwrap_or(Value::Undefined);
    match promise_id {
        Some(id) => reject(id, value, heap, fuel),
        None => Ok((Outcome::Normal(Value::Undefined), heap, fuel)),
    }
}

/// Install `__resolve_promise` and `__reject_promise` as `const`
/// cells in `env`.  Test code and embedders that want to drive
/// pending promises from JS call this once on the initial env.
#[must_use]
pub fn install_test_hooks(env: crate::env::Env, heap: Heap) -> (crate::env::Env, Heap) {
    let (resolve_cell, heap) = heap.alloc_cell(crate::value::Cell::new(
        Value::Native(resolve_test_hook),
        false,
    ));
    let (reject_cell, heap) = heap.alloc_cell(crate::value::Cell::new(
        Value::Native(reject_test_hook),
        false,
    ));
    let env = env
        .extend_cell("__resolve_promise", resolve_cell)
        .extend_cell("__reject_promise", reject_cell);
    (env, heap)
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
