//! Integration tests covering objects, arrays, and member access.

use boa_cat::{Error, Value, run};

fn eval(source: &str) -> Result<Value, Error> {
    run(source).run()
}

fn assert_number(actual: &Value, expected: f64) -> Result<(), Error> {
    matches!(actual, Value::Number(n) if (*n - expected).abs() < 1e-9)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected Number({expected}), got {actual:?}"),
        })
}

fn assert_string(actual: &Value, expected: &str) -> Result<(), Error> {
    matches!(actual, Value::String(s) if s == expected)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected String({expected:?}), got {actual:?}"),
        })
}

#[test]
fn object_literal_access() -> Result<(), Error> {
    assert_number(&eval("const o = { a: 1, b: 2 }; o.a + o.b")?, 3.0)
}

#[test]
fn object_computed_key() -> Result<(), Error> {
    assert_number(&eval("const o = { x: 10 }; o[\"x\"]")?, 10.0)
}

#[test]
fn object_shorthand() -> Result<(), Error> {
    assert_number(&eval("const a = 7; const o = { a }; o.a")?, 7.0)
}

#[test]
fn object_assignment() -> Result<(), Error> {
    assert_number(&eval("const o = { x: 1 }; o.x = 100; o.x")?, 100.0)
}

#[test]
fn array_literal_indexing() -> Result<(), Error> {
    assert_number(&eval("const a = [10, 20, 30]; a[1]")?, 20.0)
}

#[test]
fn array_length() -> Result<(), Error> {
    assert_number(&eval("[1, 2, 3, 4, 5].length")?, 5.0)
}

#[test]
fn array_spread() -> Result<(), Error> {
    assert_number(
        &eval("const a = [1, 2, 3]; const b = [...a, 4]; b[3]")?,
        4.0,
    )
}

#[test]
fn object_spread() -> Result<(), Error> {
    assert_number(
        &eval("const a = { x: 1 }; const b = { ...a, y: 2 }; b.x + b.y")?,
        3.0,
    )
}

#[test]
fn string_length() -> Result<(), Error> {
    assert_number(&eval("\"hello\".length")?, 5.0)
}

#[test]
fn nested_object() -> Result<(), Error> {
    assert_number(
        &eval("const o = { inner: { value: 42 } }; o.inner.value")?,
        42.0,
    )
}

#[test]
fn try_catch_recovers() -> Result<(), Error> {
    assert_string(
        &eval("let r = \"\"; try { throw \"oops\"; } catch (e) { r = e; } r")?,
        "oops",
    )
}

#[test]
fn try_catch_finally() -> Result<(), Error> {
    assert_string(
        &eval(
            "let log = \"\";
            try { log += \"a\"; throw 1; } catch (e) { log += \"b\"; } finally { log += \"c\"; }
            log",
        )?,
        "abc",
    )
}

#[test]
fn getter_invokes_on_read_in_js_literal() -> Result<(), Error> {
    // v0.3.1 (ecma-parse-cat 0.2 dep bump): `{ get x() {} }`
    // literal syntax now reaches the engine's accessor dispatch
    // (the 0.3.0 engine path that previously only fired for
    // Rust-installed accessors via `Object::with_accessor`).
    // The getter runs every read; `x + x` proves we got the
    // return value (7) and not the function itself (14 vs NaN).
    assert_number(
        &eval("const o = { get x() { return 7; } }; o.x + o.x")?,
        14.0,
    )
}

#[test]
fn setter_invokes_on_write_in_js_literal() -> Result<(), Error> {
    // Setter writes a backing variable; the assignment expression
    // still evaluates to the RHS regardless of what the setter
    // returns.
    assert_number(
        &eval(
            "let backing = 0;
            const o = {
                get x() { return backing; },
                set x(v) { backing = v + 1; }
            };
            o.x = 5;
            o.x",
        )?,
        6.0,
    )
}

#[test]
fn data_init_can_replace_earlier_accessor_in_js_literal() -> Result<(), Error> {
    // `{ get x() {}, x: 1 }` -- the later Init wins.
    assert_number(
        &eval("const o = { get x() { return 42; }, x: 1 }; o.x")?,
        1.0,
    )
}

#[test]
fn accessor_can_replace_earlier_data_init_in_js_literal() -> Result<(), Error> {
    // `{ x: 1, get x() {} }` -- the later Get wins, x becomes
    // the accessor.
    assert_number(
        &eval("const o = { x: 1, get x() { return 42; } }; o.x")?,
        42.0,
    )
}

#[test]
fn separate_get_and_set_combine_on_same_key_in_js_literal() -> Result<(), Error> {
    // Two members for the same key build a combined accessor
    // with both halves populated.
    assert_number(
        &eval(
            "let backing = 0;
            const o = {
                get x() { return backing; },
                set x(v) { backing = v * 2; }
            };
            o.x = 5;
            o.x",
        )?,
        10.0,
    )
}

#[test]
fn shorthand_method_in_js_literal() -> Result<(), Error> {
    // `{ greet() { return 1; } }` -- shorthand methods are
    // emitted as `ObjectPropertyKind::Method` and the engine
    // treats them as data properties holding a function value.
    assert_number(
        &eval("const o = { greet() { return 7; } }; o.greet() + o.greet()")?,
        14.0,
    )
}

// v0.3 accessor-property tests using the lower-level
// `evaluate_program_with` API to install an object with an accessor
// pair from Rust, then run JS against it.  These remain useful for
// covering pure-Rust accessor installations (the path web-api-cat
// uses to bridge `document.cookie`); they exercise the same engine
// dispatch path as the JS-literal tests above.
mod accessor_dispatch {
    use boa_cat::Error;
    use boa_cat::env::Env;
    use boa_cat::evaluate_program_with;
    use boa_cat::fuel::Fuel;
    use boa_cat::heap::Heap;
    use boa_cat::outcome::{EvalResult, Outcome};
    use boa_cat::value::{AccessorPair, Cell, Object, Value};
    use ecma_lex_cat::lex;
    use ecma_parse_cat::parse_script;

    #[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
    fn ok_value(_args: Vec<Value>, _this: Value, heap: Heap, fuel: Fuel) -> EvalResult {
        Ok((Outcome::Normal(Value::Number(7.0)), heap, fuel))
    }

    #[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
    fn echo_arg_plus_one(args: Vec<Value>, _this: Value, heap: Heap, fuel: Fuel) -> EvalResult {
        let value = match args.first() {
            Some(Value::Number(n)) => Value::Number(*n + 1.0),
            Some(_) | None => Value::Undefined,
        };
        Ok((Outcome::Normal(value), heap, fuel))
    }

    fn install_accessor_object(env: &Env, heap: Heap, pair: AccessorPair) -> (Env, Heap) {
        let obj = Object::empty().with_accessor("x".to_owned(), pair);
        let (obj_id, heap) = heap.alloc_object(obj);
        let (cell_id, heap) = heap.alloc_cell(Cell::new(Value::Object(obj_id), false));
        (env.extend_cell("o", cell_id), heap)
    }

    fn run_eval(script: &str, env: Env, heap: Heap) -> Result<Value, Error> {
        let tokens = lex(script).map_err(Error::from)?;
        let program = parse_script(&tokens).map_err(Error::from)?;
        let (value, _heap) = evaluate_program_with(&program, env, heap, Fuel::new(100_000))?;
        Ok(value)
    }

    #[test]
    fn getter_invokes_on_read() -> Result<(), Error> {
        let (env, heap) = install_accessor_object(
            &Env::empty(),
            Heap::new(),
            AccessorPair::new(Some(Value::Native(ok_value)), None),
        );
        // Reading o.x twice invokes the getter twice; their sum
        // proves the read produced the getter's return value (7),
        // not the function itself.
        let value = run_eval("o.x + o.x", env, heap)?;
        matches!(value, Value::Number(n) if (n - 14.0).abs() < 1e-9)
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("expected 14, got {value:?}"),
            })
    }

    #[test]
    fn setter_invokes_on_write_assignment_returns_rhs() -> Result<(), Error> {
        // The setter would have returned `Undefined` had we not
        // followed the spec; we assert the assignment expression
        // produces the RHS (9), confirming the dispatch path
        // discards the setter's return value.
        let (env, heap) = install_accessor_object(
            &Env::empty(),
            Heap::new(),
            AccessorPair::new(None, Some(Value::Native(echo_arg_plus_one))),
        );
        let value = run_eval("(o.x = 9)", env, heap)?;
        matches!(value, Value::Number(n) if (n - 9.0).abs() < 1e-9)
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("assignment should evaluate to RHS, got {value:?}"),
            })
    }

    #[test]
    fn getter_without_setter_silently_ignores_write() -> Result<(), Error> {
        let (env, heap) = install_accessor_object(
            &Env::empty(),
            Heap::new(),
            AccessorPair::new(Some(Value::Native(ok_value)), None),
        );
        // The write is a no-op (no setter); subsequent read still
        // returns the getter's value.
        let value = run_eval("o.x = 999; o.x", env, heap)?;
        matches!(value, Value::Number(n) if (n - 7.0).abs() < 1e-9)
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("expected 7 from getter, got {value:?}"),
            })
    }

    #[test]
    fn setter_without_getter_reads_as_undefined() -> Result<(), Error> {
        let (env, heap) = install_accessor_object(
            &Env::empty(),
            Heap::new(),
            AccessorPair::new(None, Some(Value::Native(echo_arg_plus_one))),
        );
        let value = run_eval("typeof o.x", env, heap)?;
        matches!(value, Value::String(ref s) if s == "undefined")
            .then_some(())
            .ok_or(Error::UncaughtException {
                rendered: format!("expected \"undefined\", got {value:?}"),
            })
    }
}
