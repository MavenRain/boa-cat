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

mod microtask_driver {
    //! v0.5 microtask driver tests: queued handlers fire when the
    //! source promise transitions Pending -> Resolved / Rejected
    //! via `__resolve_promise` / `__reject_promise`.

    use super::{install_promise, run_eval};
    use boa_cat::env::Env;
    use boa_cat::heap::Heap;
    use boa_cat::value::Value;
    use boa_cat::{Error, PromiseState};

    fn install_promise_with_hooks(state: PromiseState) -> (Env, Heap) {
        let (env, heap) = install_promise(&Env::empty(), Heap::new(), state);
        boa_cat::promise::install_test_hooks(env, heap)
    }

    #[test]
    fn resolve_drains_queued_then_handler() -> Result<(), Error> {
        let (env, heap) = install_promise_with_hooks(PromiseState::Pending(Vec::new()));
        // Queue a handler; then resolve from JS; verify the handler
        // fired and observed the resolution value.
        let value = run_eval(
            "let captured = -1;
            p.then(v => { captured = v; });
            __resolve_promise(p, 42);
            captured",
            env,
            heap,
        )?;
        matches!(value, Value::Number(n) if (n - 42.0).abs() < 1e-9)
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("expected 42 captured after resolve, got {value:?}"),
            })
    }

    #[test]
    fn reject_drains_queued_then_handler_second_arg() -> Result<(), Error> {
        let (env, heap) = install_promise_with_hooks(PromiseState::Pending(Vec::new()));
        let value = run_eval(
            "let captured = '';
            p.then(null, e => { captured = e; });
            __reject_promise(p, 'boom');
            captured",
            env,
            heap,
        )?;
        matches!(value, Value::String(ref s) if s == "boom")
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("expected 'boom' captured after reject, got {value:?}"),
            })
    }

    #[test]
    fn resolve_settles_chained_grandchild() -> Result<(), Error> {
        // `.then(...).then(...)` builds a 3-promise chain rooted at
        // p.  Resolving p must cascade through the chain so the
        // grandchild's callback fires too.
        let (env, heap) = install_promise_with_hooks(PromiseState::Pending(Vec::new()));
        let value = run_eval(
            "let captured = -1;
            p.then(v => v * 2).then(v => { captured = v + 1; });
            __resolve_promise(p, 5);
            captured",
            env,
            heap,
        )?;
        matches!(value, Value::Number(n) if (n - 11.0).abs() < 1e-9)
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("expected 11 from (5*2)+1, got {value:?}"),
            })
    }

    #[test]
    fn second_resolve_call_is_a_no_op() -> Result<(), Error> {
        let (env, heap) = install_promise_with_hooks(PromiseState::Pending(Vec::new()));
        // Promise A+: only the first settle wins; the second call's
        // value is discarded.
        let value = run_eval(
            "let captured = -1;
            p.then(v => { captured = v; });
            __resolve_promise(p, 7);
            __resolve_promise(p, 999);
            captured",
            env,
            heap,
        )?;
        matches!(value, Value::Number(n) if (n - 7.0).abs() < 1e-9)
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("expected 7 (first resolve wins), got {value:?}"),
            })
    }

    #[test]
    fn callback_throw_becomes_chained_rejection() -> Result<(), Error> {
        // When an on_resolve callback throws, the chained promise
        // adopts that throw as a Rejected state.  Use a
        // `.then(null, cb)` on the child to recover.
        let (env, heap) = install_promise_with_hooks(PromiseState::Pending(Vec::new()));
        let value = run_eval(
            "let recovered = '';
            p.then(v => { throw 'oops:' + v; }).then(null, e => { recovered = e; });
            __resolve_promise(p, 9);
            recovered",
            env,
            heap,
        )?;
        matches!(value, Value::String(ref s) if s == "oops:9")
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("expected 'oops:9' recovered, got {value:?}"),
            })
    }

    #[test]
    fn multiple_then_handlers_on_one_pending_all_fire() -> Result<(), Error> {
        let (env, heap) = install_promise_with_hooks(PromiseState::Pending(Vec::new()));
        // Two `.then` calls on the same Pending p queue two
        // handlers; the resolve fans out to both.
        let value = run_eval(
            "let a = -1;
            let b = -1;
            p.then(v => { a = v; });
            p.then(v => { b = v + 100; });
            __resolve_promise(p, 5);
            a + b",
            env,
            heap,
        )?;
        matches!(value, Value::Number(n) if (n - 110.0).abs() < 1e-9)
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("expected 5 + 105 = 110, got {value:?}"),
            })
    }
}
