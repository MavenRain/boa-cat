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
