//! v0.4 Promise dispatch tests.
//!
//! These tests build promises from Rust (since v0.4 doesn't yet
//! ship a JS-side `Promise.resolve` -- that lands in
//! ecma-runtime-cat 0.3), bind them into env as `p`, and run JS
//! against them through `evaluate_program_with`.  Pending promises
//! stay pending until the v0.5 microtask driver is added, so the
//! observable surface here is the Resolved / Rejected dispatch
//! paths plus chaining.

use boa_cat::env::Env;
use boa_cat::evaluate_program_with;
use boa_cat::fuel::Fuel;
use boa_cat::heap::Heap;
use boa_cat::value::{Cell, Value};
use boa_cat::{Error, PromiseHandler, PromiseState};
use ecma_lex_cat::lex;
use ecma_parse_cat::parse_script;

fn install_promise(env: &Env, heap: Heap, state: PromiseState) -> (Env, Heap) {
    let (promise_id, heap) = heap.alloc_promise(state);
    let (cell_id, heap) = heap.alloc_cell(Cell::new(Value::Promise(promise_id), false));
    (env.extend_cell("p", cell_id), heap)
}

fn run_eval(script: &str, env: Env, heap: Heap) -> Result<Value, Error> {
    let tokens = lex(script).map_err(Error::from)?;
    let program = parse_script(&tokens).map_err(Error::from)?;
    let (value, _heap) = evaluate_program_with(&program, env, heap, Fuel::new(100_000))?;
    Ok(value)
}

#[test]
fn typeof_promise_is_object() -> Result<(), Error> {
    let (env, heap) = install_promise(
        &Env::empty(),
        Heap::new(),
        PromiseState::Resolved(Value::Number(7.0)),
    );
    let value = run_eval("typeof p", env, heap)?;
    matches!(value, Value::String(ref s) if s == "object")
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected \"object\", got {value:?}"),
        })
}

#[test]
fn then_on_resolved_invokes_callback() -> Result<(), Error> {
    let (env, heap) = install_promise(
        &Env::empty(),
        Heap::new(),
        PromiseState::Resolved(Value::Number(7.0)),
    );
    let value = run_eval(
        "let captured = -1;
        p.then(v => { captured = v; return v + 1; });
        captured",
        env,
        heap,
    )?;
    matches!(value, Value::Number(n) if (n - 7.0).abs() < 1e-9)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected 7 captured, got {value:?}"),
        })
}

#[test]
fn then_on_rejected_passes_through_value() -> Result<(), Error> {
    // Rejected promise with a `.then(onResolve)` (no on_reject)
    // returns a new Rejected promise.  Chain `.then(null, cb)`
    // (the spec-equivalent of `.catch(cb)`) to recover -- note
    // ecma-parse-cat 0.2 rejects dot-member access on the
    // reserved identifier `catch`, so this test uses the two-arg
    // `.then` form.
    let (env, heap) = install_promise(
        &Env::empty(),
        Heap::new(),
        PromiseState::Rejected(Value::String("boom".to_owned())),
    );
    let value = run_eval(
        "let recovered = '';
        p.then(v => v + '!').then(null, e => { recovered = e; });
        recovered",
        env,
        heap,
    )?;
    matches!(value, Value::String(ref s) if s == "boom")
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected \"boom\" recovered, got {value:?}"),
        })
}

#[test]
fn catch_on_resolved_passes_through_value() -> Result<(), Error> {
    let (env, heap) = install_promise(
        &Env::empty(),
        Heap::new(),
        PromiseState::Resolved(Value::Number(42.0)),
    );
    let value = run_eval(
        "let captured = -1;
        p.then(null, e => 999).then(v => { captured = v; });
        captured",
        env,
        heap,
    )?;
    matches!(value, Value::Number(n) if (n - 42.0).abs() < 1e-9)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected 42 captured, got {value:?}"),
        })
}

#[test]
fn chained_then_propagates_callback_return() -> Result<(), Error> {
    let (env, heap) = install_promise(
        &Env::empty(),
        Heap::new(),
        PromiseState::Resolved(Value::Number(3.0)),
    );
    let value = run_eval(
        "let final_value = -1;
        p.then(v => v * 2).then(v => v + 10).then(v => { final_value = v; });
        final_value",
        env,
        heap,
    )?;
    matches!(value, Value::Number(n) if (n - 16.0).abs() < 1e-9)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected 16 from 3*2+10, got {value:?}"),
        })
}

#[test]
fn pending_then_returns_pending_child_with_handler_queued() -> Result<(), Error> {
    // Pending promise -> `.then(cb)` queues cb on the source AND
    // returns a fresh Pending child promise.  The child stays
    // Pending (the v0.5 microtask driver isn't here yet); we
    // observe that the returned value is still a Promise (typeof
    // object) even though the cb never fires.
    let (env, heap) = install_promise(
        &Env::empty(),
        Heap::new(),
        PromiseState::Pending(Vec::new()),
    );
    let value = run_eval(
        "let fired = false;
        const child = p.then(_v => { fired = true; });
        typeof child + ':' + fired",
        env,
        heap,
    )?;
    matches!(value, Value::String(ref s) if s == "object:false")
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected 'object:false', got {value:?}"),
        })
}

#[test]
fn then_without_callback_passes_through_resolution() -> Result<(), Error> {
    let (env, heap) = install_promise(
        &Env::empty(),
        Heap::new(),
        PromiseState::Resolved(Value::Number(99.0)),
    );
    let value = run_eval(
        "let captured = -1;
        p.then().then(v => { captured = v; });
        captured",
        env,
        heap,
    )?;
    matches!(value, Value::Number(n) if (n - 99.0).abs() < 1e-9)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected 99 captured, got {value:?}"),
        })
}

#[test]
fn promise_handler_carries_chained_id() -> Result<(), Error> {
    // Rust-side smoke: the `PromiseHandler` builder records the
    // chained promise + callback halves verbatim.  Useful for the
    // v0.5 microtask driver chunk to assert ordering.
    let (chained_id, _heap) = Heap::new().alloc_promise(PromiseState::Pending(Vec::new()));
    let handler = PromiseHandler::new(Some(Value::Number(1.0)), None, chained_id);
    let on_resolve_ok =
        matches!(handler.on_resolve(), Some(Value::Number(n)) if (*n - 1.0).abs() < 1e-9);
    let on_reject_absent = handler.on_reject().is_none();
    let chained_matches = handler.chained() == chained_id;
    (on_resolve_ok && on_reject_absent && chained_matches)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: "PromiseHandler fields did not round-trip".to_owned(),
        })
}
